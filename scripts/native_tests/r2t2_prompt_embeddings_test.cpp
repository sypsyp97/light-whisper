
#define main established_qwen_test_main
#include "qwen_shared_decoder_test.cpp"
#undef main

namespace {
using Prompt = runtime::GreedyQwenDecoderRuntime::Prompt;
struct Lookup {
    const ggml_tensor * output = nullptr;
    std::vector<int32_t> ids;
    size_t read_rows = 0;
    bool success = false;
};
struct Observation {
    size_t graphs = 0, decoder_lookups = 0, row_bytes = 0, fail_on_lookup = 0;
    std::vector<Lookup> lookups;
    const ggml_tensor * pending_output = nullptr;
} observed;
size_t cases = 0, failures = 0, failed_graph_cases = 0, generic_guards = 0;
size_t total_lookup_graphs = 0, total_lookup_rows = 0, total_lookup_bytes = 0;
std::vector<float> embedding_weights = pattern(vocabulary * width, 101);




const ggml_tensor * table_lookup(const ggml_tensor * node) {
    return node->op == GGML_OP_GET_ROWS && node->src[0] && node->src[1] &&
        node->src[0]->ne[0] == width && node->src[0]->ne[1] == vocabulary &&
        node->src[1]->type == GGML_TYPE_I32 ? node : nullptr;
}
const ggml_tensor * embedding_only(ggml_cgraph * graph) {
    const ggml_tensor * lookup = nullptr;
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        const auto * node = ggml_graph_node(graph, i);
        if (table_lookup(node)) {
            if (lookup) { return nullptr; }
            lookup = node;
        } else if (node->op != GGML_OP_RESHAPE && node->op != GGML_OP_VIEW && node->op != GGML_OP_CONT) {
            return nullptr;
        }
    }
    return lookup;
}

std::vector<int32_t> ids(size_t count, int seed) {
    std::vector<int32_t> result(count);
    for (size_t i = 0; i < count; ++i) {
        result[i] = static_cast<int32_t>(1 + (i * 17 + seed) % (vocabulary - 1));
    }
    return result;
}
Prompt placed(const std::vector<int32_t> & text, size_t steps,
              const std::vector<int32_t> & text_positions, uint32_t seed) {
    require(text.size() == text_positions.size(), "invalid test text placement");
    Prompt prompt;
    prompt.input_ids = ids(steps, 31);
    std::vector<bool> text_mask(steps, false);
    for (size_t i = 0; i < text.size(); ++i) {
        require(text_positions[i] >= 0 && static_cast<size_t>(text_positions[i]) < steps,
                "invalid test text position");
        prompt.input_ids[text_positions[i]] = text[i];
        text_mask[text_positions[i]] = true;
    }
    for (size_t i = 0; i < steps; ++i) {
        if (!text_mask[i]) { prompt.injection.positions.push_back(static_cast<int32_t>(i)); }
    }
    prompt.injection.tokens = static_cast<int64_t>(prompt.injection.positions.size());
    prompt.injection.values = pattern(prompt.injection.tokens * width, seed, 0.75F);
    return prompt;
}
Prompt plain(std::vector<int32_t> text) {
    Prompt prompt;
    prompt.input_ids = std::move(text);
    return prompt;
}
Prompt around(const std::vector<int32_t> & text, uint32_t seed) {
    std::vector<int32_t> positions;
    for (size_t i = 0; i < text.size(); ++i) { positions.push_back(static_cast<int32_t>(i + 1)); }
    return placed(text, text.size() + 2, positions, seed);
}
std::vector<int32_t> text_ids(const Prompt & prompt) {
    std::vector<bool> injected(prompt.input_ids.size(), false);
    for (const auto position : prompt.injection.positions) { injected[position] = true; }
    std::vector<int32_t> result;
    for (size_t i = 0; i < prompt.input_ids.size(); ++i) {
        if (!injected[i]) { result.push_back(prompt.input_ids[i]); }
    }
    return result;
}
void record_result(std::ofstream & trace, const std::vector<int32_t> & result) {
    require(result.size() == 3, "synthetic generation did not honor exact non-EOS budget");
    generated_tokens.insert(result.begin(), result.end());
    record_tokens(trace, result);
}

struct Sequence {
    runtime::GreedyQwenDecoderRuntime decoder;
    std::vector<int32_t> latest;
    bool valid = false;
    bool enforce;
    std::ofstream & trace;
    explicit Sequence(core::ExecutionContext & execution, const qwen::Qwen3ASRAssets & assets,
                      bool enforce, std::ofstream & trace)
        : decoder(assets.model_weights, decoder_spec(assets), execution, arena, arena, arena,
                  assets::TensorStorageType::F32), enforce(enforce), trace(trace) {}

    std::vector<int32_t> run(const std::string & label, const Prompt & prompt) {
        const auto text = text_ids(prompt);
        const auto expected = valid && text == latest ? std::vector<int32_t>{} : text;
        observed = {};
        const auto result = decoder.generate(prompt, 3, true);
        require(observed.graphs > observed.lookups.size(), label + ": no real decoder graphs observed");
        require(observed.decoder_lookups >= 2, label + ": decoder token lookup negative control missing");
        std::vector<int32_t> actual;
        for (const auto & lookup : observed.lookups) {
            require(lookup.success && lookup.read_rows > 0, label + ": lookup output was not read");
            actual.insert(actual.end(), lookup.ids.begin(), lookup.ids.begin() + lookup.read_rows);
        }
        const size_t expected_graphs = (expected.size() + 63) / 64;
        const bool okay = actual == expected && observed.lookups.size() == expected_graphs &&
            observed.row_bytes == expected.size() * width * sizeof(float);
        if (enforce && !okay) { ++failures; }
        std::cout << (okay ? "PROMPT_WORK_OK " : "PROMPT_WORK_RED ") << label
                  << " lookup_graphs=" << observed.lookups.size() << " lookup_rows=" << actual.size()
                  << " row_bytes=" << observed.row_bytes << " expected_graphs=" << expected_graphs
                  << " expected_rows=" << expected.size() << " ids_equal=" << (actual == expected)
                  << " decoder_token_lookups=" << observed.decoder_lookups << '\n';
        total_lookup_graphs += observed.lookups.size();
        total_lookup_rows += actual.size();
        total_lookup_bytes += observed.row_bytes;
        record_result(trace, result);
        latest = text;
        valid = true;
        ++cases;
        return result;
    }

    void failure(const Prompt & prompt) {
        observed = {};
        observed.fail_on_lookup = 2;
        bool rejected = false;
        try { decoder.generate(prompt, 3, true); }
        catch (const std::exception &) { rejected = true; }
        require(rejected && observed.lookups.size() == 2 && observed.lookups[0].success &&
                !observed.lookups[1].success && observed.lookups[0].read_rows == 64 &&
                observed.lookups[1].read_rows == 0,
                "injected second embedding graph failure did not reach the real lookup boundary");
        require(observed.row_bytes == 64 * width * sizeof(float), "failed lookup unexpectedly read output");
        record(trace, static_cast<int32_t>(rejected));
        ++failed_graph_cases;
    }

    template <typename Function> void guard(Function run) {
        observed = {};
        bool rejected = false;
        try { run(); } catch (const std::exception &) { rejected = true; }
        require(rejected && observed.graphs == 0 && observed.lookups.empty(),
                "public generate guard accepted invalid input or performed graph work");
        record(trace, static_cast<int32_t>(rejected));
        ++generic_guards;
    }
};

void prompt_suite(core::ExecutionContext & execution, std::ofstream & trace, bool enforce) {
    for (int variant = 0; variant < 4; ++variant) {
        const auto assets = make_assets(variant >= 2, variant == 1 || variant == 3);
        Sequence seq(execution, *assets, enforce, trace);
        const auto name = "variant" + std::to_string(variant) + "/";
        const auto short_text = ids(3, 7);
        auto dense = placed(short_text, 129, {0, 64, 128}, 701);
        seq.run(name + "dense-audio", dense);
        dense.injection.values = pattern(dense.injection.values.size(), 702, 0.75F);
        seq.run(name + "same-text-new-audio", dense);
        for (const auto position : dense.injection.positions) { dense.input_ids[position] = 93; }
        seq.run(name + "same-text-new-placeholder-ids", dense);
        seq.run(name + "shifted-positions", placed(short_text, 129, {1, 63, 127}, 703));
        seq.run(name + "same-text-growing-audio", placed(short_text, 133, {2, 65, 132}, 704));
        auto changed = short_text;
        ++changed[1];
        seq.run(name + "changed-text-id", around(changed, 705));
        const auto medium_text = ids(65, 11);
        seq.run(name + "growing-text-65", around(medium_text, 706));
        seq.run(name + "same-growing-text-new-audio", around(medium_text, 707));
        seq.run(name + "growing-text-129", around(ids(129, 13), 708));
        seq.run(name + "shrinking-text", around(short_text, 709));
        seq.run(name + "no-injection-65", plain(medium_text));
        seq.run(name + "same-no-injection", plain(medium_text));
        seq.run(name + "all-injected-65", placed({}, 65, {}, 710));
        seq.run(name + "all-injected-shrink-new-audio", placed({}, 3, {}, 711));
        seq.run(name + "return-to-older-text", around(short_text, 712));

        auto duplicate = plain(ids(65, 19));
        duplicate.injection = {pattern(3 * width, 713, 0.75F), 3, {0, 0, 2}};
        const auto duplicate_result = seq.run(name + "duplicate-injection-positions", duplicate);
        std::fill_n(duplicate.injection.values.begin(), width, 9.0F);
        require(seq.run(name + "duplicate-overwritten-first-row", duplicate) == duplicate_result,
                "duplicate injection did not preserve last-write-wins");
        auto collapsed = duplicate;
        collapsed.injection.positions = {0, 2};
        collapsed.injection.tokens = 2;
        collapsed.injection.values.erase(collapsed.injection.values.begin(), collapsed.injection.values.begin() + width);
        require(seq.run(name + "duplicate-collapsed-last-row", collapsed) == duplicate_result,
                "duplicate injection differs from its final rows");
        std::swap(collapsed.input_ids[1], collapsed.input_ids[3]);
        seq.run(name + "reordered-text", collapsed);

        const auto stable = plain(medium_text);
        const auto stable_result = seq.run(name + "failure-prime", stable);
        const auto failing = plain(ids(129, 23));
        seq.failure(failing);
        require(seq.run(name + "failure-preserves-previous-cache", stable) == stable_result,
                "failed changed-text lookup damaged previous generation");
        seq.run(name + "retry-failed-text", failing);
        seq.run(name + "retry-text-cached", failing);

        seq.guard([&] { seq.decoder.generate({}, 3, true); });
        for (const int64_t budget : {0, -1, 128}) {
            seq.guard([&] { seq.decoder.generate(failing, budget, true); });
        }
        for (int kind = 0; kind < 7; ++kind) {
            auto bad = around(short_text, 714);
            if (kind == 0) { bad.injection.tokens = -1; }
            if (kind == 1) { bad.injection.tokens = 6; }
            if (kind == 2) { bad.injection.positions.pop_back(); }
            if (kind == 3) { bad.injection.values.pop_back(); }
            if (kind == 4) { bad.injection.positions[0] = -1; }
            if (kind == 5) { bad.injection.positions[0] = 5; }
            if (kind == 6) { bad.injection.values.push_back(1.0F); }
            seq.guard([&] { seq.decoder.generate(bad, 3, true); });
        }
        seq.run(name + "guards-preserve-cache", failing);
    }
    require(generated_tokens.size() > 4, "synthetic prompt tests lack generated token diversity");
    std::cout << "PROMPT_QUALITY_OK synthetic_only cases=" << cases << " guards=" << generic_guards
              << " injected_failures=" << failed_graph_cases << " distinct_tokens=" << generated_tokens.size()
              << " assertions=" << assertions << '\n';
    std::cout << "PROMPT_WORK lookup_graphs=" << total_lookup_graphs << " lookup_rows=" << total_lookup_rows
              << " row_bytes=" << total_lookup_bytes << " work_failures=" << failures << '\n';
}
}

namespace engine::core {
ggml_status prompt_test_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {
    observed.pending_output = nullptr;
    const auto * lookup = embedding_only(graph);
    if (lookup) {
        Lookup entry;
        entry.output = ggml_graph_node(graph, ggml_graph_n_nodes(graph) - 1);
        entry.ids.resize(static_cast<size_t>(ggml_nelements(lookup->src[1])));
        ggml_backend_tensor_get(lookup->src[1], entry.ids.data(), 0, entry.ids.size() * sizeof(int32_t));
        observed.lookups.push_back(std::move(entry));
        if (observed.fail_on_lookup == observed.lookups.size()) { return GGML_STATUS_FAILED; }
    } else {
        for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
            if (table_lookup(ggml_graph_node(graph, i))) { ++observed.decoder_lookups; }
        }
    }
    const auto status = compute_backend_graph(backend, graph, plan, label);
    if (status == GGML_STATUS_SUCCESS) {
        ++observed.graphs;
        if (lookup) {
            observed.lookups.back().success = true;
            observed.pending_output = observed.lookups.back().output;
        }
    }
    return status;
}
}

void prompt_test_backend_tensor_get(const ggml_tensor * tensor, void * data, size_t offset, size_t size) {
    ggml_backend_tensor_get(tensor, data, offset, size);
    if (tensor != observed.pending_output) { return; }
    require(offset == 0 && size % (width * sizeof(float)) == 0, "unexpected embedding readback range");
    auto & lookup = observed.lookups.back();
    const size_t rows = size / (width * sizeof(float));
    require(lookup.read_rows == 0 && rows > 0 && rows <= lookup.ids.size(), "invalid embedding readback row count");
    for (size_t row = 0; row < rows; ++row) {
        const auto id = lookup.ids[row];
        require(id >= 0 && id < vocabulary, "observed embedding ID outside synthetic vocabulary");
        require(std::memcmp(static_cast<const float *>(data) + row * width,
                            embedding_weights.data() + id * width, width * sizeof(float)) == 0,
                "real embedding row bytes differ from the synthetic weight table");
    }
    lookup.read_rows = rows;
    observed.row_bytes += size;
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_prompt_embeddings_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "invalid prompt test mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open prompt trace");
        core::ExecutionContext execution{{core::BackendType::Cpu, 0, 1}};
        prompt_suite(execution, trace, mode == "work");
        require(trace.good(), "cannot write complete prompt trace");
        require(failures == 0, "prompt lookup still computes placeholder or unchanged-text rows in " +
                std::to_string(failures) + " cases");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
