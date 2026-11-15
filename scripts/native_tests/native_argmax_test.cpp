
#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml-cpu.h"
#ifdef TEST_CUDA
#include "ggml-cuda.h"
#endif

#include <cmath>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <limits>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
void require(bool condition, const char * message) {
    if (!condition) { throw std::runtime_error(message); }
}
struct Case { std::string name; std::vector<float> values; };

std::vector<Case> cases_for(int n) {
    std::vector<Case> cases;
    auto add = [&](const char * name, float fill, int a, int b, float peak) {
        cases.push_back({name, std::vector<float>(n, fill)});
        if (a >= 0) { cases.back().values[a] = peak; }
        if (b >= 0) { cases.back().values[b] = peak; }
    };
    add("all_zero", 0.0F, -1, -1, 0.0F);
    add("all_negative", -7.0F, -1, -1, 0.0F);
    add("same_warp_tie_3_11", -4.0F, 3, 11, 2.0F);
    add("cross_warp_tie_3_35", -4.0F, 3, 35, 2.0F);
    add("first_last_tie", -4.0F, 0, n - 1, 2.0F);
    if (n > 1035) { add("strided_tie_3_1035", -4.0F, 3, 1035, 2.0F); }
    if (n > 1024) { add("stride_boundary_tie_3_1024", -4.0F, 3, 1024, 2.0F); }
    add("near_tie_high_index_wins", -4.0F, 3, n - 1, 1.0F);
    cases.back().values[n - 1] = std::nextafter(1.0F, std::numeric_limits<float>::infinity());
    add("near_tie_low_index_wins", -4.0F, 3, n - 1, 1.0F);
    cases.back().values[3] = std::nextafter(1.0F, std::numeric_limits<float>::infinity());
    add("unique_first", -4.0F, 0, -1, 2.0F);
    add("unique_last", -4.0F, n - 1, -1, 2.0F);
    return cases;
}

struct Graph {
    ggml_context * context = ggml_init({8 * ggml_tensor_overhead() +
                                      ggml_graph_overhead_custom(16, false), nullptr, true});
    ggml_backend_buffer_t buffer = nullptr;
    ~Graph() { ggml_backend_buffer_free(buffer); ggml_free(context); }
};

int suite(ggml_backend_t backend, const std::string & kind, std::ostream & json) {
    int failures = 0, total = 0;
    json << "{\"backend\":\"" << kind << "\",\"results\":[";
    for (const int n : {97, 1024, 1025, 151936}) {
        const auto cases = cases_for(n);
        std::vector<float> values;
        for (const auto & item : cases) { values.insert(values.end(), item.values.begin(), item.values.end()); }
        Graph owner;
        require(owner.context != nullptr, "ggml context allocation failed");
        auto * input = ggml_new_tensor_2d(owner.context, GGML_TYPE_F32, n, cases.size());
        auto * first = ggml_argmax(owner.context, input);
        auto * legacy = ggml_argmax(owner.context, input);
        require(first->op_params[0] == 0 && legacy->op_params[0] == 0, "argmax default flag changed");
        first->op_params[0] = 1;
        require(ggml_backend_supports_op(backend, first) && ggml_backend_supports_op(backend, legacy),
                "selected backend does not support argmax");
        auto * graph = ggml_new_graph_custom(owner.context, 16, false);
        ggml_build_forward_expand(graph, first);
        ggml_build_forward_expand(graph, legacy);
        owner.buffer = ggml_backend_alloc_ctx_tensors(owner.context, backend);
        require(owner.buffer != nullptr, "ggml backend tensor allocation failed");
        ggml_backend_tensor_set(input, values.data(), 0, values.size() * sizeof(float));
        require(ggml_backend_graph_compute(backend, graph) == GGML_STATUS_SUCCESS, "ggml graph compute failed");
        std::vector<int32_t> actual_first(cases.size()), actual_legacy(cases.size());
        ggml_backend_tensor_get(first, actual_first.data(), 0, actual_first.size() * sizeof(int32_t));
        ggml_backend_tensor_get(legacy, actual_legacy.data(), 0, actual_legacy.size() * sizeof(int32_t));
        for (size_t row = 0; row < cases.size(); ++row) {
            const auto & item = cases[row];
            int32_t expected_first = 0, expected_last = 0;
            for (int32_t i = 1; i < n; ++i) {
                if (item.values[i] > item.values[expected_first]) { expected_first = i; }
                if (item.values[i] >= item.values[expected_last]) { expected_last = i; }
            }
            const bool assert_legacy_index = kind == "cpu" || expected_first == expected_last;
            const bool legacy_maximum = actual_legacy[row] >= 0 && actual_legacy[row] < n &&
                item.values[actual_legacy[row]] == item.values[expected_first];
            const bool pass = actual_first[row] == expected_first && legacy_maximum &&
                (!assert_legacy_index || actual_legacy[row] == expected_last);
            failures += !pass;
            if (total++) { json << ','; }
            json << "{\"case\":\"" << item.name << "\",\"size\":" << n
                 << ",\"first_token\":" << actual_first[row] << ",\"expected_first\":" << expected_first
                 << ",\"default_token\":" << actual_legacy[row] << ",\"expected_last\":" << expected_last
                 << ",\"default_index_asserted\":" << (assert_legacy_index ? "true" : "false")
                 << ",\"pass\":" << (pass ? "true" : "false") << '}';
        }
    }
    json << "],\"cases\":" << total << ",\"failures\":" << failures << "}\n";
    return failures ? 1 : 0;
}
}

int main(int argc, char ** argv) {
    ggml_backend_t backend = nullptr;
    try {
        std::string kind = "cpu", output;
        for (int i = 1; i < argc; ++i) {
            const std::string option = argv[i];
            require(i + 1 < argc && (option == "--backend" || option == "--output"),
                    "usage: native_argmax_test --backend cpu|cuda [--output results.json]");
            (option == "--backend" ? kind : output) = argv[++i];
        }
        require(kind == "cpu" || kind == "cuda", "backend must be cpu or cuda");
        if (kind == "cpu") { backend = ggml_backend_cpu_init(); }
#ifdef TEST_CUDA
        else { backend = ggml_backend_cuda_init(0); }
#endif
        require(backend != nullptr, "requested backend unavailable in this test build");
        if (kind == "cpu") { ggml_backend_cpu_set_n_threads(backend, 1); }
        std::ostringstream json;
        const int result = suite(backend, kind, json);
        ggml_backend_free(backend);
        backend = nullptr;
        if (!output.empty()) {
            std::ofstream file(output);
            file << json.str();
            require(file.good(), "test JSON output write failed");
        }
        std::cout << json.str();
        return result;
    } catch (const std::exception & error) {
        if (backend) { ggml_backend_free(backend); }
        std::cerr << "native argmax test failed: " << error.what() << '\n';
        return 2;
    }
}
