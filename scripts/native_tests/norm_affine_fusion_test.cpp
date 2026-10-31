// Real GGML graphs, synthetic finite inputs, and actual CUDA kernel-node counts.
// Capture wraps only graph_compute_async: allocation, uploads, warmup and reads
// stay outside it. No engine hooks, source assertions, models or timing gates.
#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml-cpu.h"
#include "ggml-cuda.h"
#include <cuda_runtime_api.h>

#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
constexpr float EPS = 1.0e-5F;
constexpr double ATOL = 3.0e-5;
constexpr double RTOL = 3.0e-5;

void require(bool condition, const std::string & message) {
    if (!condition) { throw std::runtime_error(message); }
}
void cuda_check(cudaError_t status, const char * operation) {
    require(status == cudaSuccess, std::string(operation) + ": " + cudaGetErrorString(status));
}

enum class Affine { channel, row, full };
enum class Residual { none, external, input, broadcast, strided, half };
enum class Observe { none, norm, mul, affine, norm_branch, mul_branch, affine_branch };
struct Case {
    std::string name;
    int64_t cols, rows, batch2 = 1, batch3 = 1;
    Affine affine = Affine::channel;
    Residual residual = Residual::none;
    Observe observe = Observe::none;
    bool constant = false;
    bool eligible = true;
};

std::vector<Case> cases() {
    std::vector<Case> result;
    for (const int cols : {1, 7, 64, 97, 1025}) {
        for (const auto affine : {Affine::channel, Affine::row}) {
            for (const auto residual : {Residual::none, Residual::external}) {
                Case item;
                item.name = std::string(affine == Affine::channel ? "channel_" : "row_") +
                    std::to_string(cols) + (residual == Residual::none ? "_affine" : "_residual");
                item.cols = cols; item.rows = 3; item.affine = affine; item.residual = residual;
                result.push_back(item);
            }
        }
    }
    for (const auto affine : {Affine::channel, Affine::row}) {
        for (const auto residual : {Residual::none, Residual::external, Residual::input}) {
            Case item;
            item.name = std::string(affine == Affine::channel ? "channel_rank4_" : "row_rank4_") +
                (residual == Residual::none ? "affine" : residual == Residual::input ? "input_residual" : "residual");
            item.cols = 31; item.rows = 3; item.batch2 = 2; item.batch3 = 2;
            item.affine = affine; item.residual = residual;
            result.push_back(item);
        }
        Case constant;
        constant.name = affine == Affine::channel ? "channel_constant_rows" : "row_constant_rows";
        constant.cols = 33; constant.rows = 3; constant.affine = affine; constant.constant = true;
        constant.residual = Residual::external;
        result.push_back(constant);
    }
    for (const auto observe : {Observe::norm, Observe::mul, Observe::affine,
                               Observe::norm_branch, Observe::mul_branch, Observe::affine_branch}) {
        Case item;
        item.name = observe == Observe::norm ? "retained_norm_output" :
            observe == Observe::mul ? "retained_mul_output" :
            observe == Observe::affine ? "retained_affine_output" :
            observe == Observe::norm_branch ? "norm_other_consumer" :
            observe == Observe::mul_branch ? "mul_other_consumer" : "affine_other_consumer_before_residual";
        item.cols = 97; item.rows = 3; item.observe = observe; item.eligible = false;
        item.residual = Residual::input;
        result.push_back(item);
    }
    for (const auto residual : {Residual::none, Residual::external}) {
        Case item;
        item.name = residual == Residual::none ? "full_affine_layout_fallback" : "full_affine_residual_fallback";
        item.cols = 97; item.rows = 3; item.batch2 = 2; item.batch3 = 2;
        item.affine = Affine::full; item.residual = residual; item.eligible = false;
        result.push_back(item);
    }
    for (const auto residual : {Residual::broadcast, Residual::strided, Residual::half}) {
        Case item;
        item.name = residual == Residual::broadcast ? "broadcast_residual_fallback" :
            residual == Residual::strided ? "strided_residual_fallback" : "f16_residual_fallback";
        item.cols = 97; item.rows = 3; item.batch2 = 2; item.batch3 = 2;
        item.residual = residual; item.eligible = false;
        result.push_back(item);
    }
    return result;
}

struct Graph {
    ggml_context * ctx = ggml_init({48 * ggml_tensor_overhead() +
                                   ggml_graph_overhead_custom(48, false), nullptr, true});
    ggml_gallocr_t allocator = nullptr;
    ~Graph() { ggml_gallocr_free(allocator); ggml_free(ctx); }
};

std::vector<float> read(ggml_tensor * tensor) {
    require(tensor->type == GGML_TYPE_F32 && ggml_is_contiguous(tensor), "read requires contiguous F32");
    std::vector<float> result(ggml_nelements(tensor));
    ggml_backend_tensor_get(tensor, result.data(), 0, result.size() * sizeof(float));
    return result;
}

// Round the host fixture too, so the independent reference sees stored F16 data.
void upload(ggml_tensor * tensor, std::vector<float> & values) {
    if (tensor->type == GGML_TYPE_F16) {
        std::vector<ggml_fp16_t> half(values.size());
        for (size_t i = 0; i < values.size(); ++i) {
            half[i] = ggml_fp32_to_fp16(values[i]);
            values[i] = ggml_fp16_to_fp32(half[i]);
        }
        ggml_backend_tensor_set(tensor, half.data(), 0, half.size() * sizeof(ggml_fp16_t));
    } else {
        ggml_backend_tensor_set(tensor, values.data(), 0, values.size() * sizeof(float));
    }
}

struct Launches { int kernels = 0, memcpy_nodes = 0, other_nodes = 0; };
Launches capture(ggml_backend_t backend, ggml_cgraph * graph) {
    auto stream = static_cast<cudaStream_t>(ggml_backend_cuda_get_stream(backend));
    require(stream != nullptr, "CUDA backend stream unavailable");
    cuda_check(cudaStreamSynchronize(stream), "pre-capture synchronize");
    cuda_check(cudaStreamBeginCapture(stream, cudaStreamCaptureModeRelaxed), "cudaStreamBeginCapture");
    const auto computed = ggml_backend_graph_compute_async(backend, graph);
    cudaGraph_t captured = nullptr;
    cuda_check(cudaStreamEndCapture(stream, &captured), "cudaStreamEndCapture");
    require(computed == GGML_STATUS_SUCCESS, "captured ggml graph compute failed");
    require(captured != nullptr, "external CUDA capture returned no graph");
    cudaGraphExec_t executable = nullptr;
    try {
        size_t count = 0;
        cuda_check(cudaGraphGetNodes(captured, nullptr, &count), "cudaGraphGetNodes(size)");
        std::vector<cudaGraphNode_t> nodes(count);
        cuda_check(cudaGraphGetNodes(captured, nodes.data(), &count), "cudaGraphGetNodes");
        Launches launches;
        for (const auto node : nodes) {
            cudaGraphNodeType type;
            cuda_check(cudaGraphNodeGetType(node, &type), "cudaGraphNodeGetType");
            require(type != cudaGraphNodeTypeGraph, "nested capture hides kernels; disable backend CUDA graphs");
            launches.kernels += type == cudaGraphNodeTypeKernel;
            launches.memcpy_nodes += type == cudaGraphNodeTypeMemcpy;
            launches.other_nodes += type != cudaGraphNodeTypeKernel && type != cudaGraphNodeTypeMemcpy;
        }
        require(launches.kernels > 0, "external capture recorded no actual CUDA kernels");
        cuda_check(cudaGraphInstantiateWithFlags(&executable, captured, 0), "cudaGraphInstantiateWithFlags");
        cuda_check(cudaGraphLaunch(executable, stream), "cudaGraphLaunch");
        cuda_check(cudaStreamSynchronize(stream), "captured execution synchronize");
        cuda_check(cudaGraphExecDestroy(executable), "cudaGraphExecDestroy");
        executable = nullptr;
        cuda_check(cudaGraphDestroy(captured), "cudaGraphDestroy");
        return launches;
    } catch (...) {
        if (executable) { cudaGraphExecDestroy(executable); }
        cudaGraphDestroy(captured);
        throw;
    }
}

struct Result {
    std::map<std::string, std::vector<float>> outputs;
    Launches launches;
    bool numerical = true, retained_inputs = true, refreshed_output = false, work = true;
    size_t residual_bytes_checked = 0;
    double reference_max_abs = 0;
};

Result run_case(ggml_backend_t backend, const std::string & kind, const Case & item, bool unfused, bool work) {
    Graph owner;
    require(owner.ctx != nullptr, "GGML metadata allocation failed");
    const size_t n = size_t(item.cols * item.rows * item.batch2 * item.batch3);
    auto * input = ggml_new_tensor_4d(owner.ctx, GGML_TYPE_F32, item.cols, item.rows, item.batch2, item.batch3);
    ggml_set_input(input);
    ggml_set_output(input); // The retained input, including self-residuals, must survive all computes.
    const int64_t affine_cols = item.affine == Affine::row ? 1 : item.cols;
    const int64_t affine_rows = item.affine == Affine::channel ? 1 : item.rows;
    auto * gamma = ggml_new_tensor_2d(owner.ctx, GGML_TYPE_F32, affine_cols, affine_rows);
    auto * beta = ggml_new_tensor_2d(owner.ctx, GGML_TYPE_F32, affine_cols, affine_rows);
    ggml_set_input(gamma); ggml_set_input(beta);
    // Updated parameters must not alias an output and accidentally refresh it
    // by upload alone. This also models retained, immutable model weights.
    ggml_set_output(gamma); ggml_set_output(beta);
    auto * norm = ggml_norm(owner.ctx, input, EPS);
    auto * mul = ggml_mul(owner.ctx, norm, gamma);
    auto * affine = ggml_add(owner.ctx, mul, beta);
    ggml_tensor * residual = nullptr;
    ggml_tensor * residual_storage = nullptr;
    if (item.residual == Residual::input) {
        residual = input;
    } else if (item.residual != Residual::none) {
        const auto type = item.residual == Residual::half ? GGML_TYPE_F16 : GGML_TYPE_F32;
        const auto cols = item.residual == Residual::broadcast ? 1 : item.cols;
        const auto pad = item.residual == Residual::strided ? 3 : 0;
        residual_storage = ggml_new_tensor_4d(owner.ctx, type, cols + pad, item.rows, item.batch2, item.batch3);
        ggml_set_input(residual_storage);
        ggml_set_output(residual_storage);
        residual = pad ? ggml_view_4d(owner.ctx, residual_storage, cols, item.rows, item.batch2, item.batch3,
                                     residual_storage->nb[1], residual_storage->nb[2], residual_storage->nb[3], 0)
                       : residual_storage;
    }
    // Existing CUDA binbcast supports F16 as src0 with an F32 result. Its
    // F32-src0/F16-src1 direction is not implemented in this pinned baseline.
    auto * output = item.residual == Residual::half ? ggml_add_cast(owner.ctx, residual, affine, GGML_TYPE_F32) :
                   residual ? ggml_add(owner.ctx, affine, residual) : affine;
    ggml_set_output(output);
    std::map<std::string, ggml_tensor *> observed{{"output", output}};
    if (item.observe == Observe::norm || item.observe == Observe::mul || item.observe == Observe::affine) {
        auto * tensor = item.observe == Observe::norm ? norm : item.observe == Observe::mul ? mul : affine;
        ggml_set_output(tensor);
        observed[item.observe == Observe::norm ? "norm" : item.observe == Observe::mul ? "mul" : "affine"] = tensor;
    }
    ggml_tensor * branch = nullptr;
    if (item.observe == Observe::norm_branch || item.observe == Observe::mul_branch || item.observe == Observe::affine_branch) {
        branch = ggml_sqr(owner.ctx, item.observe == Observe::norm_branch ? norm :
                                     item.observe == Observe::mul_branch ? mul : affine);
        ggml_set_output(branch);
        observed["branch"] = branch;
    }
    auto * graph = ggml_new_graph_custom(owner.ctx, 48, false);
    if (item.observe == Observe::affine_branch) { ggml_build_forward_expand(graph, branch); }
    ggml_build_forward_expand(graph, output);
    if (branch && item.observe != Observe::affine_branch) { ggml_build_forward_expand(graph, branch); }
    for (int i = 0; i < ggml_graph_n_nodes(graph); ++i) {
        require(ggml_backend_supports_op(backend, ggml_graph_node(graph, i)), "backend does not support fixture graph");
    }
    owner.allocator = ggml_gallocr_new(ggml_backend_get_default_buffer_type(backend));
    require(owner.allocator != nullptr && ggml_gallocr_alloc_graph(owner.allocator, graph), "GGML graph allocation failed");

    std::vector<float> x(n), scale(affine_cols * affine_rows), bias(scale.size()), r;
    for (size_t i = 0; i < n; ++i) {
        x[i] = item.constant ? (0.375F + float((i / item.cols) % 3) * 0.125F) :
            float(int((i * 37 + (i / item.cols) * 11) % 257) - 128) / 32.0F;
    }
    for (size_t i = 0; i < scale.size(); ++i) {
        scale[i] = 0.5F + float(int((i * 7) % 17) - 8) / 8.0F;
        bias[i] = float(int((i * 5) % 19) - 9) / 16.0F;
    }
    upload(input, x); upload(gamma, scale); upload(beta, bias);
    std::vector<uint8_t> residual_before;
    if (residual_storage) {
        r.resize(ggml_nelements(residual_storage));
        for (size_t i = 0; i < r.size(); ++i) { r[i] = float(int((i * 13) % 43) - 21) / 16.0F; }
        upload(residual_storage, r);
        residual_before.resize(ggml_nbytes(residual_storage));
        ggml_backend_tensor_get(residual_storage, residual_before.data(), 0, residual_before.size());
    }
    // Repeated warmup also detects a fused implementation that destructively
    // normalizes a retained input before using its original residual value.
    for (int i = 0; i < 2; ++i) {
        require(ggml_backend_graph_compute_async(backend, graph) == GGML_STATUS_SUCCESS, "warmup compute failed");
        ggml_backend_synchronize(backend);
    }
    const auto warmup_output = read(output);
    // Capture must compute fresh values, not leave a warmup result behind.
    // Constant rows remain constant, but their changed bias changes the result.
    for (size_t i = 0; i < x.size(); ++i) {
        x[i] = item.constant ? x[i] + 0.25F : -0.5F * x[i] + float(i % 5) * 0.25F + 0.125F;
    }
    for (size_t i = 0; i < scale.size(); ++i) {
        scale[i] = -0.75F * scale[i] + 0.125F;
        bias[i] += 0.375F + float(i % 3) / 16.0F;
    }
    upload(input, x); upload(gamma, scale); upload(beta, bias);
    Result result;
    if (kind == "cuda") { result.launches = capture(backend, graph); }
    else {
        require(ggml_backend_graph_compute_async(backend, graph) == GGML_STATUS_SUCCESS, "CPU compute failed");
        ggml_backend_synchronize(backend);
    }
    for (const auto & [name, tensor] : observed) { result.outputs[name] = read(tensor); }
    for (size_t i = 0; i < warmup_output.size(); ++i) {
        result.refreshed_output = result.refreshed_output ||
            std::abs(double(result.outputs.at("output")[i]) - warmup_output[i]) > ATOL + RTOL * std::abs(warmup_output[i]);
    }
    result.retained_inputs = read(input) == x && read(gamma) == scale && read(beta) == bias;
    if (residual_storage) {
        std::vector<uint8_t> residual_after(residual_before.size());
        ggml_backend_tensor_get(residual_storage, residual_after.data(), 0, residual_after.size());
        result.residual_bytes_checked = residual_after.size();
        result.retained_inputs = result.retained_inputs && residual_after == residual_before;
    }
    for (size_t row = 0; row < n / item.cols; ++row) {
        double mean = 0, variance = 0;
        for (int64_t col = 0; col < item.cols; ++col) { mean += x[row * item.cols + col]; }
        mean /= item.cols;
        for (int64_t col = 0; col < item.cols; ++col) {
            const double centered = x[row * item.cols + col] - mean;
            variance += centered * centered;
        }
        const double inv_std = 1.0 / std::sqrt(variance / item.cols + double(EPS));
        for (int64_t col = 0; col < item.cols; ++col) {
            const size_t index = row * item.cols + col;
            const size_t a = item.affine == Affine::channel ? col : item.affine == Affine::row ?
                             row % item.rows : (row % item.rows) * item.cols + col;
            const double normalized = (x[index] - mean) * inv_std;
            const double multiplied = normalized * scale[a];
            const double affine_value = multiplied + bias[a];
            double expected = affine_value;
            if (item.residual == Residual::input) { expected += x[index]; }
            else if (residual_storage) {
                const size_t stride = size_t(residual_storage->ne[0]);
                expected += r[row * stride + (item.residual == Residual::broadcast ? 0 : col)];
            }
            for (const auto & [name, actual] : result.outputs) {
                const double reference = name == "output" ? expected : name == "norm" ? normalized :
                    name == "mul" ? multiplied : name == "affine" ? affine_value :
                    item.observe == Observe::norm_branch ? normalized * normalized :
                    item.observe == Observe::mul_branch ? multiplied * multiplied : affine_value * affine_value;
                const double error = std::abs(double(actual[index]) - reference);
                result.reference_max_abs = std::max(result.reference_max_abs, error);
                result.numerical = result.numerical && std::isfinite(actual[index]) &&
                    error <= ATOL + RTOL * std::abs(reference);
            }
        }
    }
    if (kind == "cuda") {
        if (unfused) {
            const int expected = 3 + (residual != nullptr) + (branch != nullptr);
            result.work = result.launches.kernels == expected;
        } else if (item.eligible && work) {
            result.work = result.launches.kernels == 1;
        } else if (!item.eligible) {
            // Excluded affine layouts and broadcast/strided/F16 residuals use
            // existing paths. Observable intermediates cannot vanish, while a
            // three-op prefix may still fuse when its affine output is retained.
            result.work = result.launches.kernels >= 2;
        }
        result.work = result.work && result.launches.memcpy_nodes == 0;
    }
    return result;
}

void vector_json(std::ostream & out, const std::vector<float> & values) {
    out << '[';
    for (size_t i = 0; i < values.size(); ++i) { if (i) { out << ','; } out << values[i]; }
    out << ']';
}

int suite(ggml_backend_t backend, const std::string & kind, bool unfused, bool work, std::ostream & json) {
    json << std::setprecision(9) << "{\"backend\":\"" << kind << "\",\"unfused\":"
         << (unfused ? "true" : "false") << ",\"cases\":[";
    int total = 0, failed = 0, eligible = 0;
    for (const auto & item : cases()) {
        // F16 mixed arithmetic has a CUDA-only fixture; F32 cases also run on CPU.
        if (kind == "cpu" && item.residual == Residual::half) { continue; }
        const auto result = run_case(backend, kind, item, unfused, work);
        const bool pass = result.numerical && result.retained_inputs && result.refreshed_output && result.work;
        if (total++) { json << ','; }
        eligible += item.eligible;
        failed += !pass;
        json << "{\"case\":\"" << item.name << "\",\"eligible\":" << (item.eligible ? "true" : "false")
             << ",\"shape\":[" << item.cols << ',' << item.rows << ',' << item.batch2 << ',' << item.batch3 << ']'
             << ",\"kernels\":" << result.launches.kernels << ",\"memcpy_nodes\":" << result.launches.memcpy_nodes
             << ",\"other_nodes\":" << result.launches.other_nodes
             << ",\"reference_max_abs\":" << result.reference_max_abs
             << ",\"numerical\":" << (result.numerical ? "true" : "false")
             << ",\"retained_inputs\":" << (result.retained_inputs ? "true" : "false")
             << ",\"residual_bytes_checked\":" << result.residual_bytes_checked
             << ",\"refreshed_output\":" << (result.refreshed_output ? "true" : "false")
             << ",\"work\":" << (result.work ? "true" : "false") << ",\"outputs\":{";
        int output = 0;
        for (const auto & [name, values] : result.outputs) {
            if (output++) { json << ','; }
            json << '"' << name << "\":"; vector_json(json, values);
        }
        json << "}}";
        if (!pass) {
            std::cout << (!result.work ? "WORK_RED " : "NUMERIC_RED ") << item.name
                      << " kernels=" << result.launches.kernels
                      << " expected=" << (unfused ? "unfused chain length" : item.eligible ? "1" : ">=2")
                      << " numerical=" << result.numerical << " retained_inputs=" << result.retained_inputs
                      << " refreshed_output=" << result.refreshed_output << '\n';
        }
    }
    json << "],\"total\":" << total << ",\"eligible\":" << eligible << ",\"failures\":" << failed << "}\n";
    std::cout << "NORM_AFFINE backend=" << kind << " unfused=" << unfused << " cases=" << total
              << " eligible=" << eligible << " failures=" << failed << '\n';
    return failed ? 1 : 0;
}
} // namespace

int main(int argc, char ** argv) {
    ggml_backend_t backend = nullptr;
    try {
        std::string kind = "cuda", output, mode = "all";
        bool unfused = false;
        for (int i = 1; i < argc; ++i) {
            const std::string option = argv[i];
            require(i + 1 < argc, "missing option value");
            const std::string value = argv[++i];
            if (option == "--backend") { kind = value; }
            else if (option == "--output") { output = value; }
            else if (option == "--mode") { mode = value; }
            else if (option == "--unfused") { unfused = value == "1"; }
            else { throw std::runtime_error("unknown option: " + option); }
        }
        require((kind == "cpu" || kind == "cuda") && (mode == "all" || mode == "numerics") && !output.empty(),
                "usage: norm_affine_fusion_test --backend cpu|cuda --mode all|numerics --unfused 0|1 --output file.json");
        require(std::getenv("GGML_CUDA_DISABLE_GRAPHS") != nullptr, "set GGML_CUDA_DISABLE_GRAPHS=1 before backend initialization");
        const char * disabled = std::getenv("GGML_CUDA_DISABLE_FUSION");
        require(disabled != nullptr && (std::atoi(disabled) != 0) == unfused, "fusion env must match --unfused before initialization");
        backend = kind == "cpu" ? ggml_backend_cpu_init() : ggml_backend_cuda_init(0);
        require(backend != nullptr, "requested backend unavailable");
        if (kind == "cpu") { ggml_backend_cpu_set_n_threads(backend, 1); }
        std::ostringstream json;
        const int result = suite(backend, kind, unfused, mode == "all", json);
        ggml_backend_free(backend); backend = nullptr;
        std::ofstream file(output);
        file << json.str();
        require(file.good(), "result JSON write failed");
        return result;
    } catch (const std::exception & error) {
        if (backend) { ggml_backend_free(backend); }
        std::cerr << "norm affine test failed: " << error.what() << '\n';
        return 2;
    }
}
