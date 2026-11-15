
#define main r2t2_original_main
#define r2t2_test_compute_backend_graph r2t2_original_compute_backend_graph
#include "r2t2_prefill_test.cpp"
#undef r2t2_test_compute_backend_graph
#undef main

namespace {
struct Upload {
    ggml_backend_t backend;
    ggml_tensor * tensor;
    const void * source;
    size_t offset;
    std::vector<unsigned char> snapshot;
};
struct ObservedUpload { ggml_type type; bool queued; };
std::vector<Upload> pending;
std::vector<ObservedUpload> since_compute;
size_t prefill_graphs = 0;
size_t queued_batches = 0;
size_t queued_uploads = 0;
size_t queued_bytes = 0;
size_t stable_sources = 0;
size_t explicit_drains = 0;

void drain(ggml_backend_t backend) {
    for (const auto & upload : pending) {
        require(upload.backend == backend, "queued input uses a different compute backend");
        require(std::memcmp(upload.source, upload.snapshot.data(), upload.snapshot.size()) == 0,
                "queued input source changed before compute consumed it");
        ++stable_sources;

        ggml_backend_tensor_set(upload.tensor, upload.source, upload.offset, upload.snapshot.size());
    }
    pending.clear();
}
}

void r2t2_deferred_tensor_set(ggml_backend_t backend, ggml_tensor * tensor,
                            const void * data, size_t offset, size_t size) {
    require(backend && tensor && data && size, "invalid deferred input upload");
    require(offset <= ggml_nbytes(tensor) && size <= ggml_nbytes(tensor) - offset,
            "deferred input upload exceeds tensor");
    const auto * bytes = static_cast<const unsigned char *>(data);
    pending.push_back({backend, tensor, data, offset, {bytes, bytes + size}});
    since_compute.push_back({tensor->type, true});
    ++queued_uploads;
    queued_bytes += size;
}

void r2t2_observed_tensor_set(ggml_tensor * tensor, const void * data,
                            size_t offset, size_t size) {
    require(pending.empty(), "synchronous upload interrupted a queued input batch");
    since_compute.push_back({tensor->type, false});
    ggml_backend_tensor_set(tensor, data, offset, size);
}

void r2t2_observed_synchronize(ggml_backend_t backend) {
    if (!pending.empty()) { ++explicit_drains; }
    drain(backend);
    ggml_backend_synchronize(backend);
}

namespace engine::core {
ggml_status r2t2_deferred_compute_backend_graph(
    ggml_backend_t backend, ggml_cgraph * graph, ggml_backend_graph_plan_t plan, const char * label) {


    if (since_compute.size() >= 4) {
        const auto start = since_compute.size() - 4;
        const ggml_type expected[] = {GGML_TYPE_F32, GGML_TYPE_I32, GGML_TYPE_F16, GGML_TYPE_I32};
        bool prefill = true;
        bool queued = true;
        for (size_t i = 0; i < 4; ++i) {
            prefill = prefill && since_compute[start + i].type == expected[i];
            queued = queued && since_compute[start + i].queued;
        }
        if (prefill) {
            ++prefill_graphs;
            if (queued) {
                require(pending.size() == 4, "prefill compute did not receive one complete queued input batch");
                ++queued_batches;
            }
        }
    }
    drain(backend);
    since_compute.clear();
    const auto status = r2t2_original_compute_backend_graph(backend, graph, plan, label);
    require(pending.empty(), "compute returned with undrained input uploads");
    return status;
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: r2t2_prefill_async_test.exe equivalence|work trace.bin");
        const std::string mode = argv[1];
        require(mode == "equivalence" || mode == "work", "unknown test mode");


        char equivalence[] = "equivalence";
        char * original_args[] = {argv[0], equivalence, argv[2]};
        const int result = r2t2_original_main(argc, original_args);
        if (result) { return result; }
        require(pending.empty(), "suite returned with undrained input uploads");
        require(prefill_graphs > cases, "input observer missed multi-block prefill work");
        require(stable_sources == queued_uploads, "some queued sources were not checked at consumption");
        require(explicit_drains == 0, "input uploads synchronized before the compute boundary");
        std::cout << "UPLOAD_OBSERVED prefill_graphs=" << prefill_graphs
                  << " queued_batches=" << queued_batches << " queued_uploads=" << queued_uploads
                  << " queued_bytes=" << queued_bytes << " stable_sources=" << stable_sources
                  << " explicit_drains=" << explicit_drains << '\n';
        if (mode == "work") {
            require(queued_batches == prefill_graphs && queued_uploads == 4 * prefill_graphs,
                    "prefill inputs did not queue all four uploads until compute");
        }
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
