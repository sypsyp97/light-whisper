// Reuse only the established deterministic CPU weights/configuration and helpers.
#define main inherited_prefill_main
#define r2t2_test_compute_backend_graph inherited_prefill_compute
#include "r2t2_prefill_test.cpp"
#undef r2t2_test_compute_backend_graph
#undef main

namespace {
using Runtime = modules::QwenCausalDecodeRuntime;
using Result = modules::QwenCausalDecodeStepResult;
struct Observation {
    int decoder_graphs = 0;
    int comparisons = 0;
    int invalid_comparisons = 0;
    int backend_copies = 0;
    int unsafe_reads = 0;
    size_t upload_bytes = 0;
    size_t readback_bytes = 0;
};
Observation observed;
bool observing = false;
bool enforce_work = false;
const ggml_tensor * current_source = nullptr;
int fail_decoder_graph = 0;
size_t device_cases = 0;
size_t semantic_failures = 0;

void check(bool condition, const std::string & message) {
    if (enforce_work && !condition) {
        ++semantic_failures;
        std::cout << "DEVICE_RED " << message << '\n';
    }
}

struct Observe {
    explicit Observe(const ggml_tensor * source) {
        observed = {};
        current_source = source;
        observing = true;
    }
    ~Observe() { observing = false; current_source = nullptr; fail_decoder_graph = 0; }
};

// A malformed input must be rejected before calling ggml. The negative control
// throws here instead of allowing ggml's unbacked-tensor assertion to kill the suite.
struct UnsafeBackendRead : std::runtime_error {
    UnsafeBackendRead() : std::runtime_error("unbacked input reached backend read") {}
};

void note_get(const ggml_tensor * tensor, size_t bytes) {
    if (!observing) { return; }
    if (tensor == current_source && !tensor->buffer && !tensor->view_src) {
        ++observed.unsafe_reads;
        throw UnsafeBackendRead();
    }
    if (tensor == current_source || (tensor->type == GGML_TYPE_F32 && tensor->ne[0] == width &&
                                    tensor->ne[1] > 1)) {
        observed.readback_bytes += bytes;
    }
}

void note_set(const ggml_tensor * tensor, size_t bytes) {
    if (observing && tensor->type == GGML_TYPE_F32 && tensor->ne[0] == width &&
        tensor->ne[2] == 1 && tensor->ne[3] == 1) {
        observed.upload_bytes += bytes;
    }
}

struct DeviceInput {
    ggml_context * context = ggml_init({64 * 1024, nullptr, true});
    ggml_backend_buffer_t buffer = nullptr;
    ggml_tensor * tensor = nullptr;
    DeviceInput(ggml_backend_t backend, int64_t steps, int64_t columns = width,
                ggml_type type = GGML_TYPE_F32, int64_t planes = 1, bool allocate = true) {
        require(context != nullptr, "device input metadata allocation failed");
        tensor = ggml_new_tensor_3d(context, type, columns, steps, planes);
        if (allocate) {
            buffer = ggml_backend_alloc_ctx_tensors(context, backend);
            require(buffer != nullptr, "device input buffer allocation failed");
        }
    }
    void write(const std::vector<float> & values) {
        require(ggml_nbytes(tensor) == values.size() * sizeof(float), "test input size mismatch");
        ggml_backend_tensor_set(tensor, values.data(), 0, ggml_nbytes(tensor));
    }
    ~DeviceInput() { ggml_backend_buffer_free(buffer); ggml_free(context); }
};

void same_result(const Result & expected, const Result & actual, const std::string & label) {
    require(expected.token_id == actual.token_id, label + ": token result differs");
    const std::vector<float> * left[] = {&expected.logits, &expected.hidden};
    const std::vector<float> * right[] = {&actual.logits, &actual.hidden};
    for (int i = 0; i < 2; ++i) {
        require(left[i]->size() == right[i]->size(), label + ": output shape differs");
        require(left[i]->empty() || std::memcmp(left[i]->data(), right[i]->data(),
                left[i]->size() * sizeof(float)) == 0, label + ": output differs bit-for-bit");
        for (float value : *right[i]) { require(std::isfinite(value), label + ": nonfinite output"); }
    }
}

void check_observation(const std::string & label, int expected_graphs, bool compare_prefix) {
    std::cout << "DEVICE_WORK " << label << " graphs=" << observed.decoder_graphs
              << " expected=" << expected_graphs << " comparisons=" << observed.comparisons
              << " backend_copies=" << observed.backend_copies << " upload_bytes=" << observed.upload_bytes
              << " readback_bytes=" << observed.readback_bytes << '\n';
    check(observed.decoder_graphs == expected_graphs, label + ": wrong number of decoder blocks");
    check(observed.upload_bytes == 0 && observed.readback_bytes == 0,
          label + ": embedding data crossed the host boundary");
    check(observed.backend_copies > 0, label + ": no backend embedding copy observed");
    check(observed.invalid_comparisons == 0, label + ": prefix equality did not use I32 views");
    if (compare_prefix) { check(observed.comparisons > 0, label + ": no backend exact-prefix comparison"); }
}

Result call_device(Runtime & runtime, ggml_tensor * input, int64_t steps, int64_t capacity, int64_t chunk) {
    Observe active(input);
    return runtime.prefill_device_embeddings_into_cache(input, steps, capacity, chunk);
}

void finite_case(Fixture & fixture, Runtime & runtime, Runtime & oracle, std::ofstream & trace,
                 DeviceInput & input, const std::vector<float> & values, int mode,
                 int64_t steps, int64_t capacity, int64_t chunk, int expected_graphs,
                 bool compare_prefix, const std::string & label) {
    input.write(values);
    // Independent fresh-vector oracle cannot share either prompt snapshot or KV.
    oracle.release_runtime_graphs();
    auto expected = oracle.prefill_embeddings_into_cache(values, steps, capacity, chunk);
    auto actual = call_device(runtime, input.tensor, steps, capacity, chunk);
    check_observation(label, expected_graphs, compare_prefix);
    same_result(expected, actual, label);
    require(runtime.decode_current_end() == steps && runtime.decode_valid_steps() == steps,
            label + ": incorrect prefill cache position");
    require(runtime.decode_cache_steps() >= capacity, label + ": cache capacity too small");
    record(trace, steps);
    record_result(trace, actual, mode);
    // Matching four real continuations detects stale or missing KV rows.
    for (int i = 0; i < 4; ++i) {
        const int32_t token = mode == 2 ? 7 + 19 * i : static_cast<int32_t>(
            std::max_element(expected.logits.begin(), expected.logits.end()) - expected.logits.begin());
        expected = oracle.decode_token(token);
        actual = runtime.decode_token(token);
        same_result(expected, actual, label + ": continuation " + std::to_string(i));
        require(runtime.decode_current_end() == steps + i + 1 &&
                runtime.decode_valid_steps() == steps + i + 1, label + ": continuation cache position");
        record(trace, token);
        record_result(trace, actual, mode);
    }
    ++device_cases;
}

void finite_suite(Fixture & fixture, std::ofstream & trace) {
    for (int mode = 0; mode < 3; ++mode) {
        auto weights = fixture.weights;
        if (mode == 2) { weights.lm_head.reset(); }
        Runtime runtime(fixture.execution, config_for(mode), weights);
        Runtime oracle(fixture.execution, config_for(mode), weights);
        for (int64_t steps : {1, 63, 64, 65, 127, 128, 129, 193, 3}) {
            DeviceInput input(fixture.execution.backend(), steps);
            finite_case(fixture, runtime, oracle, trace, input,
                        pattern(static_cast<size_t>(width * steps), static_cast<uint32_t>(steps + 200), 0.3F),
                        mode, steps, 320, 64, static_cast<int>((steps + 63) / 64), false,
                        "mode" + std::to_string(mode) + "-boundary" + std::to_string(steps));
        }
        DeviceInput input(fixture.execution.backend(), 129);
        auto values = pattern(width * 129, 719, 0.3F);
        auto run = [&](const char * label, int blocks, bool compare = true, int64_t chunk = 64,
                       int64_t capacity = 320) {
            finite_case(fixture, runtime, oracle, trace, input, values, mode, 129, capacity,
                        chunk, blocks, compare, "mode" + std::to_string(mode) + '-' + label);
        };
        run("new-source", 3, false);
        run("identical-repeat", 1);
        values[70 * width + 3] += 0.01F;
        run("mutate-borrowed-middle", 2);
        values[3] += 0.01F;
        run("mutate-borrowed-first", 3);
        values[128 * width + 3] += 0.01F;
        run("mutate-final-tail", 1);
        run("chunk-change", 8, false, 17);
        run("repeat-chunk17", 1, true, 17);
        run("capacity-growth", 8, false, 17, 512);
        runtime.release_runtime_graphs();
        require(runtime.decode_current_end() == 0 && runtime.decode_valid_steps() == 0 &&
                runtime.decode_cache_steps() == 0, "release did not reset cache state");
        run("release-invalidates", 3, false);

        // A destroyed caller allocation must not be retained as the previous prompt.
        { DeviceInput temporary(fixture.execution.backend(), 129);
          finite_case(fixture, runtime, oracle, trace, temporary, values, mode, 129, 320, 64,
                      1, true, "mode" + std::to_string(mode) + "-temporary-source"); }
        run("after-source-destruction", 1);
    }
}

void bits_suite(Fixture & fixture) {
    Runtime runtime(fixture.execution, config_for(0), fixture.weights);
    DeviceInput input(fixture.execution.backend(), 129);
    auto values = pattern(width * 129, 911, 0.3F);
    auto bits = [&](uint32_t value) { std::memcpy(&values[70 * width + 3], &value, sizeof(value)); };
    auto run = [&](const char * label, int blocks, bool compare) {
        input.write(values);
        call_device(runtime, input.tensor, 129, 320, 64);
        check_observation(label, blocks, compare);
    };
    bits(0x00000000U); run("positive-zero", 3, false);
    run("same-positive-zero", 1, true);
    bits(0x80000000U); run("signed-zero-bit-mismatch", 2, true);
    bits(0x7fc00001U); run("quiet-nan-first-payload", 2, true);
    run("identical-nan-bits", 1, true);
    bits(0x7fc00002U); run("nan-payload-bit-mismatch", 2, true);
    bits(0xffc00002U); run("nan-sign-bit-mismatch", 2, true);
    // NaN is a bit-comparison adversary, not an accepted finite-output oracle.
}

void invalidation_suite(Fixture & fixture, std::ofstream & trace) {
    Runtime runtime(fixture.execution, config_for(0), fixture.weights);
    Runtime oracle(fixture.execution, config_for(0), fixture.weights);
    DeviceInput input(fixture.execution.backend(), 129);
    auto values = pattern(width * 129, 1011, 0.3F);
    auto restore = [&](const std::string & label, int blocks) {
        finite_case(fixture, runtime, oracle, trace, input, values, 0, 129, 320, 64,
                    blocks, false, label);
    };
    restore("transition-initial", 3);
    { Observe active(nullptr);
      runtime.prefill_embeddings_into_cache(values, 129, 320, 64); }
    check(observed.decoder_graphs == 3, "device-to-host transition reused a device prompt");
    restore("host-to-device-invalidates", 3);
    runtime.prefill_tokens({1, 2, 3});
    restore("token-prefill-invalidates", 3);

    auto reject = [&](const std::string & label, ggml_tensor * tensor, int64_t steps,
                      int64_t capacity, int64_t chunk) {
        bool rejected = false;
        try { call_device(runtime, tensor, steps, capacity, chunk); }
        catch (const UnsafeBackendRead &) { /* Missing validation, reported below. */ }
        catch (const std::exception &) { rejected = true; }
        check(rejected, label + ": malformed input was not rejected by runtime validation");
        check(observed.decoder_graphs == 0 && observed.upload_bytes == 0 &&
              observed.readback_bytes == 0 && observed.unsafe_reads == 0,
              label + ": invalid input reached backend work before validation");
        restore(label + "-invalidates", 3);
    };
    reject("null-input", nullptr, 129, 320, 64);
    reject("zero-steps", input.tensor, 0, 320, 64);
    reject("steps-shape-mismatch", input.tensor, 128, 320, 64);
    reject("small-cache", input.tensor, 129, 128, 64);
    reject("zero-chunk", input.tensor, 129, 320, 0);
    DeviceInput wrong_width(fixture.execution.backend(), 129, width - 1);
    reject("hidden-width-mismatch", wrong_width.tensor, 129, 320, 64);
    DeviceInput wrong_type(fixture.execution.backend(), 129, width, GGML_TYPE_F16);
    reject("non-f32", wrong_type.tensor, 129, 320, 64);
    DeviceInput batched(fixture.execution.backend(), 129, width, GGML_TYPE_F32, 2);
    reject("extra-plane", batched.tensor, 129, 320, 64);
    DeviceInput padded(fixture.execution.backend(), 129, width + 1);
    auto * strided = ggml_view_2d(padded.context, padded.tensor, width, 129,
                                 (width + 1) * sizeof(float), 0);
    require(ggml_backend_view_init(strided) == GGML_STATUS_SUCCESS, "strided fixture view init failed");
    require(!ggml_is_contiguous(strided), "strided fixture unexpectedly contiguous");
    reject("noncontiguous", strided, 129, 320, 64);
    DeviceInput unbacked(fixture.execution.backend(), 129, width, GGML_TYPE_F32, 1, false);
    reject("unbacked", unbacked.tensor, 129, 320, 64);

    // Fail after a real second-block compute, leaving partially changed KV.
    auto changed = values;
    changed[3] += 0.2F;
    input.write(changed);
    bool failed = false;
    try {
        Observe active(input.tensor);
        fail_decoder_graph = 2;
        runtime.prefill_device_embeddings_into_cache(input.tensor, 129, 320, 64);
    } catch (const std::exception &) { failed = true; }
    require(failed && observed.decoder_graphs == 2, "compute-failure injection did not execute");
    restore("partial-compute-failure-invalidates", 3);
}
}  // namespace

void device_test_set(ggml_tensor * tensor, const void * data, size_t offset, size_t size) {
    note_set(tensor, size); ggml_backend_tensor_set(tensor, data, offset, size);
}
void device_test_set_async(ggml_backend_t backend, ggml_tensor * tensor,
                           const void * data, size_t offset, size_t size) {
    note_set(tensor, size); ggml_backend_tensor_set_async(backend, tensor, data, offset, size);
}
void device_test_get(const ggml_tensor * tensor, void * data, size_t offset, size_t size) {
    note_get(tensor, size); ggml_backend_tensor_get(tensor, data, offset, size);
}
void device_test_get_async(ggml_backend_t backend, const ggml_tensor * tensor,
                           void * data, size_t offset, size_t size) {
    note_get(tensor, size); ggml_backend_tensor_get_async(backend, tensor, data, offset, size);
}
void device_test_copy(const ggml_tensor * source, ggml_tensor * target) {
    if (observing) { ++observed.backend_copies; }
    ggml_backend_tensor_copy(source, target);
}
void device_test_copy_async(ggml_backend_t source_backend, ggml_backend_t target_backend,
                            const ggml_tensor * source, ggml_tensor * target) {
    if (observing) { ++observed.backend_copies; }
    ggml_backend_tensor_copy_async(source_backend, target_backend, source, target);
}
namespace engine::core {
std::vector<float> device_test_read_f32(const ggml_tensor * tensor) {
    note_get(tensor, ggml_nbytes(tensor)); return read_tensor_f32(tensor);
}
void device_test_read_f32_into(const ggml_tensor * tensor, std::vector<float> & values) {
    note_get(tensor, ggml_nbytes(tensor)); read_tensor_f32_into(tensor, values);
}
ggml_status device_test_compute(ggml_backend_t backend, ggml_cgraph * graph,
                                ggml_backend_graph_plan_t plan, const char * label) {
    bool decoder = false;
    if (observing) {
        for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
            const auto * node = ggml_graph_node(graph, i);
            decoder = decoder || node->op == GGML_OP_FLASH_ATTN_EXT;
            if (node->op == GGML_OP_CPY) { ++observed.backend_copies; }
            if (node->op == GGML_OP_COUNT_EQUAL) {
                ++observed.comparisons;
                for (int source = 0; source < 2; ++source) {
                    if (!node->src[source] || node->src[source]->type != GGML_TYPE_I32 ||
                        !node->src[source]->view_src) { ++observed.invalid_comparisons; }
                }
            }
        }
        if (decoder) { ++observed.decoder_graphs; }
    }
    const auto status = compute_backend_graph(backend, graph, plan, label);
    return observing && decoder && observed.decoder_graphs == fail_decoder_graph
        ? GGML_STATUS_FAILED : status;
}
}  // namespace engine::core

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: device_embeddings_test equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "unknown test mode");
        enforce_work = mode == "work";
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open device embedding trace");
        Fixture fixture;
        finite_suite(fixture, trace);
        if (enforce_work) { bits_suite(fixture); invalidation_suite(fixture, trace); }
        require(trace.good(), "incomplete output trace");
        std::cout << "QUALITY_OK synthetic_cpu cases=" << device_cases << " assertions=" << assertions
                  << " continuation_steps=" << device_cases * 4
                  << " semantic_failures=" << semantic_failures << '\n';
        require(semantic_failures == 0, "device embedding contract failed in " +
                std::to_string(semantic_failures) + " checks");
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
