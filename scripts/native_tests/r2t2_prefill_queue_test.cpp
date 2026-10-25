#include "engine/framework/modules/transformers/qwen_causal_decode_runtime.h"

#include <cstdlib>
#include <malloc.h>
#include <utility>

namespace {
bool emulate_cuda = true;

// The backend is always real CPU. Only the runtime's saved dispatch type is
// changed; restore the public config immediately after base construction.
struct ScopedBackendType {
    engine::core::BackendConfig & config;
    engine::core::BackendType original;
    explicit ScopedBackendType(engine::core::ExecutionContext & execution)
        : config(const_cast<engine::core::BackendConfig &>(execution.config())), original(config.type) {
        if (emulate_cuda) { config.type = engine::core::BackendType::Cuda; }
    }
    void restore() { config.type = original; }
    ~ScopedBackendType() { restore(); }
};
}

namespace engine::modules {
class PrefillQueueRuntime : private ScopedBackendType, public QwenCausalDecodeRuntime {
public:
    PrefillQueueRuntime(core::ExecutionContext & execution, QwenCausalDecodeRuntimeConfig config,
                        QwenCausalDecodeRuntimeWeights weights)
        : ScopedBackendType(execution), QwenCausalDecodeRuntime(execution, std::move(config), std::move(weights)) {
        restore();
    }
};
}

// Reuse all synthetic weights, output modes, boundary cases, and continuation
// checks. No inherited test or production implementation is edited.
#define QwenCausalDecodeRuntime PrefillQueueRuntime
#define main r2t2_original_main
#define r2t2_test_compute_backend_graph r2t2_original_compute_backend_graph
#include "r2t2_prefill_test.cpp"
#undef r2t2_test_compute_backend_graph
#undef main
#undef QwenCausalDecodeRuntime

namespace {
struct Source {
    const void * pointer;
    size_t size;
    bool released = false;
};
struct Operation {
    ggml_backend_t backend;
    ggml_tensor * tensor = nullptr;
    ggml_cgraph * graph = nullptr;
    size_t source = 0;
    size_t offset = 0;
    std::vector<unsigned char> snapshot;
};
std::vector<Source> sources;
std::vector<Operation> pending;
std::vector<ggml_type> input_types;
std::vector<unsigned char> kv_trace;
bool watch_releases = false;
bool structural_only = false;
struct Batch { size_t graphs; size_t bytes; };
std::vector<Batch> structural_batches;
size_t structural_graphs = 0;
size_t pending_host_bytes = 0;
size_t peak_host_bytes = 0;
size_t queued_graphs = 0;
size_t prefill_submissions = 0;
size_t async_submissions = 0;
size_t async_drains = 0;
size_t multi_graph_drains = 0;
size_t max_graphs_per_drain = 0;
size_t uploaded_sources = 0;
size_t consumed_sources = 0;
size_t kv_snapshots = 0;
size_t fail_submission = 0;
size_t injected_failures = 0;
size_t graphs_pending_at_failure = 0;

void observe_release(void * pointer) noexcept {
    if (!watch_releases || !pointer || pointer == sources.data()) { return; }
    // MSVC's ordinary new/delete use the CRT heap. Mark the containing
    // allocation, including uploads borrowed from the middle of a vector.
    const auto begin = reinterpret_cast<uintptr_t>(pointer);
    const size_t size = _msize(pointer);
    for (auto & source : sources) {
        const auto address = reinterpret_cast<uintptr_t>(source.pointer);
        if (address >= begin && address - begin < size) { source.released = true; }
    }
}

void append_kv(ggml_cgraph * graph) {
    size_t found = 0;
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        const auto * node = ggml_graph_node(graph, i);
        if (node->op != GGML_OP_SET_ROWS) { continue; }
        // FastKVSetRowsModule attaches the original cache as src[2]. Capture
        // every allocated byte, including masked/padded rows, after execution.
        const auto * cache = node->src[2];
        require(cache != nullptr, "KV update lost its destination tensor");
        const size_t bytes = ggml_nbytes(cache);
        const auto * count = reinterpret_cast<const unsigned char *>(&bytes);
        kv_trace.insert(kv_trace.end(), count, count + sizeof(bytes));
        const size_t start = kv_trace.size();
        kv_trace.resize(start + bytes);
        ggml_backend_tensor_get(cache, kv_trace.data() + start, 0, bytes);
        ++found;
    }
    require(found == 4, "synthetic two-layer graph must update four KV tensors");
    ++kv_snapshots;
}

ggml_status execute_graph(ggml_backend_t backend, ggml_cgraph * graph,
                          ggml_backend_graph_plan_t plan = nullptr, const char * label = nullptr) {
    // Large-mask cases test scheduling/storage only. Graph construction and
    // original-pointer uploads remain real; outputs are discarded, not scored.
    if (structural_only) {
        ++structural_graphs;
        return GGML_STATUS_SUCCESS;
    }
    const auto status = core::r2t2_original_compute_backend_graph(backend, graph, plan, label);
    if (status == GGML_STATUS_SUCCESS) { append_kv(graph); }
    return status;
}

void discard_queue() {
    sources.clear();
    pending.clear();
    input_types.clear();
    queued_graphs = 0;
    pending_host_bytes = 0;
}

void drain(ggml_backend_t backend) {
    if (structural_only && !sources.empty()) {
        structural_batches.push_back({queued_graphs, pending_host_bytes});
    }
    if (queued_graphs) {
        ++async_drains;
        multi_graph_drains += queued_graphs > 1;
        max_graphs_per_drain = std::max(max_graphs_per_drain, queued_graphs);
    }
    for (const auto & operation : pending) {
        require(operation.backend == backend, "queued work changed backend before synchronization");
        if (operation.graph) {
            require(execute_graph(backend, operation.graph) == GGML_STATUS_SUCCESS,
                    "deferred CPU graph execution failed");
        } else {
            const auto & source = sources[operation.source];
            require(!source.released, "queued host allocation released before synchronization");
            require(std::memcmp(source.pointer, operation.snapshot.data(), source.size) == 0,
                    "queued host source overwritten before synchronization");
            // Consume the ORIGINAL address, never the observer's snapshot.
            ggml_backend_tensor_set(operation.tensor, source.pointer, operation.offset, source.size);
            ++consumed_sources;
        }
    }
    discard_queue();
}

bool observe_submission() {
    const ggml_type expected[] = {GGML_TYPE_F32, GGML_TYPE_I32, GGML_TYPE_F16, GGML_TYPE_I32};
    const bool prefill = input_types.size() >= 4 &&
        std::equal(std::begin(expected), std::end(expected), input_types.end() - 4);
    input_types.clear();
    if (!prefill) { return false; }
    ++prefill_submissions;
    if (fail_submission && --fail_submission == 0) {
        ++injected_failures;
        graphs_pending_at_failure = queued_graphs;
        return true;
    }
    return false;
}

void record_continuation(modules::PrefillQueueRuntime & runtime, std::ofstream & trace,
                         modules::QwenCausalDecodeStepResult result, int64_t steps) {
    record(trace, steps);
    record(trace, runtime.decode_current_end());
    record(trace, runtime.decode_valid_steps());
    record_result(trace, result, 0);
    for (int i = 0; i < 4; ++i) {
        const auto token = static_cast<int32_t>(
            std::max_element(result.logits.begin(), result.logits.end()) - result.logits.begin());
        record(trace, token);
        result = runtime.decode_token(token);
        require(runtime.decode_current_end() == steps + i + 1 &&
                runtime.decode_valid_steps() == steps + i + 1, "prefix reuse changed continuation position");
        record_result(trace, result, 0);
    }
}

void prefix_suite(std::ofstream & trace) {
    Fixture fixture;
    modules::PrefillQueueRuntime runtime(fixture.execution, config_for(0), fixture.weights);
    const auto all = pattern(257 * width, 1801, 0.3F);
    auto changed = all;
    changed[7] += 0.125F;
    const int64_t steps[] = {193, 257, 257, 257, 65, 64};
    const size_t expected_graphs[] = {4, 2, 1, 5, 1, 1};
    for (size_t i = 0; i < 6; ++i) {
        const auto & values = i < 3 ? all : changed;
        std::vector<float> input(values.begin(), values.begin() + steps[i] * width);
        work.clear();
        auto result = runtime.prefill_embeddings_into_cache(input, steps[i], 384, 64);
        require(pending.empty(), "prefill returned before draining its queued work");
        require(work.size() == expected_graphs[i], "prefix reuse executed the wrong number of blocks");
        check_work("prefix-" + std::to_string(i), static_cast<int64_t>(work.size()) * 64, 64, 0, true);
        record_continuation(runtime, trace, std::move(result), steps[i]);
    }
    std::cout << "PREFIX_OK cases=6 extension=2 identical=1 changed=5 shrink=1 boundary=1\n";
}

void require_same(const modules::QwenCausalDecodeStepResult & expected,
                  const modules::QwenCausalDecodeStepResult & actual) {
    for (const auto pair : {std::make_pair(&expected.logits, &actual.logits),
                            std::make_pair(&expected.hidden, &actual.hidden)}) {
        require(pair.first->size() == pair.second->size(), "retry output shape changed");
        require(pair.first->empty() || std::memcmp(pair.first->data(), pair.second->data(),
                pair.first->size() * sizeof(float)) == 0, "retry output differs after failed prefill");
    }
    require(expected.token_id == actual.token_id, "retry greedy token changed");
}

void failure_suite(bool require_queued_failure) {
    Fixture fixture;
    modules::PrefillQueueRuntime runtime(fixture.execution, config_for(0), fixture.weights);
    const auto original = pattern(193 * width, 2501, 0.3F);
    const auto expected = runtime.prefill_embeddings_into_cache(original, 193, 320, 64);
    std::vector<modules::QwenCausalDecodeStepResult> continuation;
    for (int32_t token : {3, 17, 29, 43}) { continuation.push_back(runtime.decode_token(token)); }
    auto changed = original;
    changed[0] += 0.25F;
    fail_submission = 2;
    const size_t failures_before = injected_failures;
    bool failed = false;
    try { runtime.prefill_embeddings_into_cache(changed, 193, 320, 64); }
    catch (const std::runtime_error &) { failed = true; }
    require(failed && injected_failures == failures_before + 1,
            "injected second prefill submission did not fail");
    // Check before retry/release, while an unsafe implementation still has
    // dangling borrowed inputs. Never consume such pointers in the test.
    require(pending.empty() && sources.empty(), "failed prefill returned with undrained borrowed storage");
    work.clear();
    const auto recovered = runtime.prefill_embeddings_into_cache(original, 193, 320, 64);
    require(work.size() == 4, "failed prefill retained a reusable embedding snapshot");
    require_same(expected, recovered);
    size_t i = 0;
    for (int32_t token : {3, 17, 29, 43}) { require_same(continuation[i++], runtime.decode_token(token)); }
    std::cout << "FAILURE_OBSERVED injected=1 retry_graphs=4 continuation_steps=4 queued_at_failure="
              << graphs_pending_at_failure << '\n';
    if (require_queued_failure) {
        require(graphs_pending_at_failure > 0,
                "failure injection occurred after queued graphs were already synchronized");
    }
}

void structural_staging_case(bool oversized) {
    // Production policy: 32 MiB of input/mask/position/last staging, with one
    // oversized block allowed. These dimensions avoid large KV allocations:
    // one decoder layer, 8192 cache slots, and no attention arithmetic.
    constexpr size_t cap_bytes = 32 * 1024 * 1024;
    constexpr int64_t capacity = 8192;
    const int64_t chunk = oversized ? 2048 : 512;
    const int64_t steps = oversized ? 4097 : 2049;
    const size_t blocks = static_cast<size_t>((steps + chunk - 1) / chunk);
    const size_t block_bytes = static_cast<size_t>(chunk) *
        (width * sizeof(float) + sizeof(int32_t) + capacity * sizeof(ggml_fp16_t)) + sizeof(int32_t);
    require(oversized ? block_bytes > cap_bytes :
            (3 * block_bytes <= cap_bytes && 4 * block_bytes > cap_bytes && blocks > 3),
            "structural fixture does not cross the staging boundary");
    Fixture fixture;
    auto config = config_for(2);
    config.decoder.stack.layers = 1;
    auto weights = fixture.weights;
    weights.stack.layers.resize(1);
    weights.lm_head.reset();
    modules::PrefillQueueRuntime runtime(fixture.execution, config, weights);
    structural_only = true;
    const auto input = pattern(static_cast<size_t>(steps * width), 3101, 0.3F);
    runtime.prefill_embeddings_into_cache(input, steps, capacity, chunk);
    require(pending.empty() && sources.empty(), "structural prefill returned before its final drain");
    require(runtime.decode_current_end() == steps && runtime.decode_valid_steps() == steps,
            "structural prefill changed cache positions");
    require(structural_graphs == blocks && consumed_sources == 4 * blocks && uploaded_sources == consumed_sources,
            "structural prefill did not consume every original host input and graph");
    std::cout << "STRUCTURAL_OBSERVED arithmetic=skipped case=" << (oversized ? "oversized" : "cap-crossing")
              << " blocks=" << blocks << " cap_bytes=" << cap_bytes << " block_bytes=" << block_bytes
              << " peak_retained_bytes=" << peak_host_bytes << " async_submissions=" << async_submissions
              << " batches=" << structural_batches.size() << " per_batch=";
    for (const auto & batch : structural_batches) { std::cout << batch.graphs << '/' << batch.bytes << ','; }
    std::cout << '\n';
    require(async_submissions == blocks, "structural prefill did not submit every graph asynchronously");
    require(peak_host_bytes <= std::max(cap_bytes, block_bytes),
            "retained host staging exceeded 32 MiB plus the single oversized-block allowance");
    for (const auto & batch : structural_batches) {
        require(batch.graphs > 0 && batch.bytes == batch.graphs * block_bytes,
                "staging byte accounting omitted input, mask, positions, or last index");
        require(oversized ? batch.graphs == 1 : batch.bytes <= cap_bytes,
                "staging drain crossed its permitted byte bound");
    }
    require(oversized ? structural_batches.size() == blocks :
            (structural_batches.size() >= 2 && multi_graph_drains > 0),
            "staging boundary did not split queued graph batches");
    structural_only = false;
}

void observer_controls() {
    Fixture fixture;
    auto * tensor = fixture.weights.final_norm.weight->tensor;
    for (int kind = 0; kind < 2; ++kind) {
        auto * values = new float[width]{};
        const size_t bytes = width * sizeof(float);
        sources.push_back({values, bytes});
        Operation upload{fixture.execution.backend(), tensor, nullptr, 0, 0, {}};
        const auto * raw = reinterpret_cast<const unsigned char *>(values);
        upload.snapshot.assign(raw, raw + bytes);
        pending.push_back(std::move(upload));
        if (kind == 0) { values[0] = 1.0F; }
        else { delete[] values; }
        bool rejected = false;
        try { drain(fixture.execution.backend()); }
        catch (const std::runtime_error &) { rejected = true; }
        require(rejected, kind == 0 ? "observer accepted overwritten host data" : "observer accepted released host data");
        discard_queue();
        if (kind == 0) { delete[] values; }
    }
    std::cout << "CONTROLS_OK overwritten=1 released=1\n";
}
}  // namespace

// Windows-only harness: MSVC's default operator new allocates on the CRT heap.
// Observe release without throwing, then preserve normal allocator behavior.
void operator delete(void * pointer) noexcept { observe_release(pointer); std::free(pointer); }
void operator delete[](void * pointer) noexcept { ::operator delete(pointer); }
void operator delete(void * pointer, size_t) noexcept { ::operator delete(pointer); }
void operator delete[](void * pointer, size_t) noexcept { ::operator delete(pointer); }

void r2t2_queue_tensor_set(ggml_backend_t backend, ggml_tensor * tensor,
                         const void * data, size_t offset, size_t size) {
    require(backend && tensor && data && size, "invalid queued input upload");
    require(offset <= ggml_nbytes(tensor) && size <= ggml_nbytes(tensor) - offset,
            "queued input upload exceeds tensor");
    const auto * bytes = static_cast<const unsigned char *>(data);
    sources.push_back({data, size});
    pending.push_back({backend, tensor, nullptr, sources.size() - 1, offset, {bytes, bytes + size}});
    pending_host_bytes += size;
    peak_host_bytes = std::max(peak_host_bytes, pending_host_bytes);
    input_types.push_back(tensor->type);
    ++uploaded_sources;
}

void r2t2_queue_sync_tensor_set(ggml_tensor * tensor, const void * data, size_t offset, size_t size) {
    require(pending.empty(), "synchronous upload interrupted queued prefill work");
    input_types.push_back(tensor->type);
    ggml_backend_tensor_set(tensor, data, offset, size);
}

void r2t2_queue_synchronize(ggml_backend_t backend) {
    drain(backend);
    ggml_backend_synchronize(backend);
}

ggml_status r2t2_queue_graph_compute_async(ggml_backend_t backend, ggml_cgraph * graph) {
    const size_t before = prefill_submissions;
    if (observe_submission()) { return GGML_STATUS_FAILED; }
    require(prefill_submissions == before + 1, "async compute did not follow four prefill uploads");
    pending.push_back({backend, nullptr, graph});
    ++queued_graphs;
    ++async_submissions;
    return GGML_STATUS_SUCCESS;
}

namespace engine::core {
ggml_status r2t2_queue_compute_backend_graph(ggml_backend_t backend, ggml_cgraph * graph,
                                           ggml_backend_graph_plan_t plan, const char * label) {
    require(queued_graphs == 0, "synchronous compute interrupted queued prefill graphs");
    const bool fail = observe_submission();
    drain(backend);
    return fail ? GGML_STATUS_FAILED : execute_graph(backend, graph, plan, label);
}
}  // namespace engine::core

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_prefill_queue_test.exe equivalence|work|cpu|failure|failure-control|cap|oversized|controls trace.bin");
        const std::string mode = argv[1];
        watch_releases = true;
        if (mode == "controls") { observer_controls(); }
        else if (mode == "failure" || mode == "failure-control") { failure_suite(mode == "failure"); }
        else if (mode == "cap" || mode == "oversized") { structural_staging_case(mode == "oversized"); }
        else {
            require(mode == "equivalence" || mode == "work" || mode == "cpu", "unknown test mode");
            emulate_cuda = mode != "cpu";
            char work_mode[] = "work";
            char * original_args[] = {argv[0], work_mode, argv[2]};
            const int result = r2t2_original_main(argc, original_args);
            require(result == 0, "inherited numerical or final-head suite failed");
            std::ofstream trace(argv[2], std::ios::binary | std::ios::app);
            prefix_suite(trace);
            require(work_failures == 0, "prefix reuse evaluated intermediate output heads");
            require(pending.empty() && sources.empty(), "suite returned with undrained prefill work");
            require(consumed_sources == uploaded_sources, "queued source consumption count differs");
            require(prefill_submissions > cases, "observer missed multi-block prefill submissions");
            require(kv_snapshots > prefill_submissions, "observer missed prefill or continuation KV updates");
            record(trace, static_cast<uint64_t>(kv_trace.size()));
            trace.write(reinterpret_cast<const char *>(kv_trace.data()), static_cast<std::streamsize>(kv_trace.size()));
            require(trace.good(), "could not write complete KV trace");
            std::cout << "QUEUE_OBSERVED prefill_submissions=" << prefill_submissions
                      << " async_submissions=" << async_submissions << " async_drains=" << async_drains
                      << " multi_graph_drains=" << multi_graph_drains << " max_graphs_per_drain=" << max_graphs_per_drain
                      << " stable_sources=" << consumed_sources << " kv_snapshots=" << kv_snapshots << '\n';
            if (mode == "work") {
                require(async_submissions == prefill_submissions && multi_graph_drains > 0 &&
                        async_drains < async_submissions,
                        "prefill still synchronizes each block instead of queueing multiple graphs");
            }
            if (mode == "cpu") { require(async_submissions == 0, "CPU fallback submitted async prefill graphs"); }
        }
        watch_releases = false;
        return 0;
    } catch (const std::exception & error) {
        watch_releases = false;
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
