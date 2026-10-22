// Real dispatcher tests. No replacement matmul, controller, or cache implementation.
#include "ggml.h"
#include "ggml-backend.h"
#include "ggml-alloc.h"
#include "ggml-backend-impl.h"
#include "ggml-cuda.h"
#include "ggml-cuda/mmvf.cuh"
#include "observer.h"
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
void require(bool okay, const char * message) { if (!okay) throw std::runtime_error(message); }
void check(cudaError_t status) { require(status == cudaSuccess, cudaGetErrorString(status)); }
constexpr int lowering = 4; // Frozen public contract; baseline enum need not declare a new name.
struct Call { int n; uintptr_t context, output; };
struct Mark { cudaEvent_t event; size_t at; };
struct Sample { int n; float milliseconds; size_t at; };
struct Observer {
    bool active = false, raw_capture = false, inject_parity = false, injected = false, fatal_elapsed = false, parity_last = true;
    float serial_ms = 100.0f, grouped_ms = 90.0f;
    size_t calls = 0, marks = 0, samples = 0, syncs = 0, allocations = 0, event_creates = 0;
    std::array<Call, 65536> call{};
    std::array<Mark, 1024> mark{};
    std::array<Sample, 1024> sample{};
    void reset(float serial, float grouped, bool fault = false) {
        active = false; raw_capture = false; inject_parity = fault; injected = false; fatal_elapsed = false;
        serial_ms = serial; grouped_ms = grouped;
        calls = marks = samples = syncs = allocations = event_creates = 0;
    }
    size_t position(cudaEvent_t event) const {
        for (size_t i = marks; i; --i) if (mark[i - 1].event == event) return mark[i - 1].at;
        throw std::runtime_error("fixture: elapsed event not observed at real cudaEventRecord");
    }
} observer;

// Fault affects exactly one byte, after the real grouped kernel, on the same stream.
__global__ void flip_output_byte(unsigned char * output, size_t offset) { output[offset] ^= 1; }
unsigned next_lifetime = 0;
struct Backend {
    ggml_backend_t ptr = ggml_backend_cuda_init(0);
    unsigned lifetime = ++next_lifetime;
    Backend() { require(ptr != nullptr, "CUDA backend 0 unavailable"); }
    ~Backend() { observer.active = false; ggml_backend_free(ptr); }
    ggml_backend_cuda_context & context() { return *static_cast<ggml_backend_cuda_context *>(ptr->context); }
};
struct Shape {
    int k = 64, vocab = 33, n = 4;
    ggml_type type = GGML_TYPE_F16;
    ggml_prec precision = GGML_PREC_DEFAULT;
    bool padded_weight = false, padded_input = false;
};
struct Fixture {
    Backend & backend;
    Shape shape;
    bool marked;
    ggml_context * ctx = nullptr;
    ggml_backend_buffer_t buffer = nullptr;
    ggml_tensor * weight_base, * input_base, * weight, * input, * output;
    ggml_cgraph * graph, * scalar_graph;
    std::vector<ggml_tensor *> scalar_outputs;
    std::vector<float> expected;
    std::vector<uint32_t> actual_bits;
    Fixture(Backend & backend, Shape shape = {}, bool marked = true) : backend(backend), shape(shape), marked(marked) {
        ctx = ggml_init({2 * 1024 * 1024, nullptr, true});
        require(ctx != nullptr, "ggml_init failed");
        weight_base = ggml_new_tensor_2d(ctx, shape.type, shape.k + (shape.padded_weight ? 2 : 0), shape.vocab);
        input_base = ggml_new_tensor_2d(ctx, GGML_TYPE_F32, shape.k + (shape.padded_input ? 2 : 0), shape.n);
        weight = shape.padded_weight ? ggml_view_2d(ctx, weight_base, shape.k, shape.vocab, weight_base->nb[1], 0) : weight_base;
        input = shape.padded_input ? ggml_view_2d(ctx, input_base, shape.k, shape.n, input_base->nb[1], 0) : input_base;
        output = ggml_mul_mat(ctx, weight, input);
        ggml_mul_mat_set_prec(output, shape.precision);
        if (marked) ggml_mul_mat_set_lowering(output, static_cast<ggml_mul_mat_lowering>(lowering));
        graph = ggml_new_graph_custom(ctx, 64, false);
        ggml_build_forward_expand(graph, output);
        scalar_graph = ggml_new_graph_custom(ctx, 128, false);
        for (int j = 0; j < shape.n; ++j) {
            auto * column = ggml_view_2d(ctx, input_base, shape.k, 1, input_base->nb[1], size_t(j) * input_base->nb[1]);
            auto * value = ggml_mul_mat(ctx, weight, column);
            ggml_mul_mat_set_prec(value, shape.precision);
            scalar_outputs.push_back(value);
            ggml_build_forward_expand(scalar_graph, value);
        }
        buffer = ggml_backend_alloc_ctx_tensors(ctx, backend.ptr);
        require(buffer != nullptr, "real backend allocation failed");
        fill(1);
    }
    ~Fixture() { observer.active = false; ggml_backend_buffer_free(buffer); ggml_free(ctx); }
    void fill(uint32_t seed) {
        observer.active = false;
        std::vector<float> weights(ggml_nelements(weight_base));
        for (auto & value : weights) { seed = 1664525U * seed + 1013904223U; value = float(int(seed >> 17) - 16384) / 16384.0f; }
        if (shape.type == GGML_TYPE_F16) {
            std::vector<ggml_fp16_t> half(weights.size());
            for (size_t i = 0; i < weights.size(); ++i) half[i] = ggml_fp32_to_fp16(weights[i]);
            ggml_backend_tensor_set(weight_base, half.data(), 0, half.size() * sizeof(half[0]));
        } else {
            require(shape.type == GGML_TYPE_F32, "unsupported fixture weight type");
            ggml_backend_tensor_set(weight_base, weights.data(), 0, weights.size() * sizeof(float));
        }
        std::vector<float> values(ggml_nelements(input_base));
        for (auto & value : values) { seed = 1664525U * seed + 1013904223U; value = float(int(seed >> 17) - 16384) / 32768.0f; }
        ggml_backend_tensor_set(input_base, values.data(), 0, values.size() * sizeof(float));
        require(ggml_backend_graph_compute(backend.ptr, scalar_graph) == GGML_STATUS_SUCCESS, "scalar oracle failed");
        expected.resize(size_t(shape.vocab) * shape.n);
        for (int j = 0; j < shape.n; ++j) ggml_backend_tensor_get(scalar_outputs[j], expected.data() + size_t(j) * shape.vocab, 0, shape.vocab * sizeof(float));
        for (float value : expected) require(std::isfinite(value), "scalar oracle nonfinite");
    }
    size_t run(bool capture = false) {
        observer.active = false;
        auto stream = backend.context().stream();
        check(cudaMemsetAsync(output->data, 0x7f, ggml_nbytes(output), stream));
        check(cudaStreamSynchronize(stream));
        observer.raw_capture = capture;
        if (capture) check(cudaStreamBeginCapture(stream, cudaStreamCaptureModeRelaxed));
        observer.active = true;
        // Both public entries reach the real backend iface.graph_compute.
        auto status = capture ? ggml_backend_graph_compute_async(backend.ptr, graph)
                              : ggml_backend_graph_compute(backend.ptr, graph);
        observer.active = false;
        require(status == GGML_STATUS_SUCCESS, "candidate graph failed");
        if (capture) {
            cudaGraph_t captured = nullptr;
            cudaGraphExec_t instance = nullptr;
            check(cudaStreamEndCapture(stream, &captured));
            check(cudaGraphInstantiate(&instance, captured, nullptr, nullptr, 0));
            check(cudaGraphLaunch(instance, stream));
            check(cudaStreamSynchronize(stream));
            check(cudaGraphExecDestroy(instance));
            check(cudaGraphDestroy(captured));
        }
        std::vector<float> actual(expected.size());
        ggml_backend_tensor_get(output, actual.data(), 0, actual.size() * sizeof(float));
        actual_bits.resize(actual.size());
        std::memcpy(actual_bits.data(), actual.data(), actual.size() * sizeof(float));
        size_t mismatches = 0;
        // Unmarked operations retain the frozen default result, which may differ
        // from scalar MMVF. The runner compares every such output bit externally.
        if (marked) for (size_t i = 0; i < actual.size(); ++i) mismatches += std::memcmp(&actual[i], &expected[i], sizeof(float)) != 0;
        return mismatches;
    }
};
size_t cases = 0, work_red = 0, numeric_red = 0;
bool work(bool okay, const char * reason) {
    if (!okay) { ++work_red; std::cerr << "WORK_RED " << reason << '\n'; }
    return okay;
}
bool serial_tail(size_t begin, int n) {
    if (observer.calls - begin != size_t(n)) return false;
    for (size_t i = begin; i < observer.calls; ++i) if (observer.call[i].n != 1) return false;
    return true;
}
bool selected_tail(size_t begin, bool grouped, int n) {
    return grouped ? observer.calls == begin + 1 && observer.call[begin].n == 4 : serial_tail(begin, n);
}
bool final_output(Fixture & fixture, bool grouped) {
    const size_t count = grouped ? 1 : size_t(fixture.shape.n);
    if (observer.calls < count) return false;
    for (size_t j = 0; j < count; ++j) {
        const auto & call = observer.call[observer.calls - count + j];
        if (call.output != uintptr_t(fixture.output->data) + j * fixture.output->nb[1]) return false;
    }
    return true;
}
void report(const char * name, const char * phase, Fixture & fixture, size_t mismatches) {
    ++cases;
    if (mismatches) { ++numeric_red; std::cerr << "NUMERIC_RED " << name << '/' << phase << " mismatches=" << mismatches << '\n'; }
    std::cout << "{\"case\":\"" << name << "\",\"phase\":\"" << phase << "\",\"context\":" << uintptr_t(&fixture.backend.context())
        << ",\"lifetime\":" << fixture.backend.lifetime << ",\"device\":0,\"k\":" << fixture.shape.k
        << ",\"lowering\":" << (fixture.marked ? lowering : 0)
        << ",\"weight_tensor\":" << uintptr_t(fixture.weight) << ",\"input_tensor\":" << uintptr_t(fixture.input)
        << ",\"output_tensor\":" << uintptr_t(fixture.output) << ",\"weight_data\":" << uintptr_t(fixture.weight->data)
        << ",\"input_data\":" << uintptr_t(fixture.input->data) << ",\"output_data\":" << uintptr_t(fixture.output->data)
        << ",\"vocab\":" << fixture.shape.vocab << ",\"n\":" << fixture.shape.n << ",\"capture\":" << (observer.raw_capture ? "true" : "false")
        << ",\"elapsed_calls\":" << observer.samples << ",\"allocations\":" << observer.allocations << ",\"syncs\":" << observer.syncs
        << ",\"event_creates\":" << observer.event_creates << ",\"injected\":" << (observer.injected ? "true" : "false")
        << ",\"bit_mismatches\":" << mismatches << ",\"launches\":[";
    for (size_t i = 0; i < observer.calls; ++i) { if (i) std::cout << ','; std::cout << observer.call[i].n; }
    std::cout << "],\"samples\":[";
    for (size_t i = 0; i < observer.samples; ++i) {
        if (i) std::cout << ',';
        std::cout << "{\"n\":" << observer.sample[i].n << ",\"at\":" << observer.sample[i].at << ",\"milliseconds\":";
        const float value = observer.sample[i].milliseconds;
        if (std::isfinite(value)) std::cout << value;
        else std::cout << (std::isnan(value) ? "\"NaN\"" : "\"Infinity\"");
        std::cout << '}';
    }
    std::cout << ']';
    if (!fixture.marked) {
        std::cout << ",\"oracle\":\"frozen_default_external\",\"default_bits\":[";
        for (size_t i = 0; i < fixture.actual_bits.size(); ++i) { if (i) std::cout << ','; std::cout << fixture.actual_bits[i]; }
        std::cout << ']';
    }
    std::cout << "}\n";
}
void cached(const char * name, Fixture & fixture, bool grouped, bool capture = false) {
    fixture.fill(987); // Same key, different values; cached decision must remain immutable.
    observer.reset(100, grouped ? 200 : 1);
    const auto mismatches = fixture.run(capture);
    work(observer.samples == 0, "cache hit timed again");
    work(selected_tail(0, grouped, fixture.shape.n), "cache hit did not execute the cached choice exactly once");
    if (capture) work(observer.allocations == 0 && observer.syncs == 0 && observer.event_creates == 0, "captured hit allocated/timed/synchronized");
    report(name, capture ? "capture_hit" : "cache_hit", fixture, mismatches);
}
void calibrate_fixture(const char * name, Fixture & fixture, float serial_ms, float grouped_ms, bool grouped, bool parity_fault = false, bool last_byte = true) {
    observer.reset(serial_ms, grouped_ms, parity_fault);
    observer.parity_last = last_byte;
    const auto mismatches = fixture.run();
    bool scalar_seen = false, grouped_seen = false;
    size_t last_grouped = 0;
    for (size_t i = 0; i < observer.calls; ++i) {
        scalar_seen |= observer.call[i].n == 1; grouped_seen |= observer.call[i].n == 4;
        if (observer.call[i].n == 4) last_grouped = i;
        work(observer.call[i].context == uintptr_t(&fixture.backend.context()), "kernel used another backend context");
    }
    work(scalar_seen && grouped_seen, "initial eligible request omitted serial/grouped calibration work");
    if (parity_fault) {
        work(observer.injected, "parity fault never reached the stock grouped kernel");
        work(observer.calls >= last_grouped + 5 && serial_tail(observer.calls - 4, 4), "parity failure did not execute final serial output after corrupted grouped output");
    } else {
        work(observer.samples > 0, "initial eligible request did not measure CUDA event timing");
        if (observer.samples) work(selected_tail(observer.sample[observer.samples - 1].at, grouped, 4), "calibration did not execute chosen output after its last timing sample");
    }
    work(final_output(fixture, grouped), "chosen execution did not write the requested output destination");
    report(name, "first", fixture, mismatches);
    cached(name, fixture, grouped);
}
void calibration(const char * name, Backend & backend, float serial_ms, float grouped_ms, bool grouped, bool parity_fault = false, bool last_byte = true) {
    Fixture fixture(backend);
    calibrate_fixture(name, fixture, serial_ms, grouped_ms, grouped, parity_fault, last_byte);
}
void default_request(const char * phase, Fixture & fixture, bool capture = false) {
    require(!fixture.marked, "default control accidentally marked");
    observer.reset(100, 1);
    fixture.run(capture);
    work(observer.samples == 0 && observer.event_creates == 0, "unmarked operation entered event calibration");
    bool default_dispatch = observer.calls <= 1;
    for (size_t i = 0; i < observer.calls; ++i) default_dispatch &= observer.call[i].n == fixture.shape.n;
    work(default_dispatch, "unmarked operation entered scalar-equivalent controller work");
    report("default_lowering", phase, fixture, 0);
}
void fallback(const char * name, Backend & backend, Shape shape, bool capture = false) {
    Fixture fixture(backend, shape);
    observer.reset(100, 1);
    const auto mismatches = fixture.run(capture);
    work(observer.samples == 0, "ineligible/different-key/capture-miss request tuned");
    work(serial_tail(0, shape.n), "fallback did not execute each actual column separately");
    if (capture) work(observer.allocations == 0 && observer.syncs == 0 && observer.event_creates == 0, "capture miss allocated/created events/synchronized");
    report(name, "fallback", fixture, mismatches);
}
void stock() {
    Backend backend;
    for (int k : {64, 66, 1536}) {
        Shape shape; shape.k = k;
        Fixture fixture(backend, shape);
        observer.active = false;
        ggml_cuda_mul_mat_vec_f(backend.context(), fixture.weight, fixture.input, nullptr, fixture.output);
        check(cudaStreamSynchronize(backend.context().stream()));
        std::vector<float> actual(fixture.expected.size());
        ggml_backend_tensor_get(fixture.output, actual.data(), 0, actual.size() * sizeof(float));
        require(std::memcmp(actual.data(), fixture.expected.data(), actual.size() * sizeof(float)) == 0, "stock grouped/scalar bit parity failed");
    }
    std::cout << "{\"status\":\"STOCK_GREEN\",\"cases\":3,\"synthetic\":true}\n";
}
} // namespace

void head_test_mmvf(ggml_backend_cuda_context & ctx, const ggml_tensor * src0, const ggml_tensor * src1,
                    const ggml_tensor * ids, ggml_tensor * dst, const ggml_cuda_mm_fusion_args_host * fusion) {
    if (observer.active) {
        require(observer.calls < observer.call.size(), "fixture observer call capacity exhausted");
        observer.call[observer.calls++] = {int(src1->ne[1]), uintptr_t(&ctx), uintptr_t(dst->data)};
    }
    ggml_cuda_mul_mat_vec_f(ctx, src0, src1, ids, dst, fusion);
    if (observer.active && observer.inject_parity && !observer.injected && src1->ne[1] == 4) {
        flip_output_byte<<<1, 1, 0, ctx.stream()>>>(static_cast<unsigned char *>(dst->data), observer.parity_last ? ggml_nbytes(dst) - 1 : 0);
        check(cudaGetLastError());
        observer.injected = true;
    }
}
cudaError_t head_test_event_record(cudaEvent_t event, cudaStream_t stream) {
    const auto result = cudaEventRecord(event, stream);
    if (observer.active && result == cudaSuccess) {
        require(observer.marks < observer.mark.size(), "fixture observer event capacity exhausted");
        observer.mark[observer.marks++] = {event, observer.calls};
    }
    return result;
}
cudaError_t head_test_elapsed(float * ms, cudaEvent_t begin, cudaEvent_t end) {
    const auto result = cudaEventElapsedTime(ms, begin, end); // Preserve real CUDA API failures.
    if (!observer.active || result != cudaSuccess) return result;
    const size_t first = observer.position(begin), last = observer.position(end);
    bool serial = false, grouped = false;
    for (size_t i = first; i < last; ++i) { serial |= observer.call[i].n == 1; grouped |= observer.call[i].n == 4; }
    require(serial != grouped, "fixture: timing range must contain exactly one real path kind");
    require(observer.samples < observer.sample.size(), "fixture observer sample capacity exhausted");
    // Preserve elapsed-time scaling if the controller times several repetitions.
    const float repetitions = serial ? float(last - first) / 4.0f : float(last - first);
    *ms = (serial ? observer.serial_ms : observer.grouped_ms) * repetitions;
    observer.sample[observer.samples++] = {serial ? 1 : 4, *ms, observer.calls};
    if (observer.fatal_elapsed) return cudaErrorInvalidValue;
    return result;
}
cudaError_t head_test_event_create(cudaEvent_t * event) { if (observer.active) ++observer.event_creates; return cudaEventCreate(event); }
cudaError_t head_test_event_create_flags(cudaEvent_t * event, unsigned int flags) { if (observer.active) ++observer.event_creates; return cudaEventCreateWithFlags(event, flags); }
cudaError_t head_test_event_sync(cudaEvent_t event) { if (observer.active) ++observer.syncs; return cudaEventSynchronize(event); }
cudaError_t head_test_stream_sync(cudaStream_t stream) { if (observer.active) ++observer.syncs; return cudaStreamSynchronize(stream); }
cudaError_t head_test_device_sync() { if (observer.active) ++observer.syncs; return cudaDeviceSynchronize(); }
cudaError_t head_test_malloc(void ** ptr, size_t bytes) { if (observer.active) ++observer.allocations; return cudaMalloc(ptr, bytes); }
cudaError_t head_test_malloc_managed(void ** ptr, size_t bytes, unsigned int flags) { if (observer.active) ++observer.allocations; return cudaMallocManaged(ptr, bytes, flags); }
cudaError_t head_test_malloc_host(void ** ptr, size_t bytes) { if (observer.active) ++observer.allocations; return cudaMallocHost(ptr, bytes); }
cudaError_t head_test_host_alloc(void ** ptr, size_t bytes, unsigned int flags) { if (observer.active) ++observer.allocations; return cudaHostAlloc(ptr, bytes, flags); }
CUresult head_test_mem_create(CUmemGenericAllocationHandle * handle, size_t bytes, const CUmemAllocationProp * prop, unsigned long long flags) {
    if (observer.active) ++observer.allocations; return cuMemCreate(handle, bytes, prop, flags);
}
CUresult head_test_address_reserve(CUdeviceptr * ptr, size_t bytes, size_t alignment, CUdeviceptr address, unsigned long long flags) {
    if (observer.active) ++observer.allocations; return cuMemAddressReserve(ptr, bytes, alignment, address, flags);
}

int main(int argc, char ** argv) {
    try {
        require(argc == 2, "choose stock, contract, or fatal-event");
        require(std::getenv("GGML_CUDA_DISABLE_GRAPHS") != nullptr, "disable automatic graphs in this fixture process; raw captures are explicit");
        check(cudaSetDevice(0));
        const auto & device = ggml_cuda_info().devices[0];
        require(GGML_CUDA_CC_IS_NVIDIA(device.cc), "fixture requires NVIDIA device 0");
        const std::string mode = argv[1];
        if (mode == "stock") { stock(); return 0; }
        if (mode == "fatal-event") {
            Backend backend; Fixture fixture(backend);
            observer.reset(100, 90); observer.fatal_elapsed = true;
            fixture.run();
            work(false, observer.samples ? "CUDA API failure did not take existing fatal route" : "no calibration event reached fatal-error injection");
        } else {
            require(mode == "contract", "unknown mode");
            {
                Backend backend;
                Fixture unmarked(backend, {}, false);
                default_request("before_marked", unmarked);
                default_request("capture_before_marked", unmarked, true);
                // Must still calibrate: default requests cannot occupy the slot.
                calibration("default_did_not_fill_slot", backend, 100, 90, true);
                default_request("after_marked", unmarked);
                default_request("capture_after_marked", unmarked, true);
            }
            { Backend backend; calibration("boundary_accept", backend, 100, 95, true); }
            { Backend backend; calibration("boundary_reject", backend, 100, 96, false); }
            for (float value : {0.0f, -1.0f, std::numeric_limits<float>::quiet_NaN(), std::numeric_limits<float>::infinity()}) {
                { Backend backend; calibration("invalid_serial_time", backend, value, 90, false); }
                { Backend backend; calibration("invalid_grouped_time", backend, 100, value, false); }
            }
            { Backend backend; calibration("parity_reject_first", backend, 100, 1, false, true, false); }
            { Backend backend; calibration("parity_reject_last", backend, 100, 1, false, true, true); }
            {
                Backend backend;
                Fixture original(backend);
                calibrate_fixture("immutable_slot", original, 100, 90, true);
                Shape other_k; other_k.k = 66; fallback("different_k", backend, other_k);
                Shape other_vocab; other_vocab.vocab = 35; fallback("different_vocab", backend, other_vocab);
                Fixture same_key_new_tensors(backend);
                // Both allocations stay live: neither host nor device addresses
                // can accidentally be recycled into a pointer-keyed cache hit.
                require(original.weight != same_key_new_tensors.weight && original.input != same_key_new_tensors.input && original.output != same_key_new_tensors.output,
                        "live same-key fixtures reused host tensor addresses");
                require(original.weight->data != same_key_new_tensors.weight->data && original.input->data != same_key_new_tensors.input->data && original.output->data != same_key_new_tensors.output->data,
                        "live same-key fixtures reused device addresses");
                cached("matching_key_new_tensors", same_key_new_tensors, true);
                cached("original_live_allocation", original, true);
                Backend second; calibration("independent_live_backend", second, 100, 96, false);
                cached("first_context_unchanged", same_key_new_tensors, true);
            }
            { Backend recreated; calibration("recreated_backend", recreated, 100, 90, true); }
            {
                Backend backend;
                Shape type; type.type = GGML_TYPE_F32; fallback("unsupported_type", backend, type);
                Shape precision; precision.precision = GGML_PREC_F32; fallback("unsupported_precision", backend, precision);
                Shape weights; weights.padded_weight = true; fallback("padded_weights", backend, weights);
                Shape input; input.padded_input = true; fallback("padded_input", backend, input);
                for (int n : {1, 2, 3, 5}) { Shape count; count.n = n; fallback("actual_columns", backend, count); }
                calibration("ineligible_did_not_fill_slot", backend, 100, 90, true);
            }
            {
                Backend backend;
                fallback("raw_capture_miss", backend, {}, true);
                calibration("capture_miss_did_not_fill_slot", backend, 100, 90, true);
                Fixture fixture(backend); cached("grouped_capture", fixture, true, true);
                Shape other; other.k = 66; fallback("capture_key_miss", backend, other, true);
            }
            {
                Backend backend; calibration("serial_capture", backend, 100, 96, false);
                Fixture fixture(backend); cached("serial_capture", fixture, false, true);
            }
        }
        std::cout << "{\"status\":\"" << (work_red || numeric_red ? "SEMANTIC_RED" : "PASS") << "\",\"cases\":" << cases
                  << ",\"work_red\":" << work_red << ",\"numeric_red\":" << numeric_red << "}\n";
        return work_red || numeric_red ? 2 : 0;
    } catch (const std::exception & error) {
        observer.active = false;
        std::cerr << "FIXTURE_ERROR " << error.what() << '\n';
        return 1;
    }
}
