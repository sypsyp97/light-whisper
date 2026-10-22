#include "engine/framework/modules/transformers/qwen_causal_decode_runtime.h"
#include "r2t2_grouped_head_observer.h"
#include <cmath>
#include <fstream>
#include <iostream>
#include <memory>
#include <string>

namespace {
namespace core = engine::core;
namespace modules = engine::modules;
namespace obs = grouped_head_observer;
using obs::require;
constexpr int64_t width = 64, vocabulary = 97, prefix = 64, capacity = 128;
// Reserved by the handed-off graph contract; existing lowering seam, no new API dependency.
constexpr int32_t serial_equivalent_head_lowering = 4;
size_t cases = 0, work_red = 0;

std::vector<float> pattern(size_t count, uint32_t seed, float scale) {
    std::vector<float> values(count);
    for (auto & value : values) {
        seed = 1664525U * seed + 1013904223U;
        value = scale * float(int32_t(seed >> 16) - 32768) / 32768.0F;
    }
    return values;
}
struct Variant {
    const char * name;
    ggml_type head_type = GGML_TYPE_F16;
    bool bias = false, padded = false, ties = false;
    std::optional<ggml_type> cast;
    ggml_prec precision = GGML_PREC_DEFAULT;
};
struct Fixture {
    core::ExecutionContext execution;
    ggml_context * context = ggml_init({2 * 1024 * 1024, nullptr, true});
    ggml_backend_buffer_t buffer = nullptr;
    modules::QwenCausalDecodeRuntimeWeights weights;
    std::vector<core::TensorValue> tensors;
    Variant variant;
    core::TensorValue tensor(std::initializer_list<int64_t> shape, ggml_type type = GGML_TYPE_F32) {
        core::ModuleBuildContext ctx{context, "synthetic_grouped_head", execution.backend_type()};
        auto value = core::make_tensor(ctx, type, core::TensorShape::from_dims(shape));
        tensors.push_back(value);
        return value;
    }
    Fixture(core::BackendType backend, Variant variant) : execution({backend, 0, 1}), variant(variant) {
        require(context != nullptr && execution.backend_type() == backend, "requested backend unavailable");
        weights.token_embedding = tensor({vocabulary, width});
        weights.final_norm = {tensor({width}), std::nullopt};
        auto head = tensor({vocabulary, width + (variant.padded ? 2 : 0)}, variant.head_type);
        if (variant.padded) {
            auto * view = ggml_view_2d(context, head.tensor, width, vocabulary, head.tensor->nb[1], 0);
            head = core::wrap_tensor(view, core::TensorShape::from_dims({vocabulary, width}), variant.head_type);
            require(!ggml_is_contiguous(head.tensor), "padded fixture unexpectedly contiguous");
        }
        weights.lm_head = modules::LinearWeights{head, std::nullopt};
        if (variant.bias) weights.lm_head->bias = tensor({vocabulary});
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
        require(buffer != nullptr, "backend weight allocation failed");
        for (size_t i = 0; i < tensors.size(); ++i) {
            auto values = pattern(ggml_nelements(tensors[i].tensor), uint32_t(42 + 101 + i), 0.04F);
            if (tensors[i].shape.rank == 1) std::fill(values.begin(), values.end(), 1.0F);
            if (variant.ties && tensors[i].tensor == weights.lm_head->weight.tensor) std::fill(values.begin(), values.end(), 0.0F);
            core::write_tensor_float(tensors[i], values);
        }
        obs::head_weight = weights.lm_head->weight.tensor;
        obs::final_norm_weight = weights.final_norm.weight->tensor;
    }
    ~Fixture() { ggml_backend_buffer_free(buffer); ggml_free(context); }
};
modules::QwenCausalDecodeRuntimeConfig config_for(const Variant & variant) {
    modules::QwenCausalDecodeRuntimeConfig config;
    auto & stack = config.decoder.stack;
    stack.hidden_size = width; stack.num_attention_heads = 2; stack.num_key_value_heads = 1;
    stack.head_dim = width; stack.intermediate_size = 128; stack.layers = 2;
    stack.runtime.static_cache.update_mode = modules::QwenDecoderStaticCacheUpdateMode::DirectSetRows;
    stack.runtime.static_cache.set_rows_mode = modules::QwenDecoderStaticCacheSetRowsMode::BackendViewOptimized;
    stack.runtime.attention.prefill_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    stack.runtime.attention.static_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    config.decoder.logits_size = vocabulary;
    config.decoder.static_cache_type = GGML_TYPE_F16;
    config.decoder.use_lm_head_bias = variant.bias;
    config.decoder.lm_head_input_type = variant.cast;
    config.decoder.lm_head_precision = variant.precision;
    config.prefill_graph_arena_bytes = config.decode_graph_arena_bytes = 4 * 1024 * 1024;
    config.greedy_token_readback = true;
    return config;
}
int32_t token(const modules::QwenCausalDecodeStepResult & result) {
    if (result.token_id) return *result.token_id;
    require(!result.logits.empty(), "runtime returned neither token nor logits");
    return int32_t(std::max_element(result.logits.begin(), result.logits.end()) - result.logits.begin());
}
struct Started {
    std::unique_ptr<modules::QwenCausalDecodeRuntime> decoder;
    std::vector<ggml_tensor *> caches;
    obs::Cache initial;
    int32_t first;
};
Started start(Fixture & fixture) {
    Started state;
    state.decoder = std::make_unique<modules::QwenCausalDecodeRuntime>(fixture.execution, config_for(fixture.variant), fixture.weights);
    obs::reset();
    state.first = token(state.decoder->prefill_embeddings_into_cache(pattern(prefix * width, 43, 0.3F), prefix, capacity, prefix));
    require(obs::graphs.size() == 1 && obs::graphs[0].caches.size() == 4, "prefix graph did not expose four real K/V caches");
    state.caches = obs::graphs[0].caches;
    state.initial = obs::snapshot(state.caches);
    for (const auto & cache : state.initial) {
        require(std::any_of(cache.begin(), cache.end(), [](uint8_t value) { return value != 0; }), "cache fixture is vacuous zero data");
    }
    return state;
}
void cache_equal(const Started & subject, const Started & oracle, int64_t end) {
    const auto actual = obs::snapshot(subject.caches);
    require(actual == obs::snapshot(oracle.caches), "same-backend full K/V differs from serial oracle");
    require(subject.decoder->decode_current_end() == end && subject.decoder->decode_valid_steps() == end &&
            subject.decoder->decode_cache_steps() == capacity, "cache metadata differs from accepted prefix");
    const size_t committed = ggml_row_size(GGML_TYPE_F16, prefix * width);
    const size_t used = ggml_row_size(GGML_TYPE_F16, end * width);
    for (size_t i = 0; i < actual.size(); ++i) {
        require(std::memcmp(actual[i].data(), subject.initial[i].data(), committed) == 0, "committed prefix changed");
        require(std::all_of(actual[i].begin() + used, actual[i].end(), [](uint8_t value) { return value == 0; }), "rejected or padded future K/V survived");
    }
}
template<class T> void record(std::ofstream & trace, const T & value) {
    trace.write(reinterpret_cast<const char *>(&value), sizeof(value));
}
void record_cache(std::ofstream & trace, const Started & state) {
    for (const auto & bytes : obs::snapshot(state.caches)) {
        record(trace, uint64_t(bytes.size()));
        trace.write(reinterpret_cast<const char *>(bytes.data()), bytes.size());
    }
}
void run_case(core::BackendType backend, const Variant & variant, bool mismatch, std::ofstream & trace) {
    Fixture fixture(backend, variant);
    std::vector<int32_t> draft, predictions;
    std::vector<float> scalar_logits;
    obs::Arithmetic scalar_arithmetic;
    {
        auto generator = start(fixture);
        int32_t next = generator.first;
        for (int i = 0; i < 4; ++i) {
            draft.push_back(next);
            obs::reset();
            next = token(generator.decoder->decode_token(next));
            require(obs::graphs.size() == 1 && obs::graphs[0].heads.size() == 1 && obs::graphs[0].heads[0].columns == 1,
                    "serial oracle did not execute a scalar head");
        }
    }
    if (variant.ties) require(draft == std::vector<int32_t>(4, 0), "tie fixture did not use FIRST maximum");
    if (mismatch) draft[2] = (draft[2] + 1) % vocabulary;
    {
        // Force all supplied IDs, including rejected suffix IDs, through the
        // real scalar runtime. Reusing the greedy draft generator here would
        // compare the wrong context after the deliberately mismatched token.
        auto full_oracle = start(fixture);
        for (auto id : draft) {
            obs::reset();
            predictions.push_back(token(full_oracle.decoder->decode_token(id)));
            require(obs::graphs.size() == 1 && obs::graphs[0].heads.size() == 1 &&
                    obs::graphs[0].heads[0].columns == 1 && obs::graphs[0].logits.size() == vocabulary,
                    "full oracle did not execute and expose one complete scalar head");
            obs::add(scalar_arithmetic, obs::graphs[0].arithmetic);
            scalar_logits.insert(scalar_logits.end(), obs::graphs[0].logits.begin(), obs::graphs[0].logits.end());
        }
    }
    auto oracle = start(fixture);
    auto subject = start(fixture);
    require(subject.first == oracle.first && subject.initial == oracle.initial, "initial same-backend oracle state differs");
    int64_t accepted = 0;
    int32_t next = oracle.first;
    for (auto id : draft) {
        if (id != next) break;
        ++accepted;
        next = token(oracle.decoder->decode_token(id));
    }
    require(accepted == (mismatch ? 2 : 4), "fixture accepted an unexpected draft length");
    obs::reset();
    const auto actual = subject.decoder->verify_tokens(draft, subject.first);
    require(obs::graphs.size() == 1, "verification did not execute one real graph");
    const auto graph = obs::graphs[0];
    require(actual.accepted_tokens == accepted && actual.next_token == next, "verification token result differs from serial oracle");
    require(graph.predictions.size() == 4 && graph.first_max == 1, "verification did not use four FIRST-max outputs");
    for (int64_t i = 0; i < accepted; ++i) require(graph.predictions[size_t(i)] == predictions[size_t(i)], "accepted-row token differs from scalar oracle");
    require(graph.logits.size() == 4 * vocabulary && scalar_logits.size() == graph.logits.size(),
            "complete four-column verification logits were not captured");
    require(std::all_of(graph.logits.begin(), graph.logits.end(), [](float value) { return std::isfinite(value); }) &&
            std::all_of(scalar_logits.begin(), scalar_logits.end(), [](float value) { return std::isfinite(value); }),
            "synthetic head logits contain nonfinite values");
    for (int column = 0; column < 4; ++column) {
        require(std::memcmp(graph.logits.data() + column * vocabulary, scalar_logits.data() + column * vocabulary,
                            vocabulary * sizeof(float)) == 0,
                "verification column logits differ bit-for-bit from supplied-draft scalar oracle");
    }
    cache_equal(subject, oracle, prefix + accepted);
    require(graph.final_norms == 4 && graph.scalar_final_norms, "final RMSNorm was grouped or reused across tokens");
    require(graph.layer_matmuls == 56 && graph.scalar_layer_matmuls, "transformer projections stopped using scalar rows");
    require(graph.arithmetic == scalar_arithmetic, "transformer arithmetic shapes/counts differ from four scalar decode graphs");
    const bool eligible = backend == core::BackendType::Cuda && variant.head_type == GGML_TYPE_F16 &&
        !variant.bias && !variant.padded && !variant.cast && variant.precision == GGML_PREC_DEFAULT;
    bool work_ok = graph.heads.size() == (eligible ? 1U : 4U);
    for (const auto & head : graph.heads) {
        work_ok &= head.columns == (eligible ? 4 : 1) &&
            head.lowering == (eligible ? serial_equivalent_head_lowering : GGML_MUL_MAT_LOWERING_DEFAULT) &&
            head.precision == variant.precision && head.input_type == variant.cast.value_or(GGML_TYPE_F32);
    }
    if (!eligible) require(work_ok, "ineligible head did not retain four scalar default-lowering nodes");
    if (!work_ok) ++work_red;
    record(trace, actual.accepted_tokens); record(trace, actual.next_token);
    record(trace, uint64_t(graph.logits.size()));
    trace.write(reinterpret_cast<const char *>(graph.logits.data()), graph.logits.size() * sizeof(float));
    record_cache(trace, subject);
    for (int step = 0; step < 3; ++step) {
        // Distinct probe inputs exercise cache continuation even for a constant argmax fixture.
        const int32_t probe = (next + 13 * step) % vocabulary;
        const auto expected_next = token(oracle.decoder->decode_token(probe));
        const auto actual_next = token(subject.decoder->decode_token(probe));
        require(actual_next == expected_next, "continuation token differs after verification/trim");
        cache_equal(subject, oracle, prefix + accepted + step + 1);
        record(trace, actual_next); record_cache(trace, subject);
        next = expected_next;
    }
    ++cases;
    std::cout << "CASE backend=" << (backend == core::BackendType::Cuda ? "cuda" : "cpu")
              << " variant=" << variant.name << " accepted=" << accepted << " heads=" << graph.heads.size()
              << " logits=" << graph.logits.size() << " columns=";
    for (const auto & head : graph.heads) std::cout << head.columns << ',';
    std::cout << " lowering=";
    for (const auto & head : graph.heads) std::cout << head.lowering << ',';
    std::cout << " expected=" << (eligible ? "one4_hint4" : "four1_default")
              << " work=" << (work_ok ? "GREEN" : "RED") << '\n';
}
}  // namespace

int main(int argc, char ** argv) {
    try {
        require(argc == 4, "usage: grouped_head_test.exe cpu|cuda equivalence|work trace.bin");
        const std::string backend_name = argv[1], mode = argv[2];
        require(backend_name == "cpu" || backend_name == "cuda", "invalid backend");
        require(mode == "equivalence" || mode == "work", "invalid test mode");
        std::ofstream trace(argv[3], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open trace");
        const auto backend = backend_name == "cuda" ? core::BackendType::Cuda : core::BackendType::Cpu;
        run_case(backend, {"f16_default"}, false, trace);
        if (backend == core::BackendType::Cuda) {
            run_case(backend, {"f16_default_trim"}, true, trace);
            Variant ties{"f16_ties"}; ties.ties = true; run_case(backend, ties, false, trace);
            run_case(backend, {"f32_head", GGML_TYPE_F32}, false, trace);
            run_case(backend, {"bf16_head", GGML_TYPE_BF16}, false, trace);
            Variant bias{"bias"}; bias.bias = true; run_case(backend, bias, false, trace);
            Variant cast32{"explicit_f32_cast"}; cast32.cast = GGML_TYPE_F32; run_case(backend, cast32, false, trace);
            Variant cast16{"f16_cast"}; cast16.cast = GGML_TYPE_F16; run_case(backend, cast16, false, trace);
            Variant precision{"forced_f32"}; precision.precision = GGML_PREC_F32; run_case(backend, precision, false, trace);
            Variant padded{"padded_f16"}; padded.padded = true; run_case(backend, padded, false, trace);
        }
        require(trace.good(), "trace write failed");
        std::cout << "NUMERICAL_GREEN backend=" << backend_name << " cases=" << cases << " work_red=" << work_red
                  << " assertions=" << obs::assertions << " synthetic_only=1 timing_claim=0\n";
        if (mode == "work" && work_red) {
            std::cerr << "WORK_RED grouped-head graph contract failed in " << work_red << " cases\n";
            return 2;
        }
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
