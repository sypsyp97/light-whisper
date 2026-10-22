#pragma once
#include "engine/framework/core/backend.h"
#include <ggml-alloc.h>
#include <algorithm>
#include <cstring>
#include <map>
#include <set>
#include <sstream>
#include <stdexcept>
#include <vector>

namespace grouped_head_observer {
using Cache = std::vector<std::vector<uint8_t>>;
using Arithmetic = std::map<std::string, size_t>;
inline ggml_tensor * head_weight = nullptr;
inline ggml_tensor * final_norm_weight = nullptr;
inline size_t assertions = 0;
inline void require(bool value, const char * message) {
    ++assertions;
    if (!value) throw std::runtime_error(message);
}
inline ggml_tensor * root(ggml_tensor * tensor) {
    while (tensor->view_src) tensor = tensor->view_src;
    return tensor;
}
template<class T> std::vector<T> read(ggml_tensor * tensor) {
    require(tensor && ggml_is_contiguous(tensor), "observer needs a contiguous tensor");
    std::vector<T> values(ggml_nbytes(tensor) / sizeof(T));
    ggml_backend_tensor_get(tensor, values.data(), 0, ggml_nbytes(tensor));
    return values;
}
inline Cache snapshot(const std::vector<ggml_tensor *> & caches) {
    Cache out;
    for (auto * cache : caches) out.push_back(read<uint8_t>(cache));
    return out;
}
struct Head {
    int64_t columns;
    ggml_type input_type;
    int32_t precision, lowering;
};
struct Graph {
    std::vector<Head> heads;
    std::vector<ggml_tensor *> caches;
    std::vector<int32_t> predictions;
    std::vector<float> logits;
    Arithmetic arithmetic;
    size_t final_norms = 0, layer_matmuls = 0;
    bool scalar_final_norms = true, scalar_layer_matmuls = true;
    int32_t first_max = 0;
};
inline std::vector<Graph> graphs;
inline void reset() { graphs.clear(); }
inline bool arithmetic_op(ggml_op op) {
    switch (op) {
        case GGML_OP_MUL_MAT: case GGML_OP_RMS_NORM: case GGML_OP_ROPE:
        case GGML_OP_FLASH_ATTN_EXT: case GGML_OP_SET_ROWS: case GGML_OP_GLU:
        case GGML_OP_ADD: case GGML_OP_MUL: case GGML_OP_UNARY: return true;
        default: return false;
    }
}
inline void shape(std::ostringstream & out, const ggml_tensor * tensor) {
    out << ':' << int(tensor->type);
    for (auto extent : tensor->ne) out << ':' << extent;
}
inline std::string signature(ggml_tensor * node) {
    std::ostringstream out;
    out << int(node->op);
    shape(out, node);
    for (auto * source : node->src) { if (source) shape(out, source); }
    return out.str();
}
inline void add(Arithmetic & to, const Arithmetic & from) {
    for (const auto & item : from) to[item.first] += item.second;
}
}  // namespace grouped_head_observer

bool grouped_head_alloc(ggml_gallocr_t allocator, ggml_cgraph * graph) {
    // Retain the actual argmax input through readback; intermediate scalar
    // heads may otherwise share storage with later concatenation nodes.
    // Scalar decode allocates every tensor via ggml_backend_alloc_ctx_tensors.
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        auto * node = ggml_graph_node(graph, i);
        if (node->op == GGML_OP_ARGMAX && (node->flags & GGML_TENSOR_FLAG_OUTPUT)) {
            grouped_head_observer::require(node->src[0] != nullptr, "argmax logits input missing");
            ggml_set_output(node->src[0]);
        }
    }
    return ggml_gallocr_alloc_graph(allocator, graph);
}

namespace engine::core {
ggml_status grouped_head_compute(ggml_backend_t backend, ggml_cgraph * graph,
                                 ggml_backend_graph_plan_t plan, const char * label) {
    namespace obs = grouped_head_observer;
    obs::Graph seen;
    ggml_tensor * prediction = nullptr;
    std::set<ggml_tensor *> final_norms;
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        auto * node = ggml_graph_node(graph, i);
        const bool head = node->op == GGML_OP_MUL_MAT && node->src[0] == obs::head_weight;
        if (head) {
            seen.heads.push_back({node->ne[1], node->src[1]->type, node->op_params[0], node->op_params[1]});
        } else if (obs::arithmetic_op(node->op)) {
            ++seen.arithmetic[obs::signature(node)];
            if (node->op == GGML_OP_MUL_MAT) {
                ++seen.layer_matmuls;
                seen.scalar_layer_matmuls &= node->ne[1] == 1 && node->ne[2] == 1 && node->ne[3] == 1;
            }
        }
        if (node->op == GGML_OP_MUL && node->src[1] == obs::final_norm_weight) {
            auto * norm = node->src[0];
            seen.scalar_final_norms &= norm->op == GGML_OP_RMS_NORM &&
                norm->ne[1] == 1 && norm->ne[2] == 1 && norm->ne[3] == 1;
            final_norms.insert(norm);
        }
        if (node->op == GGML_OP_SET_ROWS) {
            auto * cache = obs::root(node->src[2]);
            if (std::find(seen.caches.begin(), seen.caches.end(), cache) == seen.caches.end()) seen.caches.push_back(cache);
        }
        if (node->op == GGML_OP_ARGMAX && (node->flags & GGML_TENSOR_FLAG_OUTPUT)) {
            obs::require(prediction == nullptr, "multiple output argmax nodes");
            prediction = node;
            seen.first_max = node->op_params[0];
        }
    }
    seen.final_norms = final_norms.size();
    const auto status = compute_backend_graph(backend, graph, plan, label);
    ggml_backend_synchronize(backend);
    if (status == GGML_STATUS_SUCCESS) {
        if (prediction) {
            auto * logits = prediction->src[0];
            obs::require(logits && logits->type == GGML_TYPE_F32 &&
                         logits->ne[0] == obs::head_weight->ne[1] &&
                         logits->ne[1] == ggml_nelements(prediction) && logits->ne[2] == 1 && logits->ne[3] == 1,
                         "argmax input is not the complete F32 head-logits matrix");
            seen.logits = obs::read<float>(logits);
            seen.predictions = obs::read<int32_t>(prediction);
        }
        obs::graphs.push_back(std::move(seen));
    }
    return status;
}
}  // namespace engine::core
