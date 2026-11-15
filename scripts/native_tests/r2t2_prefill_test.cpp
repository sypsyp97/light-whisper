#include "engine/framework/modules/transformers/qwen_causal_decode_runtime.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iostream>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
namespace core = engine::core;
namespace modules = engine::modules;
constexpr int64_t width = 64;
constexpr int64_t vocabulary = 97;
constexpr const char * head_name = "thinker.lm_head.weight";
constexpr const char * norm_name = "thinker.model.norm.weight";
struct GraphWork { int head = 0; int norm = 0; const ggml_tensor * cache_key = nullptr; };
std::vector<GraphWork> work;
size_t assertions = 0;
size_t cases = 0;
size_t work_failures = 0;
std::set<int32_t> greedy_tokens;

void require(bool condition, const std::string & message) {
    ++assertions;
    if (!condition) { throw std::runtime_error(message); }
}



std::vector<float> pattern(size_t count, uint32_t seed, float scale) {
    std::vector<float> values(count);
    for (auto & value : values) {
        seed = 1664525U * seed + 1013904223U;
        value = scale * static_cast<float>(static_cast<int32_t>(seed >> 16) - 32768) / 32768.0F;
    }
    return values;
}

struct Fixture {
    core::ExecutionContext execution{{core::BackendType::Cpu, 0, 1}};
    ggml_context * context = ggml_init({2 * 1024 * 1024, nullptr, true});
    ggml_backend_buffer_t buffer = nullptr;
    modules::QwenCausalDecodeRuntimeWeights weights;
    std::vector<core::TensorValue> tensors;

    core::TensorValue tensor(std::initializer_list<int64_t> shape, const char * name = "") {
        core::ModuleBuildContext ctx{context, "synthetic_r2t2_prefill_test", core::BackendType::Cpu};
        auto value = core::make_tensor(ctx, GGML_TYPE_F32, core::TensorShape::from_dims(shape));
        ggml_set_name(value.tensor, name);
        tensors.push_back(value);
        return value;
    }

    Fixture() {
        require(context != nullptr, "synthetic weight context allocation failed");
        weights.token_embedding = tensor({vocabulary, width}, "thinker.model.embed_tokens.weight");
        weights.final_norm = {tensor({width}, norm_name), std::nullopt};
        weights.lm_head = modules::LinearWeights{tensor({vocabulary, width}, head_name), std::nullopt};
        for (int i = 0; i < 2; ++i) {
            modules::QwenDecoderLayerWeights layer;
            layer.input_norm = {tensor({width}), std::nullopt};
            layer.post_norm = {tensor({width}), std::nullopt};
            layer.q_norm = {tensor({width}), std::nullopt};
            layer.k_norm = {tensor({width}), std::nullopt};
            layer.self_attention.q_weight = tensor({128, width});
            layer.self_attention.k_weight = tensor({width, width});
            layer.self_attention.v_weight = tensor({width, width});
            layer.self_attention.out_weight = tensor({width, 128});
            layer.mlp.gate_proj = {tensor({128, width}), std::nullopt};
            layer.mlp.up_proj = {tensor({128, width}), std::nullopt};
            layer.mlp.down_proj = {tensor({width, 128}), std::nullopt};
            weights.stack.layers.push_back(layer);
        }
        buffer = ggml_backend_alloc_ctx_tensors(context, execution.backend());
        require(buffer != nullptr, "synthetic weight buffer allocation failed");
        for (size_t i = 0; i < tensors.size(); ++i) {
            const auto & value = tensors[i];
            auto values = pattern(static_cast<size_t>(ggml_nelements(value.tensor)),
                                  static_cast<uint32_t>(101 + i), 0.04F);
            if (value.shape.rank == 1) { std::fill(values.begin(), values.end(), 1.0F); }
            core::write_tensor_f32(value, values);
        }
    }

    ~Fixture() {
        ggml_backend_buffer_free(buffer);
        ggml_free(context);
    }
};

modules::QwenCausalDecodeRuntimeConfig config_for(int mode) {
    modules::QwenCausalDecodeRuntimeConfig config;
    auto & stack = config.decoder.stack;
    stack.hidden_size = width;
    stack.num_attention_heads = 2;
    stack.num_key_value_heads = 1;
    stack.head_dim = width;
    stack.intermediate_size = 128;
    stack.layers = 2;
    stack.runtime.static_cache.update_mode = modules::QwenDecoderStaticCacheUpdateMode::DirectSetRows;
    stack.runtime.static_cache.set_rows_mode = modules::QwenDecoderStaticCacheSetRowsMode::BackendViewOptimized;
    stack.runtime.attention.prefill_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    stack.runtime.attention.static_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    config.decoder.logits_size = vocabulary;
    config.decoder.static_cache_type = GGML_TYPE_F16;
    config.prefill_graph_arena_bytes = 4 * 1024 * 1024;
    config.decode_graph_arena_bytes = 4 * 1024 * 1024;
    config.return_hidden = mode == 1;
    if (mode == 2) { config.output_mode = modules::QwenCausalDecodeOutputMode::Hidden; }
    return config;
}

template <typename T>
void record(std::ofstream & trace, const T & value) {
    trace.write(reinterpret_cast<const char *>(&value), sizeof(value));
}

void record_result(std::ofstream & trace, const modules::QwenCausalDecodeStepResult & result, int mode) {
    require(result.logits.size() == (mode == 2 ? 0U : static_cast<size_t>(vocabulary)), "logit shape changed");
    require(result.hidden.size() == (mode == 0 ? 0U : static_cast<size_t>(width)), "hidden shape changed");
    for (const auto * values : {&result.logits, &result.hidden}) {
        record(trace, static_cast<uint64_t>(values->size()));
        for (const float value : *values) {
            require(std::isfinite(value), "nonfinite runtime output");
            record(trace, value);
        }
    }
}

void check_work(const std::string & label, int64_t steps, int64_t chunk, int mode, bool enforce) {
    const auto blocks = static_cast<size_t>((steps + std::min(steps, chunk) - 1) / std::min(steps, chunk));
    require(work.size() == blocks, label + ": graph counter missed actual prefill compute");
    bool correct = true;
    int heads = 0;
    int norms = 0;
    for (size_t i = 0; i < work.size(); ++i) {
        const bool final = i + 1 == work.size();
        correct = correct && work[i].head == (final && mode != 2 ? 1 : 0) && work[i].norm == (final ? 1 : 0);


        if (!final) { correct = correct && work[i].cache_key != work.back().cache_key; }
        heads += work[i].head;
        norms += work[i].norm;
    }

    if (blocks == 1) { require(correct, label + ": output operation counter missed single-block work"); }
    if (enforce && !correct) { ++work_failures; }
    std::cout << (correct ? "WORK_OK " : "WORK_RED ") << label << " graphs=" << blocks
              << " lm_heads=" << heads << " final_norms=" << norms << " per_graph=";
    for (const auto & graph : work) { std::cout << graph.head << '/' << graph.norm << ','; }
    std::cout << " distinct_prefix_final_key=" << (blocks == 1 || work.front().cache_key != work.back().cache_key)
              << " expected_final=" << (mode == 2 ? 0 : 1) << "/1 intermediate=0/0\n";
}

void run_case(modules::QwenCausalDecodeRuntime & runtime, std::ofstream & trace,
              int mode, int64_t steps, int64_t capacity, int64_t chunk, uint32_t seed, bool enforce) {
    const auto label = "mode" + std::to_string(mode) + "-case" + std::to_string(cases++) +
        "-steps" + std::to_string(steps) + "-chunk" + std::to_string(chunk);
    work.clear();
    auto result = runtime.prefill_embeddings_into_cache(pattern(static_cast<size_t>(steps * width), seed, 0.3F),
                                                        steps, capacity, chunk);
    check_work(label, steps, chunk, mode, enforce);
    require(runtime.decode_current_end() == steps, label + ": wrong prefill position");
    require(runtime.decode_valid_steps() == steps, label + ": wrong prefill valid steps");
    require(runtime.decode_cache_steps() >= capacity, label + ": insufficient cache capacity");
    record(trace, steps);
    record(trace, runtime.decode_cache_steps());
    record_result(trace, result, mode);
    for (int i = 0; i < 4; ++i) {
        const int32_t token = mode == 2 ? 7 + i * 19 : static_cast<int32_t>(
            std::max_element(result.logits.begin(), result.logits.end()) - result.logits.begin());
        greedy_tokens.insert(token);
        record(trace, token);
        result = runtime.decode_token(token);
        require(runtime.decode_current_end() == steps + i + 1, label + ": wrong continuation position");
        require(runtime.decode_valid_steps() == steps + i + 1, label + ": wrong continuation valid steps");
        record(trace, runtime.decode_current_end());
        record(trace, runtime.decode_valid_steps());
        record_result(trace, result, mode);
    }
}

void suite(Fixture & fixture, std::ofstream & trace, bool enforce) {
    for (int mode = 0; mode < 3; ++mode) {
        auto weights = fixture.weights;
        if (mode == 2) { weights.lm_head.reset(); }
        modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(mode), weights);


        for (const int64_t steps : {1, 63, 64, 65, 127, 128, 129, 129, 3, 193, 5}) {
            run_case(runtime, trace, mode, steps, steps > 129 ? 320 : 256, 64,
                     static_cast<uint32_t>(300 + cases), enforce);
        }
        run_case(runtime, trace, mode, 65, 320, 17, 719, enforce);
        runtime.release_runtime_graphs();
        require(runtime.decode_current_end() == 0 && runtime.decode_valid_steps() == 0 &&
                runtime.decode_cache_steps() == 0, "release_runtime_graphs did not reset cache state");
        run_case(runtime, trace, mode, 65, 192, 64, 809, enforce);
    }
}
}

namespace engine::core {
ggml_status r2t2_test_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {
    GraphWork observed;
    observed.cache_key = ggml_graph_node(graph, 0);
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        const auto * node = ggml_graph_node(graph, i);
        if (node->op == GGML_OP_MUL_MAT && node->src[0] &&
            std::strcmp(ggml_get_name(node->src[0]), head_name) == 0) { ++observed.head; }
        if (node->op == GGML_OP_MUL && node->src[1] &&
            std::strcmp(ggml_get_name(node->src[1]), norm_name) == 0) { ++observed.norm; }
    }
    const auto status = compute_backend_graph(backend, graph, plan, label);
    if (status == GGML_STATUS_SUCCESS) { work.push_back(observed); }
    return status;
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_prefill_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "unknown test mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open exact output trace");
        Fixture fixture;
        suite(fixture, trace, mode == "work");
        require(greedy_tokens.size() > 4, "synthetic inputs produced insufficient greedy token diversity");
        require(trace.good(), "could not write complete exact output trace");
        std::cout << "QUALITY_OK synthetic_only cases=" << cases << " assertions=" << assertions
                  << " continuation_steps=" << cases * 4 << " distinct_tokens=" << greedy_tokens.size() << '\n';
        require(work_failures == 0, "intermediate output work remains in " + std::to_string(work_failures) +
                " cases; LM head/final normalization must execute only on final prefill block");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
