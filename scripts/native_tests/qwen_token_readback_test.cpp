#include "engine/models/qwen3_asr/thinker.h"
#include "engine/framework/core/backend.h"
#include "engine/framework/runtime/greedy_qwen_decoder.h"

#include <algorithm>
#include <cmath>
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
size_t assertions = 0;

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

struct Readback {
    size_t graphs = 0, full_reads = 0, full_bytes = 0, token_reads = 0, token_bytes = 0, argmax_nodes = 0;
    std::vector<std::vector<float>> full_logits;
} reads;
size_t cases = 0, work_failures = 0;
std::set<int32_t> tokens_seen;

runtime::GreedyQwenDecoderRuntime::Prompt input_for(int64_t steps, bool inject) {
    const auto prompt = make_prompt(steps, inject);
    const auto audio = make_audio(prompt);
    return {prompt.input_ids, {audio.values, audio.tokens, prompt.audio_token_positions}};
}

void record_tokens(std::ofstream & trace, const std::vector<int32_t> & tokens) {
    const uint64_t count = tokens.size();
    trace.write(reinterpret_cast<const char *>(&count), sizeof(count));
    for (const auto token : tokens) {
        require(token >= 0 && token < vocabulary, "token outside vocabulary");
        trace.write(reinterpret_cast<const char *>(&token), sizeof(token));
        tokens_seen.insert(token);
    }
}

void check_readback(const std::string & label, size_t decisions, bool enforce) {
    require(reads.graphs > 0, label + ": graph interposition missed execution");
    const bool okay = reads.full_reads == 0 && reads.token_reads == decisions &&
        reads.token_bytes == decisions * sizeof(int32_t) && reads.argmax_nodes >= decisions;
    if (enforce && !okay) { ++work_failures; }
    std::cout << (okay ? "WORK_OK " : "WORK_RED ") << label << " decisions=" << decisions
              << " full_vocab_reads=" << reads.full_reads << " full_vocab_bytes=" << reads.full_bytes
              << " token_reads=" << reads.token_reads << " token_bytes=" << reads.token_bytes
              << " backend_argmax_nodes=" << reads.argmax_nodes << '\n';
    ++cases;
}

void suite(core::ExecutionContext & execution, std::ofstream & trace, bool enforce) {
    std::vector<std::vector<int32_t>> separate_trace;
    size_t tied_differences = 0;
    for (int variant = 0; variant < 4; ++variant) {
        const auto assets = make_assets(variant >= 2, variant == 1 || variant == 3);
        runtime::GreedyQwenDecoderRuntime decoder(assets->model_weights, decoder_spec(*assets), execution,
                                                 arena, arena, arena, assets::TensorStorageType::F32);
        size_t index = 0;
        for (const int64_t steps : {1, 63, 64, 65, 129, 3}) {
            for (const bool inject : {false, true}) {
                const auto input = input_for(steps, inject);
                reads = {};
                const auto output = decoder.generate(input, 4, true);
                require(output.size() == 4, "generation budget changed without EOS");
                check_readback("variant" + std::to_string(variant) + "-steps" + std::to_string(steps)
                               + "-audio" + std::to_string(inject), 4, enforce);
                record_tokens(trace, output);
                if (variant == 0) { separate_trace.push_back(output); }
                else if (variant != 3) { require(output == separate_trace[index], "legacy tie/HF separate-head behavior changed"); }
                else if (output != separate_trace[index]) { ++tied_differences; }
                ++index;
            }
        }


        reads = {};
        const auto control = decoder.generate(input_for(65, true), 3, false);
        require(control.size() == 3 && reads.full_reads > 0 && reads.full_bytes >= vocabulary * sizeof(float),
                "full-logits negative control did not expose actual vocabulary readback");
        require(!reads.full_logits.empty(), "no real logits captured by negative control");
        for (const auto & logits : reads.full_logits) {
            require(std::all_of(logits.begin(), logits.end(), [](float value) { return std::isfinite(value); }),
                    "nonfinite logits in synthetic control");
        }
        std::cout << "CONTROL_FULL_LOGITS variant=" << variant << " reads=" << reads.full_reads
                  << " bytes=" << reads.full_bytes << '\n';
    }
    require(tied_differences > 0, "tied and separate head fixture traces indistinguishable");
}

std::shared_ptr<qwen::Qwen3ASRAssets> controlled_assets(int mode) {
    auto assets = make_assets(false, false);
    auto source = std::const_pointer_cast<TinySource>(std::static_pointer_cast<const TinySource>(assets->model_weights));
    for (auto & entry : source->entries) {
        auto & values = entry.second.values;
        const auto & name = entry.first;
        if (name.find("norm.weight") != std::string::npos) {
            std::fill(values.begin(), values.end(), 1.0F);
        } else {
            std::fill(values.begin(), values.end(), 0.0F);
        }
        if (name.find("embed_tokens.weight") != std::string::npos) {
            for (int64_t token = 0; token < vocabulary; ++token) { values[token * width] = 1.0F; }
        }
        if (name == "thinker.lm_head.weight" && mode != 0) {
            values[3 * width] = 1.0F;
            values[11 * width] = mode == 1 ? 1.0F : std::nextafter(1.0F, 2.0F);
        }
    }
    return assets;
}

void finite_ties(core::ExecutionContext & execution, std::ofstream & trace, bool enforce) {
    for (int mode = 0; mode < 3; ++mode) {
        const auto assets = controlled_assets(mode);
        auto spec = decoder_spec(*assets);
        const int32_t expected = mode == 0 ? 0 : mode == 1 ? 3 : 11;
        runtime::GreedyQwenDecoderRuntime decoder(assets->model_weights, spec, execution,
                                                 arena, arena, arena, assets::TensorStorageType::F32);
        const auto input = input_for(1, false);
        reads = {};
        require(decoder.generate(input, 1, false) == std::vector<int32_t>{expected}, "controlled host first-max result wrong");
        require(!reads.full_logits.empty(), "controlled full-logits probe missing");
        const auto & logits = reads.full_logits.front();
        require(std::all_of(logits.begin(), logits.end(), [](float value) { return std::isfinite(value); }),
                "controlled logits must all be finite");
        if (mode == 0) {
            require(std::all_of(logits.begin(), logits.end(), [](float value) { return value == 0; }), "zero-tie fixture not zero");
        } else if (mode == 1) {
            require(logits[3] == logits[11] && logits[3] > logits[0], "nonzero first-max tie fixture invalid");
        } else {
            require(logits[11] > logits[3] && logits[11] - logits[3] < 0.0001F, "adjacent-F32 near-tie fixture invalid");
        }



        qwen::Qwen3ASRThinkerRuntime classifier(assets, execution, arena, arena, arena,
                                               assets::TensorStorageType::F32);
        const auto classification_prompt = make_prompt(3, false);
        const auto classification = classifier.classify_prompt(classification_prompt, make_audio(classification_prompt));
        const int32_t default_expected = mode == 0 ? static_cast<int32_t>(vocabulary - 1) : 11;
        require(classification == std::vector<int32_t>(3, default_expected),
                "default GreedyDecodeModule/classify_prompt tie behavior changed");
        record_tokens(trace, classification);
        std::cout << "DEFAULT_CLASSIFY mode=" << mode << " token=" << default_expected << " steps=3\n";
        for (const int budget : {1, 4}) {
            reads = {};
            const auto output = decoder.generate(input, budget, true);
            require(output == std::vector<int32_t>(budget, expected), "backend selection changed finite first-max/near-tie tokens");
            check_readback("finite-mode" + std::to_string(mode) + "-budget" + std::to_string(budget), budget, enforce);
            record_tokens(trace, output);
        }
        spec.eos_token_ids = {expected};
        runtime::GreedyQwenDecoderRuntime eos(assets->model_weights, spec, execution,
                                             arena, arena, arena, assets::TensorStorageType::F32);
        reads = {};
        const auto stopped = eos.generate(input, 4, true);
        require(stopped.empty(), "EOS emitted or generation continued past EOS");
        check_readback("finite-mode" + std::to_string(mode) + "-EOS", 1, enforce);
        record_tokens(trace, stopped);
    }
}

void observe_read(const ggml_tensor * tensor, const void * data, size_t offset, size_t size) {

    if (tensor->type == GGML_TYPE_F32 && ggml_nelements(tensor) == vocabulary) {
        ++reads.full_reads;
        reads.full_bytes += size;
        require(offset == 0 && size == vocabulary * sizeof(float), "unexpected partial vocabulary readback");
        const auto * values = static_cast<const float *>(data);
        reads.full_logits.emplace_back(values, values + vocabulary);
    }
    if (tensor->type == GGML_TYPE_I32 && ggml_nelements(tensor) == 1) {
        ++reads.token_reads;
        reads.token_bytes += size;
    }
}
}

namespace engine::core {
ggml_status qwen_token_compute_graph(ggml_backend_t backend, ggml_cgraph * graph,
                                   ggml_backend_graph_plan_t plan, const char * label) {
    const auto status = compute_backend_graph(backend, graph, plan, label);
    if (status == GGML_STATUS_SUCCESS) {
        ++reads.graphs;
        for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
            const auto * node = ggml_graph_node(graph, i);
            if (node->op == GGML_OP_ARGMAX && node->src[0] && node->src[0]->ne[0] == vocabulary) {
                ++reads.argmax_nodes;
            }
        }
    }
    return status;
}

std::vector<float> qwen_token_read_f32(const ggml_tensor * tensor) {
    auto values = read_tensor_f32(tensor);
    observe_read(tensor, values.data(), 0, values.size() * sizeof(float));
    return values;
}

void qwen_token_read_f32_into(const ggml_tensor * tensor, std::vector<float> & values) {
    read_tensor_f32_into(tensor, values);
    observe_read(tensor, values.data(), 0, values.size() * sizeof(float));
}
}

void qwen_token_tensor_get(const ggml_tensor * tensor, void * data, size_t offset, size_t size) {
    ggml_backend_tensor_get(tensor, data, offset, size);
    observe_read(tensor, data, offset, size);
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: qwen_token_readback_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "unknown mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot create token trace");
        core::ExecutionContext execution{{core::BackendType::Cpu, 0, 1}};
        suite(execution, trace, mode == "work");
        finite_ties(execution, trace, mode == "work");
        require(tokens_seen.size() > 4 && trace.good(), "token trace lacks diversity or could not be written");
        std::cout << "EQUIVALENCE_OK synthetic_only cases=" << cases << " distinct_tokens=" << tokens_seen.size()
                  << " finite_tie_modes=3 assertions=" << assertions << '\n';
        require(work_failures == 0, "full vocabulary readback or missing backend token selection in " + std::to_string(work_failures) + " cases");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
