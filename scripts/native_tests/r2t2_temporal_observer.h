#pragma once
#include "engine/framework/core/backend.h"
#include <ggml-alloc.h>
#include <algorithm>
#include <cstring>
#include <map>
#include <set>
#include <stdexcept>
#include <vector>

namespace temporal_observer {
constexpr int64_t width = 64, vocabulary = 97;
using Bytes = std::vector<uint8_t>;
using Cache = std::vector<Bytes>;
inline size_t assertions = 0;
inline void check(bool value, const char * message) {
    ++assertions;
    if (!value) { throw std::runtime_error(message); }
}
template<class T> std::vector<T> read(const ggml_tensor * tensor) {
    check(tensor && ggml_is_contiguous(tensor), "observer requires contiguous tensor");
    check(ggml_nbytes(tensor) % sizeof(T) == 0, "observer element size mismatch");
    std::vector<T> out(ggml_nbytes(tensor) / sizeof(T));
    ggml_backend_tensor_get(tensor, out.data(), 0, ggml_nbytes(tensor));
    return out;
}
template<class T> bool exact(const std::vector<T> & a, const std::vector<T> & b) {
    return a.size() == b.size() && (a.empty() || std::memcmp(a.data(), b.data(), a.size() * sizeof(T)) == 0);
}
struct Upload { ggml_backend_t backend; ggml_tensor * tensor; const void * source; size_t offset; Bytes snapshot; };
struct Graph {
    ggml_cgraph * graph = nullptr;
    std::vector<ggml_tensor *> caches;
    std::vector<int32_t> positions, slots, input_ids, predictions;
    std::vector<ggml_fp16_t> mask;
    std::vector<float> logits, hidden;
    Cache before, after;
    bool verification = false, scalar_rows = false;
};
inline std::vector<Upload> pending;
inline std::vector<Graph> graphs;
inline std::map<ggml_cgraph *, std::vector<ggml_backend_buffer_t>> live_verification;
inline size_t uploads = 0, readbacks = 0, readback_bytes = 0, drains = 0, calls = 0;
inline size_t queued_consumed = 0, max_live_verification = 0;

inline int fail_upload = 0;
inline bool fail_compute_before = false, fail_compute_after = false;
inline bool preserve_intermediates = true;
inline void reset() {
    check(pending.empty(), "previous operation leaked queued uploads");
    graphs.clear(); uploads = readbacks = readback_bytes = drains = calls = 0;
    fail_upload = 0; fail_compute_before = fail_compute_after = false;
}
inline Cache snapshot(const std::vector<ggml_tensor *> & caches) {
    Cache out;
    for (auto * tensor : caches) { out.push_back(read<uint8_t>(tensor)); }
    return out;
}
inline void drain(ggml_backend_t backend) {
    for (const auto & upload : pending) {
        check(upload.backend == backend, "queued upload has different backend");
        check(std::memcmp(upload.source, upload.snapshot.data(), upload.snapshot.size()) == 0,
              "queued source changed before consumption");
        ggml_backend_tensor_set(upload.tensor, upload.source, upload.offset, upload.snapshot.size());
        ++queued_consumed;
    }
    pending.clear();
}
inline ggml_tensor * prediction_node(ggml_cgraph * graph) {
    ggml_tensor * result = nullptr;
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        auto * node = ggml_graph_node(graph, i);
        if (node->op == GGML_OP_ARGMAX && (node->flags & GGML_TENSOR_FLAG_OUTPUT)) {
            check(result == nullptr, "multiple output argmax tensors");
            result = node;
        }
    }
    return result;
}
inline ggml_tensor * root(ggml_tensor * tensor) {
    while (tensor->view_src) { tensor = tensor->view_src; }
    return tensor;
}
inline bool collect_logits(ggml_tensor * tensor, std::vector<ggml_tensor *> & out) {
    if (!tensor || tensor->ne[0] != vocabulary) { return false; }
    if (tensor->op == GGML_OP_MUL_MAT) { out.push_back(tensor); return true; }
    if (tensor->op == GGML_OP_CONCAT) {

        if (tensor->op_params[0] != 1) { return false; }
        return collect_logits(tensor->src[0], out) && collect_logits(tensor->src[1], out);
    }
    if (tensor->op == GGML_OP_CONT || tensor->op == GGML_OP_RESHAPE || tensor->op == GGML_OP_VIEW) {
        return tensor->src[0] && ggml_nelements(tensor) == ggml_nelements(tensor->src[0]) &&
            tensor->view_offs == 0 && collect_logits(tensor->src[0], out);
    }
    return false;
}
inline std::vector<ggml_tensor *> logits_nodes(ggml_cgraph * graph) {
    std::vector<ggml_tensor *> out;
    if (auto * prediction = prediction_node(graph)) {
        check(collect_logits(prediction->src[0], out), "argmax source is not ordered real LM logits");
    } else {
        for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
            auto * node = ggml_graph_node(graph, i);
            if (node->op == GGML_OP_MUL_MAT && node->ne[0] == vocabulary) { out.push_back(node); }
        }
    }
    return out;
}
inline void check_row_views(const std::vector<ggml_tensor *> & sources, ggml_tensor * input,
                            size_t elements_per_row, size_t copies_per_row) {
    check(input && (input->flags & GGML_TENSOR_FLAG_INPUT), "row root is not a real uploaded input");
    check(sources.size() == 4 * copies_per_row, "wrong scalar-row decoder operation count");
    check(ggml_nelements(input) == static_cast<int64_t>(4 * elements_per_row), "wrong four-row root shape");
    const size_t bytes_per_row = ggml_row_size(input->type, elements_per_row);
    const auto full = read<uint8_t>(input);
    std::vector<size_t> counts(4);
    for (auto * source : sources) {
        check(root(source) == input && source != input && ggml_is_contiguous(source), "decoder row is not a shared-root view");
        check(source->type == input->type && ggml_nelements(source) == static_cast<int64_t>(elements_per_row),
              "decoder scalar-row view shape/type differs");
        check(source->view_offs % bytes_per_row == 0 && source->view_offs / bytes_per_row < 4,
              "decoder row offset outside uploaded root");
        const size_t row_index = source->view_offs / bytes_per_row;
        const auto actual = read<uint8_t>(source);
        check(actual.size() == bytes_per_row && std::memcmp(actual.data(), full.data() + source->view_offs, bytes_per_row) == 0,
              "decoder scalar row differs from its uploaded root row");
        ++counts[row_index];
    }
    for (const auto count : counts) { check(count == copies_per_row, "decoder does not use each root row in every layer"); }
}
inline std::vector<float> row(const std::vector<float> & values, size_t index, size_t size) {
    check(values.size() >= (index + 1) * size, "observed output row unavailable");
    return {values.begin() + index * size, values.begin() + (index + 1) * size};
}
}

void temporal_set_async(ggml_backend_t backend, ggml_tensor * tensor, const void * data, size_t offset, size_t size) {
    using namespace temporal_observer;
    check(data && tensor && size && offset + size <= ggml_nbytes(tensor), "invalid queued upload");
    const auto * bytes = static_cast<const uint8_t *>(data);
    pending.push_back({backend, tensor, data, offset, {bytes, bytes + size}});
    ++uploads;
    if (fail_upload && --fail_upload == 0) { throw std::runtime_error("injected queued-upload failure"); }
}
void temporal_set(ggml_tensor * tensor, const void * data, size_t offset, size_t size) {
    ++temporal_observer::uploads;
    temporal_observer::check(temporal_observer::pending.empty(), "sync write before queued uploads drained");
    ggml_backend_tensor_set(tensor, data, offset, size);
}
void temporal_get(const ggml_tensor * tensor, void * data, size_t offset, size_t size) {
    ++temporal_observer::readbacks;
    temporal_observer::readback_bytes += size;
    ggml_backend_tensor_get(tensor, data, offset, size);
}
void temporal_sync(ggml_backend_t backend) {
    if (!temporal_observer::pending.empty()) { ++temporal_observer::drains; }
    temporal_observer::drain(backend);
    ggml_backend_synchronize(backend);
}
bool temporal_alloc(ggml_gallocr_t allocator, ggml_cgraph * graph) {


    if (temporal_observer::preserve_intermediates) {
        for (auto * logits : temporal_observer::logits_nodes(graph)) {
            ggml_set_output(logits);
            ggml_set_output(logits->src[1]);
            ggml_set_output(temporal_observer::root(logits->src[1]));
        }
    }
    return ggml_gallocr_alloc_graph(allocator, graph);
}
void temporal_buffer_free(ggml_backend_buffer_t buffer) {
    for (const auto & item : temporal_observer::live_verification) {
        temporal_observer::check(std::find(item.second.begin(), item.second.end(), buffer) == item.second.end(),
                                 "decode cache freed before verification graph release");
    }
    ggml_backend_buffer_free(buffer);
}
namespace engine::core {
void temporal_release(ggml_backend_t backend, ggml_cgraph * graph, bool evict) {
    temporal_observer::live_verification.erase(graph);
    release_backend_graph_resources(backend, graph, evict);
}
void temporal_release(BackendType type, ggml_backend_t backend, ggml_cgraph * graph, bool evict) {
    temporal_observer::live_verification.erase(graph);
    release_backend_graph_resources(type, backend, graph, evict);
}
ggml_status temporal_compute(ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {
    using namespace temporal_observer;
    ++calls;
    if (fail_compute_before) { fail_compute_before = false; return GGML_STATUS_FAILED; }
    drain(backend);
    Graph observation;
    observation.graph = graph;
    const auto logits = logits_nodes(graph);
    auto * predictions = prediction_node(graph);
    std::vector<ggml_tensor *> positions, slots, masks;
    std::map<ggml_tensor *, std::vector<ggml_tensor *>> cache_slots;
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        auto * node = ggml_graph_node(graph, i);
        if (node->op == GGML_OP_SET_ROWS) {
            slots.push_back(node->src[1]);
            auto * cache = root(node->src[2]);
            cache_slots[cache].push_back(node->src[1]);
            if (std::find(observation.caches.begin(), observation.caches.end(), cache) == observation.caches.end()) {
                observation.caches.push_back(cache);
            }
        }
        if (node->op == GGML_OP_ROPE) {
            positions.push_back(node->src[1]);
        }
        if (node->op == GGML_OP_FLASH_ATTN_EXT) {
            masks.push_back(node->src[3]);
        }
        if (node->op == GGML_OP_GET_ROWS && node->src[0]->ne[0] == temporal_observer::width &&
            node->src[0]->ne[1] == temporal_observer::vocabulary &&
            node->src[1]->type == GGML_TYPE_I32) { observation.input_ids = read<int32_t>(node->src[1]); }
    }
    const bool four_token_output = observation.input_ids.size() == 4 &&
        predictions && predictions->type == GGML_TYPE_I32 && ggml_nelements(predictions) == 4 &&
        observation.caches.size() == 4;
    if (four_token_output) {
        check(!positions.empty() && !slots.empty() && !masks.empty(), "verification decoder inputs missing");
        observation.scalar_rows = ggml_nelements(positions.front()) == 1;
        if (observation.scalar_rows) {
            auto * position_root = root(positions.front());
            auto * mask_root = root(masks.front());
            check(position_root->type == GGML_TYPE_I32 && mask_root->type == GGML_TYPE_F16,
                  "verification root input types differ");
            check_row_views(positions, position_root, 1, 4);
            check_row_views(slots, position_root, 1, 4);
            check_row_views(masks, mask_root, ggml_nelements(mask_root) / 4, 2);
            for (const auto & item : cache_slots) { check_row_views(item.second, position_root, 1, 1); }
            observation.positions = read<int32_t>(position_root);
            observation.slots = observation.positions;
            observation.mask = read<ggml_fp16_t>(mask_root);
            check(logits.size() == 4, "scalar verification requires four real LM rows");
            std::set<ggml_tensor *> unique_logits;
            for (auto * output : logits) {
                check(ggml_nelements(output) == temporal_observer::vocabulary &&
                      ggml_nelements(output->src[1]) == temporal_observer::width,
                      "scalar verification LM/normalized hidden is not one row");
                unique_logits.insert(output);
            }
            check(unique_logits.size() == 4, "scalar verification reused/duplicated an LM row");
        } else {
            check(positions.size() == 4 && slots.size() == 4 && masks.size() == 2,
                  "dense verification decoder operation counts differ");
            check(ggml_nelements(positions.front()) == 4 && ggml_nelements(slots.front()) == 4,
                  "dense verification positions/slots do not have four rows");
            check(logits.size() == 1 && ggml_nelements(logits.front()) == 4 * temporal_observer::vocabulary &&
                  ggml_nelements(logits.front()->src[1]) == 4 * temporal_observer::width,
                  "dense verification LM/hidden row count differs");
        }
        observation.verification = true;
    }
    if (!observation.scalar_rows) {
        for (auto * source : positions) {
            const auto values = read<int32_t>(source);
            if (observation.positions.empty()) { observation.positions = values; }
            else { check(observation.positions == values, "layer positions differ"); }
        }
        for (auto * source : slots) {
            const auto values = read<int32_t>(source);
            if (observation.slots.empty()) { observation.slots = values; }
            else { check(observation.slots == values, "layer slots differ"); }
        }
        for (auto * source : masks) {
            const auto values = read<ggml_fp16_t>(source);
            if (observation.mask.empty()) { observation.mask = values; }
            else { check(observation.mask == values, "layer masks differ"); }
        }
    }
    if (observation.verification) {
        check(predictions->op_params[0] == 1, "verification argmax does not request FIRST maximum");
        std::vector<ggml_backend_buffer_t> buffers;
        for (auto * cache : observation.caches) { buffers.push_back(cache->buffer); }
        live_verification[graph] = std::move(buffers);
        max_live_verification = std::max(max_live_verification, live_verification.size());
    }
    observation.before = snapshot(observation.caches);
    const auto status = compute_backend_graph(backend, graph, plan, label);
    ggml_backend_synchronize(backend);
    if (status == GGML_STATUS_SUCCESS) {
        observation.after = snapshot(observation.caches);
        if (preserve_intermediates) {
            for (auto * output : logits) {
                const auto values = read<float>(output);
                const auto hidden = read<float>(output->src[1]);
                observation.logits.insert(observation.logits.end(), values.begin(), values.end());
                observation.hidden.insert(observation.hidden.end(), hidden.begin(), hidden.end());
            }
        }
        if (predictions) { observation.predictions = read<int32_t>(predictions); }
        graphs.push_back(std::move(observation));
    }
    if (fail_compute_after) { fail_compute_after = false; return GGML_STATUS_FAILED; }
    return status;
}
}
