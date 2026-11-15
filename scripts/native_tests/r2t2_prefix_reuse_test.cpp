
#define main r2t2_original_main
#define r2t2_test_compute_backend_graph r2t2_original_compute_backend_graph
#include "r2t2_prefill_test.cpp"
#undef r2t2_test_compute_backend_graph
#undef main

#include <functional>
#include <limits>

namespace {
size_t prefix_failures = 0;
size_t reuse_cases = 0;
size_t executed_graphs = 0;
size_t required_graphs = 0;
int fail_compute_countdown = 0;
size_t injected_failures = 0;

std::vector<float> prompt(int64_t steps) {
    return pattern(static_cast<size_t>(steps * width), 911, 0.3F);
}

void observe_work(const std::string & label, size_t expected, bool enforce) {
    require(!work.empty(), label + ": final output block did not execute");

    for (size_t i = 0; i < work.size(); ++i) {
        const bool final = i + 1 == work.size();
        require(work[i].head == (final ? 1 : 0) && work[i].norm == (final ? 1 : 0),
                label + ": final output graph work changed");
    }
    const bool correct = work.size() == expected;
    if (enforce && !correct) { ++prefix_failures; }
    ++reuse_cases;
    executed_graphs += work.size();
    required_graphs += expected;
    std::cout << (correct ? "WORK_OK " : "WORK_RED ") << label
              << " computed_graphs=" << work.size() << " expected_graphs=" << expected << '\n';
}

void exercise(modules::QwenCausalDecodeRuntime & runtime, std::ofstream & trace,
              const std::string & label, const std::vector<float> & values,
              int64_t steps, int64_t capacity, int64_t chunk, size_t expected, bool enforce) {
    work.clear();
    auto result = runtime.prefill_embeddings_into_cache(values, steps, capacity, chunk);
    observe_work(label, expected, enforce);
    require(runtime.decode_current_end() == steps && runtime.decode_valid_steps() == steps,
            label + ": prefill cache position changed");
    require(runtime.decode_cache_steps() >= capacity, label + ": cache capacity changed");
    record(trace, steps);
    record(trace, runtime.decode_cache_steps());
    record_result(trace, result, 1);

    for (int i = 0; i < 2; ++i) {
        const auto token = static_cast<int32_t>(
            std::max_element(result.logits.begin(), result.logits.end()) - result.logits.begin());
        greedy_tokens.insert(token);
        record(trace, token);
        result = runtime.decode_token(token);
        require(runtime.decode_current_end() == steps + i + 1 &&
                runtime.decode_valid_steps() == steps + i + 1, label + ": continuation position changed");
        record(trace, runtime.decode_current_end());
        record(trace, runtime.decode_valid_steps());
        record_result(trace, result, 1);
    }
}

void boundaries(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    exercise(runtime, trace, "cold193", prompt(193), 193, 384, 64, 4, enforce);
    exercise(runtime, trace, "same193-after-generation", prompt(193), 193, 384, 64, 1, enforce);
    exercise(runtime, trace, "shorten-to129", prompt(129), 129, 384, 64, 1, enforce);

    exercise(runtime, trace, "extend129-to193", prompt(193), 193, 384, 64, 2, enforce);
    exercise(runtime, trace, "exact128-final-block", prompt(128), 128, 384, 64, 1, enforce);
    exercise(runtime, trace, "exact64-final-block", prompt(64), 64, 384, 64, 1, enforce);
    exercise(runtime, trace, "below64", prompt(63), 63, 384, 64, 1, enforce);
    exercise(runtime, trace, "above64-shape-change", prompt(65), 65, 384, 64, 2, enforce);
    exercise(runtime, trace, "same65", prompt(65), 65, 384, 64, 1, enforce);
}

void changed_prefix(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    auto values = prompt(129);
    exercise(runtime, trace, "change-control", values, 129, 256, 64, 3, enforce);
    values[63 * width] += 0.125F;
    exercise(runtime, trace, "change-last-row-before64", values, 129, 256, 64, 3, enforce);
    values[64 * width] = 0.0F;
    exercise(runtime, trace, "change-first-row-at64", values, 129, 256, 64, 2, enforce);
    values[64 * width] = -0.0F;
    exercise(runtime, trace, "signed-zero-byte-change-at64", values, 129, 256, 64, 2, enforce);
    values[127 * width] += 0.125F;
    exercise(runtime, trace, "change-last-row-before128", values, 129, 256, 64, 2, enforce);
    values[128 * width] += 0.125F;
    exercise(runtime, trace, "change-final-row-at128", values, 129, 256, 64, 1, enforce);
    values[0] = std::nextafter(values[0], std::numeric_limits<float>::infinity());
    exercise(runtime, trace, "one-bit-prefix-change", values, 129, 256, 64, 3, enforce);
}

void shapes(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    const auto values = prompt(129);
    exercise(runtime, trace, "capacity-control", values, 129, 256, 64, 3, enforce);
    exercise(runtime, trace, "capacity-rebuild", values, 129, 384, 64, 3, enforce);
    exercise(runtime, trace, "same-grown-capacity", values, 129, 384, 64, 1, enforce);
    exercise(runtime, trace, "same-graph-smaller-request", values, 129, 256, 64, 1, enforce);
    exercise(runtime, trace, "chunk-change32", values, 129, 256, 32, 5, enforce);
    exercise(runtime, trace, "same-chunk32", values, 129, 256, 32, 1, enforce);
    exercise(runtime, trace, "chunk-change64", values, 129, 256, 64, 3, enforce);
    runtime.release_runtime_graphs();
    require(runtime.decode_current_end() == 0 && runtime.decode_valid_steps() == 0 &&
            runtime.decode_cache_steps() == 0, "release did not reset decode state");
    exercise(runtime, trace, "released-graphs", values, 129, 256, 64, 3, enforce);
}

void alternate_apis(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime feeder(fixture.execution, config_for(1), fixture.weights);
    const auto state = feeder.prefill_embeddings(prompt(7), 7).state;
    const auto batched = feeder.prefill_embeddings_batched(prompt(7), 1, 7).state;
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    const auto values = prompt(129);
    exercise(runtime, trace, "api-control", values, 129, 256, 64, 3, enforce);
    exercise(runtime, trace, "api-control-warm", values, 129, 256, 64, 1, enforce);
    using Runtime = modules::QwenCausalDecodeRuntime;
    const std::vector<std::pair<std::string, std::function<void(Runtime &)>>> actions = {
        {"prefill_tokens", [](Runtime & owner) { owner.prefill_tokens({1, 4, 7}); }},
        {"prefill_embeddings", [](Runtime & owner) { owner.prefill_embeddings(prompt(7), 7); }},
        {"prefill_tokens_into_decode_cache", [](Runtime & owner) {
            owner.prefill_tokens_into_decode_cache({1, 4, 7}, 256);
        }},
        {"prefill_tokens_batched", [](Runtime & owner) { owner.prefill_tokens_batched({1, 4, 7}, 1, 3); }},
        {"prefill_embeddings_batched", [](Runtime & owner) { owner.prefill_embeddings_batched(prompt(7), 1, 7); }},
        {"start_decode_tokens", [&state](Runtime & owner) { owner.start_decode_tokens(state, 256); }},
        {"start_decode_embeddings", [&state](Runtime & owner) { owner.start_decode_embeddings(state, 256); }},
        {"start_decode_tokens_batched", [&batched](Runtime & owner) { owner.start_decode_tokens_batched(batched, 256); }},
        {"start_decode_embeddings_batched", [&batched](Runtime & owner) { owner.start_decode_embeddings_batched(batched, 256); }},
    };
    for (const auto & action : actions) {
        action.second(runtime);
        exercise(runtime, trace, "after-" + action.first, values, 129, 256, 64, 3, enforce);
    }
}

void failed_prefill(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    const auto values = prompt(193);
    exercise(runtime, trace, "failure-control", values, 193, 320, 64, 4, enforce);
    work.clear();
    bool rejected = false;
    try { runtime.prefill_embeddings_into_cache({1.0F}, 193, 320, 64); }
    catch (const std::exception &) { rejected = true; }
    require(rejected && work.empty(), "invalid-size prefill did not reject before compute");
    exercise(runtime, trace, "after-invalid-prefill", values, 193, 320, 64, 4, enforce);
    auto changed = values;
    changed[0] += 0.25F;
    work.clear();
    fail_compute_countdown = 2;
    rejected = false;
    try { runtime.prefill_embeddings_into_cache(changed, 193, 320, 64); }
    catch (const std::exception &) { rejected = true; }
    fail_compute_countdown = 0;
    require(rejected && injected_failures == 1 && work.size() == 1,
            "failure control did not mutate one prefix block before compute failure");
    exercise(runtime, trace, "after-partial-compute-failure", values, 193, 320, 64, 4, enforce);
    exercise(runtime, trace, "reuse-after-recovery", values, 193, 320, 64, 1, enforce);
}

void nonfinite_suffix(Fixture & fixture, std::ofstream & trace, bool enforce) {
    modules::QwenCausalDecodeRuntime runtime(fixture.execution, config_for(1), fixture.weights);
    auto poisoned = prompt(193);
    poisoned[192 * width] = std::numeric_limits<float>::quiet_NaN();
    const auto invalid_output = runtime.prefill_embeddings_into_cache(poisoned, 193, 320, 64);
    require(std::any_of(invalid_output.logits.begin(), invalid_output.logits.end(),
                       [](float value) { return !std::isfinite(value); }),
            "NaN-tail control failed to poison old KV/output");


    exercise(runtime, trace, "finite129-after-NaN193", prompt(129), 129, 320, 64, 1, enforce);
    exercise(runtime, trace, "extend-after-NaN-recovery", prompt(193), 193, 320, 64, 2, enforce);
}
}

namespace engine::core {
ggml_status r2t2_prefix_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {
    if (fail_compute_countdown > 0 && --fail_compute_countdown == 0) {
        ++injected_failures;
        return GGML_STATUS_FAILED;
    }
    return r2t2_original_compute_backend_graph(backend, graph, plan, label);
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_prefix_reuse_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "unknown test mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open exact output trace");
        Fixture fixture;
        const bool enforce = mode == "work";
        boundaries(fixture, trace, enforce);
        changed_prefix(fixture, trace, enforce);
        shapes(fixture, trace, enforce);
        alternate_apis(fixture, trace, enforce);
        failed_prefill(fixture, trace, enforce);
        nonfinite_suffix(fixture, trace, enforce);
        require(trace.good(), "could not write exact output trace");
        std::cout << "QUALITY_OK synthetic_only cases=" << reuse_cases << " assertions=" << assertions
                  << " continuation_steps=" << reuse_cases * 2 << " computed_graphs=" << executed_graphs
                  << " expected_graphs=" << required_graphs << " injected_failures=" << injected_failures << '\n';
        require(prefix_failures == 0, "exact-prefix graph work remains in " +
                std::to_string(prefix_failures) + " cases");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
