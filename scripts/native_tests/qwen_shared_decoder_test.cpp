#include "engine/models/qwen3_asr/thinker.h"
#include "engine/framework/core/backend.h"
#include "engine/framework/runtime/greedy_qwen_decoder.h"

#include <algorithm>
#include <cstring>
#include <fstream>
#include <iostream>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
namespace assets = engine::assets;
namespace core = engine::core;
namespace qwen = engine::models::qwen3_asr;
namespace runtime = engine::runtime;
constexpr int64_t width = 64;
constexpr int64_t vocabulary = 97;
constexpr int64_t layers = 2;
constexpr size_t arena = 8 * 1024 * 1024;
size_t assertions = 0, generated_cases = 0, classified_cases = 0, guard_cases = 0, work_failures = 0;
std::set<int32_t> generated_tokens;

struct Work {
    size_t graphs = 0, kv_outputs = 0, kv_reads = 0, kv_bytes = 0;
    std::set<const ggml_tensor *> output_tensors;
} work;

void require(bool condition, const std::string & message) {
    ++assertions;
    if (!condition) { throw std::runtime_error(message); }
}

std::vector<float> pattern(size_t count, uint32_t seed, float scale = 0.08F) {
    std::vector<float> values(count);
    for (auto & value : values) {
        seed = 1664525U * seed + 1013904223U;
        value = scale * static_cast<float>(static_cast<int32_t>(seed >> 16) - 32768) / 32768.0F;
    }
    return values;
}



class TinySource final : public assets::TensorSource {
public:
    struct Entry { assets::TensorMetadata metadata; std::vector<float> values; };
    std::map<std::string, Entry> entries;
    std::filesystem::path path{"synthetic-qwen-shared-decoder-test-only"};

    void add(std::string name, std::vector<int64_t> shape, uint32_t seed, bool norm = false) {
        size_t count = 1;
        for (auto dimension : shape) { count *= static_cast<size_t>(dimension); }
        auto values = pattern(count, seed);
        if (norm) { std::fill(values.begin(), values.end(), 1.0F); }
        entries.emplace(name, Entry{{name, "F32", std::move(shape)}, std::move(values)});
    }
    const std::filesystem::path & source_path() const noexcept override { return path; }
    bool has_tensor(std::string_view name) const noexcept override { return entries.count(std::string(name)) != 0; }
    assets::TensorMetadata require_metadata(std::string_view name) const override {
        return entries.at(std::string(name)).metadata;
    }
    std::vector<assets::TensorMetadata> tensors() const override {
        std::vector<assets::TensorMetadata> out;
        for (const auto & entry : entries) { out.push_back(entry.second.metadata); }
        return out;
    }
    assets::RawTensorData require_tensor_data(std::string_view name) const override {
        const auto & entry = entries.at(std::string(name));
        assets::RawTensorData out{entry.metadata, std::vector<std::byte>(entry.values.size() * sizeof(float))};
        std::memcpy(out.bytes.data(), entry.values.data(), out.bytes.size());
        return out;
    }
    std::vector<float> require_f32(std::string_view name,
        const std::optional<std::vector<int64_t>> & expected = std::nullopt) const override {
        const auto & entry = entries.at(std::string(name));
        require(!expected || *expected == entry.metadata.shape, "synthetic weight shape mismatch");
        return entry.values;
    }
    std::optional<std::vector<float>> optional_f32(std::string_view name,
        const std::optional<std::vector<int64_t>> & expected = std::nullopt) const override {
        return has_tensor(name) ? std::optional<std::vector<float>>(require_f32(name, expected)) : std::nullopt;
    }
    int64_t require_i64_scalar(std::string_view) const override {
        throw std::runtime_error("synthetic decoder has no integer scalar weights");
    }
};

std::shared_ptr<qwen::Qwen3ASRAssets> make_assets(bool hf, bool tie) {
    auto result = std::make_shared<qwen::Qwen3ASRAssets>();
    auto source = std::make_shared<TinySource>();
    auto & config = result->config;
    config.hf_transformers_layout = hf;
    config.tie_word_embeddings = tie;
    auto & text = config.text_decoder;
    text.vocab_size = text.output_size = vocabulary;
    text.hidden_size = text.head_dim = width;
    text.intermediate_size = 128;
    text.num_hidden_layers = layers;
    text.num_attention_heads = 2;
    text.num_key_value_heads = 1;
    text.max_position_embeddings = 256;
    const std::string prefix = hf ? "model.language_model" : "thinker.model";
    source->add(prefix + ".embed_tokens.weight", {vocabulary, width}, 101);
    for (int64_t layer = 0; layer < layers; ++layer) {
        const auto base = prefix + ".layers." + std::to_string(layer);
        const auto seed = static_cast<uint32_t>(200 + layer * 20);
        source->add(base + ".input_layernorm.weight", {width}, seed, true);
        source->add(base + ".self_attn.q_proj.weight", {128, width}, seed + 1);
        source->add(base + ".self_attn.k_proj.weight", {width, width}, seed + 2);
        source->add(base + ".self_attn.v_proj.weight", {width, width}, seed + 3);
        source->add(base + ".self_attn.o_proj.weight", {width, 128}, seed + 4);
        source->add(base + ".self_attn.q_norm.weight", {width}, seed + 5, true);
        source->add(base + ".self_attn.k_norm.weight", {width}, seed + 6, true);
        source->add(base + ".post_attention_layernorm.weight", {width}, seed + 7, true);
        source->add(base + ".mlp.gate_proj.weight", {128, width}, seed + 8);
        source->add(base + ".mlp.up_proj.weight", {128, width}, seed + 9);
        source->add(base + ".mlp.down_proj.weight", {width, 128}, seed + 10);
    }
    source->add(prefix + ".norm.weight", {width}, 401, true);


    if (!(hf && tie)) {
        source->add(hf ? "lm_head.weight" : "thinker.lm_head.weight", {vocabulary, width}, 501);
    }
    result->model_weights = source;
    return result;
}

runtime::GreedyQwenDecoderSpec decoder_spec(const qwen::Qwen3ASRAssets & assets) {
    runtime::GreedyQwenDecoderSpec spec;
    auto & stack = spec.decoder.stack;
    const auto & text = assets.config.text_decoder;
    stack.hidden_size = width;
    stack.num_attention_heads = 2;
    stack.num_key_value_heads = 1;
    stack.head_dim = width;
    stack.intermediate_size = 128;
    stack.layers = layers;
    stack.rms_norm_eps = text.rms_norm_eps;
    stack.rope_theta = text.rope_theta;
    stack.use_qk_norm = true;
    stack.runtime.static_cache.update_mode = engine::modules::QwenDecoderStaticCacheUpdateMode::DirectSetRows;
    spec.decoder.logits_size = vocabulary;
    spec.vocab_size = vocabulary;
    spec.max_position_embeddings = text.max_position_embeddings;
    spec.tie_word_embeddings = assets.config.hf_transformers_layout && assets.config.tie_word_embeddings;
    const std::string prefix = assets.config.hf_transformers_layout ? "model.language_model" : "thinker.model";
    spec.token_embedding_tensor = prefix + ".embed_tokens.weight";
    spec.final_norm_tensor = prefix + ".norm.weight";
    spec.layer_prefix = prefix + ".layers";
    spec.lm_head_tensor = assets.config.hf_transformers_layout ? "lm_head.weight" : "thinker.lm_head.weight";
    spec.eos_token_ids = text.eos_token_ids;
    return spec;
}

qwen::Qwen3ASRPrompt make_prompt(int64_t steps, bool inject) {
    qwen::Qwen3ASRPrompt prompt;
    for (int64_t i = 0; i < steps; ++i) { prompt.input_ids.push_back(static_cast<int32_t>((i * 17 + steps * 3 + 7) % vocabulary)); }
    if (inject && steps > 0) {
        prompt.audio_token_positions.push_back(0);
        if (steps > 1) { prompt.audio_token_positions.push_back(static_cast<int32_t>(steps - 1)); }
    }
    return prompt;
}

qwen::Qwen3ASRAudioEmbeddings make_audio(const qwen::Qwen3ASRPrompt & prompt) {
    qwen::Qwen3ASRAudioEmbeddings audio;
    audio.hidden_size = width;
    audio.tokens = static_cast<int64_t>(prompt.audio_token_positions.size());
    audio.values = pattern(static_cast<size_t>(audio.tokens * width), 701, 0.75F);
    return audio;
}

template <typename T> void record(std::ofstream & output, const T & value) {
    output.write(reinterpret_cast<const char *>(&value), sizeof(value));
}
void record_tokens(std::ofstream & output, const std::vector<int32_t> & tokens) {
    record(output, static_cast<uint64_t>(tokens.size()));
    for (const auto token : tokens) {
        require(token >= 0 && token < vocabulary, "generated/classified token outside vocabulary");
        record(output, token);
    }
}

void check_work(const std::string & label, bool enforce) {
    require(work.graphs > 0, label + ": no actual graph execution observed");
    const bool okay = work.kv_reads == 0;
    if (enforce && !okay) { ++work_failures; }
    std::cout << (okay ? "WORK_OK " : "WORK_RED ") << label << " graphs=" << work.graphs
              << " kv_output_nodes=" << work.kv_outputs << " host_kv_reads=" << work.kv_reads
              << " host_kv_bytes=" << work.kv_bytes << " expected=0\n";
}

template <typename Function> void expect_error(std::ofstream & trace, const std::string & label, Function run) {
    bool rejected = false;
    try { run(); } catch (const std::exception &) { rejected = true; }
    require(rejected, "invalid request accepted: " + label);
    record(trace, static_cast<int32_t>(rejected));
    ++guard_cases;
}

void guards(qwen::Qwen3ASRThinkerRuntime & thinker, std::ofstream & trace) {
    auto prompt = make_prompt(3, true);
    auto audio = make_audio(prompt);
    const qwen::Qwen3ASRGenerationOptions normal{4};
    expect_error(trace, "empty generate", [&] { thinker.generate({}, make_audio({}), normal); });
    expect_error(trace, "empty classification", [&] { thinker.classify_prompt({}, make_audio({})); });
    for (const int64_t budget : {0, -1, 254}) {
        expect_error(trace, "generation budget/length", [&] { thinker.generate(prompt, audio, {budget}); });
    }
    auto too_long = make_prompt(257, false);
    expect_error(trace, "classification length", [&] { thinker.classify_prompt(too_long, make_audio(too_long)); });
    auto bad_hidden = audio;
    ++bad_hidden.hidden_size;
    auto bad_values = audio;
    bad_values.values.pop_back();
    auto bad_tokens = audio;
    ++bad_tokens.tokens;
    for (const auto & bad : {bad_hidden, bad_values, bad_tokens}) {
        expect_error(trace, "generation audio shape", [&] { thinker.generate(prompt, bad, normal); });
        expect_error(trace, "classification audio shape", [&] { thinker.classify_prompt(prompt, bad); });
    }
    for (const int32_t position : {-1, 3}) {
        auto bad_prompt = prompt;
        bad_prompt.audio_token_positions[0] = position;
        expect_error(trace, "generation audio position", [&] { thinker.generate(bad_prompt, audio, normal); });
        expect_error(trace, "classification audio position", [&] { thinker.classify_prompt(bad_prompt, audio); });
    }
}

void controls(core::ExecutionContext & execution) {
    auto assets = make_assets(false, false);
    runtime::GreedyQwenDecoderRuntime decoder(assets->model_weights, decoder_spec(*assets), execution,
                                             arena, arena, arena, assets::TensorStorageType::F32);
    const auto prompt = make_prompt(65, true);
    const auto audio = make_audio(prompt);
    runtime::GreedyQwenDecoderRuntime::Prompt input{prompt.input_ids,
        {audio.values, audio.tokens, prompt.audio_token_positions}};
    work = {};
    const auto old = decoder.generate(input, 3, false);
    require(old.size() == 3, "generic nonreuse negative control did not decode");
    require(work.graphs > 0 && work.kv_reads == layers * 2 &&
            work.kv_bytes == 65 * width * sizeof(float) * layers * 2,
            "KV counter missed actual shared nonreuse host exports");
    std::cout << "CONTROL_NONREUSE host_kv_reads=" << work.kv_reads << " host_kv_bytes=" << work.kv_bytes << '\n';
    work = {};
    const auto resident = decoder.generate(input, 3, true);
    require(work.graphs > 0 && work.kv_reads == 0, "resident shared control unexpectedly exported KV");
    require(old == resident, "shared control greedy token traces differ");
    std::cout << "CONTROL_RESIDENT graphs=" << work.graphs << " host_kv_reads=" << work.kv_reads << '\n';
}

void suite(core::ExecutionContext & execution, std::ofstream & trace, bool enforce) {
    std::vector<std::vector<int32_t>> untied_trace;
    size_t tied_differences = 0, injection_differences = 0;
    for (int variant = 0; variant < 4; ++variant) {
        const bool hf = variant >= 2;
        const bool tie = variant == 1 || variant == 3;
        const auto assets = make_assets(hf, tie);
        qwen::Qwen3ASRThinkerRuntime thinker(assets, execution, arena, arena, arena, assets::TensorStorageType::F32);
        guards(thinker, trace);
        size_t index = 0;

        for (const int64_t steps : {1, 63, 64, 65, 129, 3}) {
            std::vector<int32_t> without_audio;
            for (const bool inject : {false, true}) {
                const auto prompt = make_prompt(steps, inject);
                const auto audio = make_audio(prompt);
                work = {};
                const auto generated = thinker.generate(prompt, audio, {4}).token_ids;
                require(generated.size() == 4, "non-EOS generation did not honor exact token budget");
                generated_tokens.insert(generated.begin(), generated.end());
                check_work("variant" + std::to_string(variant) + "-steps" + std::to_string(steps)
                           + "-audio" + std::to_string(inject), enforce);
                record(trace, variant);
                record(trace, steps);
                record(trace, static_cast<int32_t>(inject));
                record_tokens(trace, generated);
                ++generated_cases;
                if (variant == 0) { untied_trace.push_back(generated); }
                else if (variant != 3) { require(generated == untied_trace[index], "legacy tie flag/HF untied changed separate-head behavior"); }
                else if (generated != untied_trace[index]) { ++tied_differences; }
                if (!inject) { without_audio = generated; }
                else if (without_audio != generated) { ++injection_differences; }
                ++index;
                if (steps == 1 || steps == 65 || steps == 129) {
                    work = {};
                    const auto classified = thinker.classify_prompt(prompt, audio);
                    require(classified.size() == static_cast<size_t>(steps), "classification lost AllSteps output");
                    require(work.graphs > 0 && work.kv_reads == 0, "classification graph/output counter invalid");
                    record_tokens(trace, classified);
                    ++classified_cases;
                }
            }
        }
        const auto eos_prompt = make_prompt(3, true);
        const auto eos_audio = make_audio(eos_prompt);
        const auto first = thinker.generate(eos_prompt, eos_audio, {1}).token_ids;
        require(first.size() == 1, "EOS fixture first token missing");
        auto eos_assets = std::make_shared<qwen::Qwen3ASRAssets>(*assets);
        eos_assets->config.text_decoder.eos_token_ids = {first[0]};
        qwen::Qwen3ASRThinkerRuntime eos_thinker(eos_assets, execution, arena, arena, arena, assets::TensorStorageType::F32);
        record_tokens(trace, eos_thinker.generate(eos_prompt, eos_audio, {4}).token_ids);
        require(eos_thinker.generate(eos_prompt, eos_audio, {4}).token_ids.empty(), "EOS token was emitted or decoding continued");
        auto limit_assets = std::make_shared<qwen::Qwen3ASRAssets>(*assets);
        limit_assets->config.text_decoder.max_position_embeddings = 4;
        qwen::Qwen3ASRThinkerRuntime limit(limit_assets, execution, arena, arena, arena, assets::TensorStorageType::F32);
        const auto short_prompt = make_prompt(1, false);
        const auto at_limit = limit.generate(short_prompt, make_audio(short_prompt), {3}).token_ids;
        require(at_limit.size() == 3, "exact max_position_embeddings boundary rejected/truncated");
        record_tokens(trace, at_limit);
        expect_error(trace, "one past generation position limit", [&] { limit.generate(short_prompt, make_audio(short_prompt), {4}); });
    }
    require(tied_differences > 0, "synthetic tied/separate heads produced indistinguishable traces");
    require(injection_differences > 0, "synthetic audio injection did not affect any token trace");
    require(generated_tokens.size() > 4, "synthetic decoding has insufficient token diversity");
    std::cout << "EQUIVALENCE_OK synthetic_only generated_cases=" << generated_cases
              << " classification_cases=" << classified_cases << " guard_cases=" << guard_cases
              << " distinct_tokens=" << generated_tokens.size() << " tied_differences=" << tied_differences
              << " injection_differences=" << injection_differences << " assertions=" << assertions << '\n';
}
}

namespace engine::core {
ggml_status qwen_test_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {
    const auto status = compute_backend_graph(backend, graph, plan, label);
    if (status == GGML_STATUS_SUCCESS) {
        ++work.graphs;
        for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
            const auto * node = ggml_graph_node(graph, i);


            if (node->op == GGML_OP_CPY && (node->flags & GGML_TENSOR_FLAG_OUTPUT) && node->ne[0] == width) {
                work.output_tensors.insert(node);
                ++work.kv_outputs;
            }
        }
    }
    return status;
}
}

void qwen_test_backend_tensor_get(const ggml_tensor * tensor, void * data, size_t offset, size_t size) {
    ggml_backend_tensor_get(tensor, data, offset, size);
    if (work.output_tensors.count(tensor)) {
        ++work.kv_reads;
        work.kv_bytes += size;
    }
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: qwen_shared_decoder_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "invalid test mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot create token/classification trace");
        core::ExecutionContext execution{{core::BackendType::Cpu, 0, 1}};
        controls(execution);
        suite(execution, trace, mode == "work");
        require(trace.good(), "cannot write complete token/classification trace");
        require(work_failures == 0, "generation still exports K/V to host in " + std::to_string(work_failures) + " cases");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
