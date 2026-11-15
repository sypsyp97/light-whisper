#include "engine/framework/modules/transformers/qwen_causal_decode_runtime.h"
#include "r2t2_temporal_observer.h"
#include <fstream>
#include <iostream>
#include <limits>
#include <memory>
#include <string>

#ifndef TEMPORAL_NATIVE
#define TEMPORAL_NATIVE 0
#endif

namespace {
namespace core = engine::core;
namespace modules = engine::modules;
namespace runtime = engine::runtime;
namespace obs = temporal_observer;
constexpr int64_t width = obs::width, vocabulary = obs::vocabulary, prefix_steps = 64, capacity = 128;
size_t cases = 0, guards = 0, work_red = 0, recoveries = 0;
size_t dense_verifications = 0, scalar_verifications = 0;
void require(bool value, const char * message) { obs::check(value, message); }
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
    core::TensorValue tensor(std::initializer_list<int64_t> shape) {
        core::ModuleBuildContext ctx{context, "synthetic_temporal_verify", core::BackendType::Cpu};
        auto value = core::make_tensor(ctx, GGML_TYPE_F32, core::TensorShape::from_dims(shape));
        tensors.push_back(value); return value;
    }
    Fixture(uint32_t seed, bool tie = false) {
        require(context != nullptr, "weight context allocation failed");
        weights.token_embedding = tensor({vocabulary, width});
        weights.final_norm = {tensor({width}), std::nullopt};
        weights.lm_head = modules::LinearWeights{tensor({vocabulary, width}), std::nullopt};
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
        require(buffer != nullptr, "weight buffer allocation failed");
        for (size_t i = 0; i < tensors.size(); ++i) {
            auto values = pattern(ggml_nelements(tensors[i].tensor), seed + static_cast<uint32_t>(101 + i), 0.04F);
            if (tensors[i].shape.rank == 1) { std::fill(values.begin(), values.end(), 1.0F); }
            if (tie && tensors[i].tensor == weights.lm_head->weight.tensor) { std::fill(values.begin(), values.end(), 0.0F); }
            core::write_tensor_f32(tensors[i], values);
        }
    }
    ~Fixture() { ggml_backend_buffer_free(buffer); ggml_free(context); }
};
modules::QwenCausalDecodeRuntimeConfig config_for(ggml_type type) {
    modules::QwenCausalDecodeRuntimeConfig config;
    auto & stack = config.decoder.stack;
    stack.hidden_size = width; stack.num_attention_heads = 2; stack.num_key_value_heads = 1;
    stack.head_dim = width; stack.intermediate_size = 128; stack.layers = 2;
    stack.runtime.static_cache.update_mode = modules::QwenDecoderStaticCacheUpdateMode::DirectSetRows;
    stack.runtime.static_cache.set_rows_mode = modules::QwenDecoderStaticCacheSetRowsMode::BackendViewOptimized;
    stack.runtime.attention.prefill_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    stack.runtime.attention.static_mode = modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    config.decoder.logits_size = vocabulary; config.decoder.static_cache_type = type;
    config.prefill_graph_arena_bytes = config.decode_graph_arena_bytes = 4 * 1024 * 1024;
    config.greedy_token_readback = true;
    return config;
}
int32_t token(const modules::QwenCausalDecodeStepResult & result) {
    if (result.token_id) { return *result.token_id; }
    require(!result.logits.empty(), "missing serial prediction");
    return static_cast<int32_t>(std::max_element(result.logits.begin(), result.logits.end()) - result.logits.begin());
}
struct Verified { int64_t accepted_tokens; int32_t next_token; };
Verified serial_verify(modules::QwenCausalDecodeRuntime & decoder, const std::vector<int32_t> & draft, int32_t first) {
    Verified result{0, first};
    for (const auto id : draft) {
        if (id != result.next_token) { break; }
        ++result.accepted_tokens;
        result.next_token = token(decoder.decode_token(id));
    }
    return result;
}
Verified verify(modules::QwenCausalDecodeRuntime & decoder, const std::vector<int32_t> & draft, int32_t first) {
#if TEMPORAL_NATIVE
    const auto result = decoder.verify_tokens(draft, first);
    return {result.accepted_tokens, result.next_token};
#else
    return serial_verify(decoder, draft, first);
#endif
}
struct Started {
    std::unique_ptr<modules::QwenCausalDecodeRuntime> decoder;
    std::vector<ggml_tensor *> caches;
    obs::Cache initial;
    int32_t first;
};
Started start(Fixture & fixture, ggml_type type, uint32_t seed, int64_t cache_steps = capacity) {
    Started out;
    out.decoder = std::make_unique<modules::QwenCausalDecodeRuntime>(fixture.execution, config_for(type), fixture.weights);
    obs::reset();
    out.first = token(out.decoder->prefill_embeddings_into_cache(pattern(prefix_steps * width, seed, 0.3F),
                                                               prefix_steps, cache_steps, prefix_steps));
    require(obs::graphs.size() == 1 && obs::graphs.back().caches.size() == 4, "prefix observer missed real K/V");
    out.caches = obs::graphs.back().caches;
    out.initial = obs::snapshot(out.caches);
    return out;
}
void metadata(const Started & state, int64_t expected) {
    require(state.decoder->decode_current_end() == expected && state.decoder->decode_valid_steps() == expected,
            "accepted-prefix metadata differs");
}
void future_zero(const obs::Cache & cache, ggml_type type, int64_t end) {
    const size_t offset = ggml_row_size(type, end * width);
    for (const auto & bytes : cache) {
        require(bytes.size() >= offset && std::all_of(bytes.begin() + offset, bytes.end(), [](uint8_t x) { return x == 0; }),
                "discarded/padded future K/V is nonzero");
    }
}
void preserved_prefix(const obs::Cache & a, const obs::Cache & b, ggml_type type, int64_t end) {
    const size_t size = ggml_row_size(type, end * width);
    require(a.size() == b.size(), "cache count changed");
    for (size_t i = 0; i < a.size(); ++i) {
        require(a[i].size() >= size && b[i].size() >= size && std::memcmp(a[i].data(), b[i].data(), size) == 0,
                "previously committed K/V changed");
    }
}
template<class T> void record(std::ofstream & stream, const T & value) {
    stream.write(reinterpret_cast<const char *>(&value), sizeof(value));
}
template<class T> void record_vector(std::ofstream & stream, const std::vector<T> & values) {
    record(stream, static_cast<uint64_t>(values.size()));
    stream.write(reinterpret_cast<const char *>(values.data()), values.size() * sizeof(T));
}
void record_cache(std::ofstream & stream, const obs::Cache & cache) {
    record(stream, static_cast<uint64_t>(cache.size()));
    for (const auto & bytes : cache) { record_vector(stream, bytes); }
}
std::vector<int32_t> continuation(Fixture & fixture, ggml_type type, uint32_t seed) {
    auto oracle = start(fixture, type, seed);
    std::vector<int32_t> ids;
    int32_t next = oracle.first;
    for (int i = 0; i < 4; ++i) { ids.push_back(next); next = token(oracle.decoder->decode_token(next)); }
    return ids;
}
void numerical_case(Fixture & fixture, ggml_type type, uint32_t seed, const std::vector<int32_t> & draft,
                    int64_t expected_accepted, std::ofstream & trace, bool enforce) {
    auto oracle = start(fixture, type, seed);
    auto subject = start(fixture, type, seed);
    require(oracle.first == subject.first && oracle.initial == subject.initial, "initial oracle state differs");
    future_zero(subject.initial, type, prefix_steps);
    obs::reset();
    const auto expected = serial_verify(*oracle.decoder, draft, oracle.first);
    const auto serial_graphs = obs::graphs;
    const auto expected_cache = obs::snapshot(oracle.caches);
    obs::reset();
    const auto actual = verify(*subject.decoder, draft, subject.first);
    require(obs::pending.empty(), "verification returned with pending input pointers");
    const auto observed = obs::graphs;
    const auto actual_cache = obs::snapshot(subject.caches);
    require(expected.accepted_tokens == expected_accepted, "mismatch fixture has wrong accepted prefix");
    require(actual.accepted_tokens == expected.accepted_tokens && actual.next_token == expected.next_token,
            "verification acceptance/next token differs from serial FIRST-max oracle");
    metadata(subject, prefix_steps + expected_accepted);
    require(subject.decoder->decode_cache_steps() == capacity, "verification changed cache capacity");
    require(actual_cache == expected_cache, "verification full-capacity K/V differs from serial oracle");
    future_zero(actual_cache, type, prefix_steps + expected_accepted);
    preserved_prefix(subject.initial, actual_cache, type, prefix_steps);
    if (!expected_accepted) {
        require(obs::calls == 0 && obs::uploads == 0 && obs::readbacks == 0, "first mismatch performed backend work");
    } else {
#if TEMPORAL_NATIVE
        require(observed.size() == 1 && observed.front().verification, "verify must execute one real fixed4 token graph");
        const auto & graph = observed.front();
        if (graph.scalar_rows) { ++scalar_verifications; } else { ++dense_verifications; }
        require(graph.before == subject.initial, "verification did not share the original static K/V");
        require(obs::readbacks == 1 && obs::readback_bytes == 4 * sizeof(int32_t), "verification did not read exactly four token IDs");
        require(obs::drains == 0, "verification synchronized before compute consumed queued inputs");
        for (size_t i = 0; i < 4; ++i) {
            require(graph.input_ids[i] == draft[std::min(i, draft.size() - 1)], "verification padding is not duplicate-final-token");
            require(graph.positions[i] == prefix_steps + static_cast<int64_t>(i) && graph.slots[i] == graph.positions[i],
                    "verification positions/slots differ from serial continuation");
            require(graph.mask.size() == 4 * capacity, "verification mask shape differs");
            for (int64_t k = 0; k < capacity; ++k) {
                require(graph.mask[i * capacity + k] == ggml_fp32_to_fp16(k <= prefix_steps + static_cast<int64_t>(i)
                    ? 0.0F : -std::numeric_limits<float>::infinity()), "verification mask is not capacity-matched causal");
            }
        }
        for (size_t i = 0; i < static_cast<size_t>(expected_accepted); ++i) {
            require(obs::exact(obs::row(graph.logits, i, vocabulary), serial_graphs[i].logits), "verification row logits differ exactly");
            require(obs::exact(obs::row(graph.hidden, i, width), serial_graphs[i].hidden), "verification row hidden differs exactly");
            require(graph.predictions[i] == serial_graphs[i].predictions.front(), "verification FIRST-max row differs");
        }
#endif
    }
    const bool work_ok = expected_accepted < 2 || (observed.size() == 1 && observed.front().verification);
    if (!work_ok) { ++work_red; }
    record(trace, seed); record(trace, static_cast<int32_t>(type)); record_vector(trace, draft);
    record(trace, actual.accepted_tokens); record(trace, actual.next_token); record_cache(trace, actual_cache);

    obs::reset();
    const auto oracle_next = oracle.decoder->decode_token(expected.next_token);
    const auto oracle_probe = obs::graphs.back();
    obs::reset();
    const auto subject_next = subject.decoder->decode_token(actual.next_token);
    require(token(oracle_next) == token(subject_next), "post-verify next token differs");
    require(obs::exact(oracle_probe.logits, obs::graphs.back().logits) &&
            obs::exact(oracle_probe.hidden, obs::graphs.back().hidden), "post-verify logits/hidden differ exactly");
    require(obs::snapshot(oracle.caches) == obs::snapshot(subject.caches), "post-verify next K/V differs");
    record(trace, token(subject_next)); record_vector(trace, obs::graphs.back().logits);
    record_vector(trace, obs::graphs.back().hidden); record_cache(trace, obs::snapshot(subject.caches));
    ++cases;
    std::cout << "CASE seed=" << seed << " type=" << ggml_type_name(type) << " length=" << draft.size()
              << " accepted=" << expected_accepted << " oracle_graphs=" << serial_graphs.size()
              << " actual_graphs=" << observed.size() << " work=" << (work_ok ? "GREEN" : "RED") << '\n';
    (void)enforce;
}
template<class Function> void rejected(Started & state, Function run) {
    const auto before = obs::snapshot(state.caches);
    const auto end = state.decoder->decode_current_end(), valid = state.decoder->decode_valid_steps();
    obs::reset(); bool failed = false;
    try { run(); } catch (const std::exception &) { failed = true; }
    require(failed, "invalid verification request accepted");
    require(obs::pending.empty() && obs::uploads == 0 && obs::calls == 0 && obs::readbacks == 0,
            "invalid verification performed input uploads/compute/readback");
    require(obs::snapshot(state.caches) == before && state.decoder->decode_current_end() == end &&
            state.decoder->decode_valid_steps() == valid, "invalid request changed cache state");
    ++guards;
}
void native_guards(Fixture & fixture) {
#if TEMPORAL_NATIVE
    auto state = start(fixture, GGML_TYPE_F32, 42);
    const auto first = state.first;
    for (const auto & draft : std::vector<std::vector<int32_t>>{{}, {first, first, first, first, first}, {-1}, {97}, {first, -1}, {first, 97}}) {
        rejected(state, [&] { state.decoder->verify_tokens(draft, first); });
    }
    for (const auto invalid : {-1, 97}) { rejected(state, [&] { state.decoder->verify_tokens({first}, invalid); }); }

    rejected(state, [&] { state.decoder->verify_tokens({(first + 1) % 97, -1}, first); });
    auto small = start(fixture, GGML_TYPE_F32, 42, 67);
    rejected(small, [&] { small.decoder->verify_tokens({small.first}, small.first); });
    auto edge = start(fixture, GGML_TYPE_F32, 42, 68);
    obs::reset();
    const auto edge_result = edge.decoder->verify_tokens({edge.first}, edge.first);
    require(edge_result.accepted_tokens == 1 && obs::graphs.size() == 1 && obs::graphs.front().verification,
            "exact four-slot capacity boundary rejected");
    runtime::TransformerKVState imported;
    imported.current_end = prefix_steps; imported.layers.resize(2);
    for (auto & layer : imported.layers) {
        layer.valid_steps = prefix_steps;
        layer.key.assign(prefix_steps * width, 0.0F); layer.value = layer.key;
    }
    for (int guard = 0; guard < 8; ++guard) {
        auto config = config_for(GGML_TYPE_F32);
        if (guard == 0) { config.greedy_token_readback = false; }
        if (guard == 1) { config.return_hidden = true; }
        if (guard == 2) { config.sliding_window = 8; }
        if (guard == 3) { config.logits_readback_token_ids = {0, 1}; }
        if (guard == 4) { config.decoder.logits_mode = modules::QwenCausalDecoderLogitsMode::AllSteps; }
        if (guard == 5) { config.decoder.stack.runtime.static_cache.update_mode = modules::QwenDecoderStaticCacheUpdateMode::ScratchTail; }
        if (guard == 6) { config.output_mode = modules::QwenCausalDecodeOutputMode::Hidden; }
        Started invalid;
        invalid.decoder = std::make_unique<modules::QwenCausalDecodeRuntime>(fixture.execution, config, fixture.weights);
        if (guard == 7) { invalid.decoder->start_decode_embeddings(imported, capacity); }
        else { invalid.decoder->start_decode_tokens(imported, capacity); }
        rejected(invalid, [&] { invalid.decoder->verify_tokens({0}, 0); });
    }
    Started fresh;
    fresh.decoder = std::make_unique<modules::QwenCausalDecodeRuntime>(fixture.execution, config_for(GGML_TYPE_F32), fixture.weights);
    rejected(fresh, [&] { fresh.decoder->verify_tokens({0}, 0); });
#endif
}
void native_recovery(Fixture & fixture) {
#if TEMPORAL_NATIVE
    const auto draft = continuation(fixture, GGML_TYPE_F32, 42);
    for (int fault = 0; fault < 4; ++fault) {
        auto state = start(fixture, GGML_TYPE_F32, 42);
        obs::reset();
        if (fault < 2) { obs::fail_upload = fault + 1; }
        if (fault == 2) { obs::fail_compute_before = true; }
        if (fault == 3) { obs::fail_compute_after = true; }
        bool failed = false;
        try { state.decoder->verify_tokens(draft, state.first); } catch (const std::exception &) { failed = true; }
        require(failed, "injected backend failure not propagated");
        require(obs::pending.empty(), "failed verification left original source pointers queued");
        metadata(state, prefix_steps);
        require(obs::snapshot(state.caches) == state.initial, "failed verification did not restore exact base prefix/future zero");
        obs::reset();
        const auto recovered = state.decoder->verify_tokens(draft, state.first);
        require(recovered.accepted_tokens == 4 && obs::graphs.size() == 1 && obs::graphs.front().verification,
                "verification did not recover after failure");
        ++recoveries;
    }
    auto state = start(fixture, GGML_TYPE_F32, 42);
    int32_t next = state.first;
    std::set<ggml_cgraph *> identities;
    for (int i = 0; i < 6; ++i) {
        obs::reset();
        const auto result = state.decoder->verify_tokens({next, (next + 1) % 97}, next);
        require(result.accepted_tokens >= 1 && result.accepted_tokens <= 2 && obs::graphs.size() == 1,
                "repeated verify state/graph invalid");
        identities.insert(obs::graphs.front().graph); next = result.next_token;
    }
    require(identities.size() == 1, "verification rebuilds graphs per call");
    state.decoder->release_runtime_graphs();
    require(obs::live_verification.empty(), "release_runtime_graphs leaked verification resources");
    obs::reset();
    const auto restored = state.decoder->prefill_embeddings_into_cache(pattern(prefix_steps * width, 42, 0.3F), 64, 256, 64);
    const auto again = state.decoder->verify_tokens({token(restored)}, token(restored));
    require(again.accepted_tokens == 1, "verification failed after release/cache growth");
#endif
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 4, "usage: verify_test.exe equivalence|work trace.bin f16|bf16");
        const bool enforce = std::string(argv[1]) == "work";
        require(enforce || std::string(argv[1]) == "equivalence", "invalid mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open trace");
        std::vector<ggml_type> types{GGML_TYPE_F32, GGML_TYPE_F16};
        if (std::string(argv[3]) == "bf16") { types.push_back(GGML_TYPE_BF16); }
        for (const uint32_t seed : {1U, 42U, 20261019U}) {
            Fixture fixture(seed);
            for (const auto type : types) {
                const auto full = continuation(fixture, type, seed);
                for (size_t length = 1; length <= 4; ++length) {
                    const std::vector<int32_t> all(full.begin(), full.begin() + length);
                    numerical_case(fixture, type, seed, all, length, trace, enforce);
                    for (size_t mismatch = 0; mismatch < length; ++mismatch) {
                        auto draft = all; draft[mismatch] = (draft[mismatch] + 1) % vocabulary;
                        numerical_case(fixture, type, seed, draft, mismatch, trace, enforce);
                    }
                }
            }
            native_guards(fixture); native_recovery(fixture);
        }
        { Fixture ties(42, true); const auto full = continuation(ties, GGML_TYPE_F32, 42);
          require(full == std::vector<int32_t>(4, 0), "tie fixture must select token zero FIRST");
          numerical_case(ties, GGML_TYPE_F32, 42, full, 4, trace, enforce); }
        require(trace.good(), "trace write failed");
        require(obs::live_verification.empty(), "suite leaked live verification graphs");
        std::cout << "NUMERICAL_GREEN cases=" << cases << " guards=" << guards << " recoveries=" << recoveries
                  << " dense_verifications=" << dense_verifications << " scalar_verifications=" << scalar_verifications
                  << " work_red=" << work_red << " assertions=" << obs::assertions
                  << " queued_consumed=" << obs::queued_consumed << " synthetic_only=1\n";
        if (enforce && work_red) { std::cerr << "WORK_RED temporal verification used serial decode graphs in " << work_red << " cases\n"; return 2; }
        return 0;
    } catch (const std::exception & error) { std::cerr << "FAIL " << error.what() << '\n'; return 1; }
}
