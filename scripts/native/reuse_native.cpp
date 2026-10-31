// FP32 SEMamba adapter for NVIDIA RE-USE's original 30-layer checkpoint.
// Architecture: NVIDIA-NemotronLabs-RE-USE, revision
// 31f82f2efad1095034492f236e8dac9239ba713b (OpenMDW-1.1).
// All learned tensors remain in their original safetensors F32 representation.
#include "engine/framework/io/safetensors.h"
#include "ggml.h"
#include "ggml-alloc.h"
#include "ggml-backend.h"
#include "ggml-cpu.h"
#ifdef REUSE_USE_CUDA
#include "ggml-cuda.h"
#endif

#include <algorithm>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <map>
#include <memory>
#include <mutex>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <vector>

#define REUSE_API extern "C" __declspec(dllexport)

namespace {
thread_local std::string last_error;
using Tensor = ggml_tensor *;
constexpr int bins = 321;
constexpr size_t graph_capacity = 32768;

void require(bool condition, const std::string & message) {
    if (!condition) throw std::runtime_error(message);
}

// Validate the complete fixed architecture before invoking assertion-based GGML APIs.
std::map<std::string, std::vector<int64_t>> expected_shapes(bool realtime = false) {
    std::map<std::string, std::vector<int64_t>> result;
    auto conv = [&](const std::string & p, int in, int out, int t, int f) {
        result[p + ".weight"] = {out, in, t, f};
        result[p + ".bias"] = {out};
    };
    auto norm = [&](const std::string & p, int index = 1) {
        const std::string q = p + "." + std::to_string(index) + (realtime ? ".norm" : "");
        result[q + ".weight"] = {64};
        result[q + ".bias"] = {64};
        result[p + "." + std::to_string(index + 1) + ".weight"] = {64};
    };
    std::vector<std::string> codecs = {"dense_encoder", "mask_decoder", "phase_decoder"};
    if (realtime) for (int i = 0; i < 12; ++i) {
        codecs.push_back("mask_decoder_list." + std::to_string(i));
        codecs.push_back("phase_decoder_list." + std::to_string(i));
    }
    for (const std::string & p : codecs) {
        for (int i = 0; i < 4; ++i) {
            const std::string q = p + ".dense_block.dense_block." + std::to_string(i);
            conv(q + (realtime ? ".1" : ".0"), 64 * (i + 1), 64, 3, 3);
            norm(q, realtime ? 2 : 1);
        }
        if (p == "dense_encoder") continue;
        conv(p + ".up_conv1.0.conv", 64, 128, 1, 3);
        conv(p + ".up_conv2.0.conv", 64, realtime ? 64 : 256, 1, 3);
        norm(p + ".up_conv1");
        norm(p + ".up_conv2");
        if (realtime) {
            if (p.find("mask_decoder") == 0) conv(p + ".final_conv", 64, 1, 1, 1);
            else {
                conv(p + ".phase_conv_r", 64, 1, 1, 1);
                conv(p + ".phase_conv_i", 64, 1, 1, 1);
            }
        }
    }
    if (realtime) for (int i = 1; i <= 3; ++i) {
        const std::string p = "dense_encoder.dense_conv_1_" + std::to_string(i);
        conv(p + ".0", 2, 64, 3, 3);
        norm(p);
    }
    else {
        conv("dense_encoder.dense_conv_1.0", 2, 64, 1, 1);
        norm("dense_encoder.dense_conv_1");
    }
    conv("dense_encoder.dense_conv_2.0", 64, 64, 1, 3);
    norm("dense_encoder.dense_conv_2");
    conv("mask_decoder.final_conv", 64, 1, 1, 1);
    conv("phase_decoder.phase_conv_r", 64, 1, 1, 1);
    conv("phase_decoder.phase_conv_i", 64, 1, 1, 1);
    for (int i = 0; i < (realtime ? 12 : 30); ++i) {
        for (const std::string axis : {"time_mamba", "freq_mamba"}) {
            const std::string p = "TSMamba." + std::to_string(i) + "." + axis;
            result[p + ".output_proj.weight"] = {64, realtime && axis == "time_mamba" ? 64 : 128};
            result[p + ".output_proj.bias"] = {64};
            result[p + ".norm.weight"] = {64};
            result[p + ".norm.bias"] = {64};
            for (const std::string direction : {"forward_blocks", "backward_blocks"}) {
                if (realtime && axis == "time_mamba" && direction == "backward_blocks") continue;
                const std::string q = p + "." + direction;
                result[q + ".A_log"] = {256, 16};
                result[q + ".D"] = {256};
                result[q + ".conv1d.weight"] = {256, 1, 4};
                result[q + ".conv1d.bias"] = {256};
                result[q + ".dt_proj.weight"] = {256, 4};
                result[q + ".dt_proj.bias"] = {256};
                result[q + ".in_proj.weight"] = {512, 64};
                result[q + ".out_proj.weight"] = {64, 256};
                result[q + ".x_proj.weight"] = {36, 256};
            }
        }
    }
    return result;
}

struct Model {
    ggml_backend_t cpu = nullptr, device = nullptr;
    ggml_backend_sched_t scheduler = nullptr;
    ggml_context * weights_context = nullptr, * context = nullptr;
    ggml_backend_buffer_t weights_buffer = nullptr;
    std::unordered_map<std::string, Tensor> weights;
    ggml_cgraph * graph = nullptr;
    Tensor magnitude = nullptr, phase = nullptr, outputs[3] = {};
    std::vector<Tensor> zero_states;
    std::map<std::pair<int, int>, Tensor> reversals;
    std::map<int, Tensor> sequence_ids, states;
    int cached_frames = 0;
    std::mutex run_mutex;

    ~Model() {
        if (scheduler) ggml_backend_sched_free(scheduler);
        if (context) ggml_free(context);
        if (weights_buffer) ggml_backend_buffer_free(weights_buffer);
        if (weights_context) ggml_free(weights_context);
        if (device && device != cpu) ggml_backend_free(device);
        if (cpu) ggml_backend_free(cpu);
    }

    Tensor w(const std::string & name) { return weights.at(name); }

    void load(const char * path_utf8, const std::string & backend, int threads, bool realtime = false) {
        require(path_utf8 && *path_utf8, "A model path is required");
        const auto path = std::filesystem::u8path(path_utf8);
        std::ifstream file(path, std::ios::binary | std::ios::ate);
        require(bool(file), "Unable to open RE-USE checkpoint");
        const auto file_bytes = static_cast<uint64_t>(file.tellg());
        require(file_bytes >= 8 && file_bytes <= 128 * 1024 * 1024, "Invalid checkpoint size");
        file.seekg(0);
        uint64_t header_bytes = 0;
        file.read(reinterpret_cast<char *>(&header_bytes), 8);
        require(header_bytes <= 4 * 1024 * 1024 && header_bytes <= file_bytes - 8,
                "Invalid checkpoint header size");
        const auto index = engine::io::load_safetensors_index(path);
        const auto expected = expected_shapes(realtime);
        require(index.tensors.size() == expected.size(), realtime ? "Checkpoint must contain the complete real-time model"
                                                                : "Checkpoint must contain the complete 30-layer model");
        std::vector<std::pair<size_t, size_t>> ranges;
        size_t parameter_count = 0;
        for (const auto & entry : expected) {
            const auto found = index.tensors.find(entry.first);
            require(found != index.tensors.end(), "Missing tensor: " + entry.first);
            const auto & info = found->second;
            require(info.dtype == "F32" && info.shape == entry.second, "Invalid tensor type/shape: " + entry.first);
            size_t count = 1;
            for (int64_t dim : info.shape) count *= static_cast<size_t>(dim);
            require(info.data_end >= info.data_begin && info.data_end - info.data_begin == count * sizeof(float)
                    && info.data_end <= file_bytes - index.header_bytes, "Invalid tensor byte range: " + entry.first);
            parameter_count += count;
            ranges.emplace_back(info.data_begin, info.data_end);
        }
        std::sort(ranges.begin(), ranges.end());
        size_t cursor = 0;
        for (const auto & range : ranges) {
            require(range.first == cursor, "Checkpoint has overlapping or missing tensor data");
            cursor = range.second;
        }
        require(parameter_count == (realtime ? 13475815 : 9607747) && index.tensors.size() == (realtime ? 1318 : 1416)
                && cursor == file_bytes - index.header_bytes, "Unexpected checkpoint parameter count/data size");

        cpu = ggml_backend_cpu_init();
        require(cpu != nullptr, "CPU backend initialization failed");
        ggml_backend_cpu_set_n_threads(cpu, threads > 0 ? std::min(threads, 2) : 2);
        device = cpu;
        if (backend == "cuda") {
#ifdef REUSE_USE_CUDA
            device = ggml_backend_cuda_init(0);
            require(device != nullptr, "CUDA backend initialization failed");
#else
            throw std::runtime_error("This RE-USE library was built for CPU");
#endif
        }
        ggml_init_params params{expected.size() * ggml_tensor_overhead() + 1024, nullptr, true};
        weights_context = ggml_init(params);
        require(weights_context != nullptr, "Weight metadata allocation failed");
        for (const auto & entry : expected) {
            int64_t dims[4] = {1, 1, 1, 1};
            std::reverse_copy(entry.second.begin(), entry.second.end(), dims);
            Tensor tensor = ggml_new_tensor(weights_context, GGML_TYPE_F32, static_cast<int>(entry.second.size()), dims);
            ggml_set_name(tensor, entry.first.c_str());
            weights.emplace(entry.first, tensor);
        }
        weights_buffer = ggml_backend_alloc_ctx_tensors(weights_context, device);
        require(weights_buffer != nullptr, "Weight buffer allocation failed");
        ggml_backend_buffer_set_usage(weights_buffer, GGML_BACKEND_BUFFER_USAGE_WEIGHTS);
        for (const auto & entry : expected) {
            const auto & info = index.tensors.at(entry.first);
            std::vector<char> data(info.data_end - info.data_begin);
            file.seekg(static_cast<std::streamoff>(index.header_bytes + info.data_begin));
            file.read(data.data(), static_cast<std::streamsize>(data.size()));
            require(bool(file), "Checkpoint read failed: " + entry.first);
            ggml_backend_tensor_set(w(entry.first), data.data(), 0, data.size());
        }
        ggml_backend_t backends[2] = {device, cpu};
        scheduler = ggml_backend_sched_new(backends, nullptr, device == cpu ? 1 : 2, graph_capacity, false, true);
        require(scheduler != nullptr, "Graph scheduler allocation failed");
    }

    Tensor cont(Tensor x) { return ggml_is_contiguous(x) ? x : ggml_cont(context, x); }
    Tensor matrix(Tensor a, Tensor b) {
        Tensor y = ggml_mul_mat(context, a, b);
        ggml_mul_mat_set_prec(y, GGML_PREC_F32);
        return y;
    }
    Tensor linear(Tensor x, const std::string & p, bool bias = true) {
        const int64_t length = x->ne[1], sequences = x->ne[2];
        Tensor y = matrix(w(p + ".weight"), ggml_reshape_2d(context, cont(x), x->ne[0], length * sequences));
        if (bias && weights.count(p + ".bias")) y = ggml_add(context, y, w(p + ".bias"));
        return ggml_reshape_3d(context, y, y->ne[0], length, sequences);
    }
    Tensor conv(Tensor x, const std::string & p, int sf = 1, int st = 1, int pf = 0, int pt = 0, int dt = 1) {
        Tensor kernel = w(p + ".weight");
        Tensor cols = ggml_im2col(context, kernel, x, sf, st, pf, pt, 1, dt, true, GGML_TYPE_F32);
        Tensor y = matrix(ggml_reshape_2d(context, kernel, cols->ne[0], kernel->ne[3]),
                          ggml_reshape_2d(context, cols, cols->ne[0], cols->ne[1] * cols->ne[2]));
        y = ggml_add(context, y, w(p + ".bias"));
        y = ggml_reshape_3d(context, y, kernel->ne[3], cols->ne[1], cols->ne[2]);
        return cont(ggml_permute(context, y, 2, 0, 1, 3));
    }
    Tensor norm_prelu(Tensor x, const std::string & p) {
        const int64_t f = x->ne[0], t = x->ne[1], c = x->ne[2];
        Tensor y = ggml_norm(context, ggml_reshape_2d(context, cont(x), f * t, c), 1e-5f);
        y = ggml_mul(context, y, ggml_reshape_2d(context, w(p + ".1.weight"), 1, c));
        y = ggml_add(context, y, ggml_reshape_2d(context, w(p + ".1.bias"), 1, c));
        Tensor positive = ggml_relu(context, y);
        y = ggml_add(context, positive, ggml_mul(context, ggml_sub(context, y, positive),
                            ggml_reshape_2d(context, w(p + ".2.weight"), 1, c)));
        return ggml_reshape_3d(context, y, f, t, c);
    }
    Tensor dense(Tensor x, const std::string & p) {
        Tensor skip = x;
        for (int i = 0; i < 4; ++i) {
            const std::string q = p + ".dense_block." + std::to_string(i);
            x = norm_prelu(conv(skip, q + ".0", 1, 1, 1, 1 << i, 1 << i), q);
            if (i != 3) skip = ggml_concat(context, x, skip, 2);
        }
        return x;
    }
    Tensor reverse(Tensor x) {
        const int length = static_cast<int>(x->ne[1]), sequences = static_cast<int>(x->ne[2]);
        auto key = std::make_pair(length, sequences);
        Tensor & ids = reversals[key];
        if (!ids) {
            ids = ggml_new_tensor_2d(context, GGML_TYPE_I32, length, sequences);
            ggml_set_input(ids);
            // Cached constants must survive the graph allocator's last-use reuse.
            ggml_set_output(ids);
        }
        return ggml_get_rows(context, x, ids);
    }
    Tensor mamba(Tensor x, const std::string & p, Tensor history = nullptr, Tensor recurrent = nullptr,
                 std::vector<Tensor> * updates = nullptr) {
        const int64_t length = x->ne[1], sequences = x->ne[2];
        Tensor xz = linear(x, p + ".in_proj");
        Tensor u = cont(ggml_view_3d(context, xz, 256, length, sequences, xz->nb[1], xz->nb[2], 0));
        Tensor z = cont(ggml_view_3d(context, xz, 256, length, sequences, xz->nb[1], xz->nb[2], 256 * sizeof(float)));
        Tensor sx = cont(ggml_transpose(context, u));
        if (history) {
            sx = ggml_concat(context, history, sx, 0);
            Tensor next = cont(ggml_view_3d(context, sx, 3, 256, sequences, sx->nb[1], sx->nb[2], sizeof(float)));
            updates->push_back(ggml_cpy(context, next, history));
        } else sx = ggml_pad_ext(context, sx, 3, 0, 0, 0, 0, 0, 0, 0);
        u = ggml_ssm_conv(context, sx, ggml_reshape_2d(context, w(p + ".conv1d.weight"), 4, 256));
        u = ggml_silu(context, ggml_add(context, u, w(p + ".conv1d.bias")));
        Tensor projection = linear(u, p + ".x_proj");
        Tensor delta = cont(ggml_view_3d(context, projection, 4, length, sequences, projection->nb[1], projection->nb[2], 0));
        delta = linear(delta, p + ".dt_proj"); // GGML SSM_SCAN applies softplus.
        Tensor b = cont(ggml_view_3d(context, projection, 16, length, sequences, projection->nb[1], projection->nb[2], 4 * sizeof(float)));
        Tensor c = cont(ggml_view_3d(context, projection, 16, length, sequences, projection->nb[1], projection->nb[2], 20 * sizeof(float)));
        Tensor state = recurrent;
        Tensor & ids = sequence_ids[static_cast<int>(sequences)];
        if (!state) {
            Tensor & zero = states[static_cast<int>(sequences)];
            if (!zero) {
                zero = ggml_new_tensor_4d(context, GGML_TYPE_F32, 16, 1, 256, sequences);
                ggml_set_input(zero);
                ggml_set_output(zero);
                zero_states.push_back(zero);
            }
            state = zero;
        }
        if (!ids) {
            ids = ggml_new_tensor_1d(context, GGML_TYPE_I32, sequences);
            ggml_set_input(ids);
            // Keep constants intact between evaluations of the same graph.
            ggml_set_output(ids);
        }
        Tensor a = ggml_neg(context, ggml_exp(context, w(p + ".A_log")));
        Tensor scan = ggml_ssm_scan(context, state,
            ggml_reshape_4d(context, u, 1, 256, length, sequences), delta, a,
            ggml_reshape_4d(context, b, 16, 1, length, sequences),
            ggml_reshape_4d(context, c, 16, 1, length, sequences), ids);
        if (recurrent) {
            // GGML packs all output tokens first, followed by the final states.
            Tensor next = ggml_view_4d(context, scan, 16, 1, 256, sequences,
                                      recurrent->nb[1], recurrent->nb[2], recurrent->nb[3],
                                      static_cast<size_t>(ggml_nelements(u)) * sizeof(float));
            updates->push_back(ggml_cpy(context, next, recurrent));
        }
        // The existing gated CUDA scan synchronizes its shared storage. Request
        // its supported SCAN/VIEW/MUL/ADD/GLU/CPY pattern; CPU executes the same math.
        ggml_ssm_scan_set_fusion(scan, GGML_SSM_SCAN_FUSION_GATE);
        Tensor y = ggml_view_3d(context, scan, 256, length, sequences, 256 * sizeof(float), 256 * length * sizeof(float), 0);
        Tensor residual = ggml_add(context, y, ggml_mul(context, u, w(p + ".D")));
        Tensor gated = ggml_cpy(context, ggml_swiglu_split(context, z, residual), y);
        return linear(gated, p + ".out_proj");
    }
    Tensor bidirectional(Tensor x, const std::string & p) {
        Tensor fw = ggml_add(context, mamba(x, p + ".forward_blocks"), x);
        Tensor rx = reverse(x);
        Tensor bw = reverse(ggml_add(context, mamba(rx, p + ".backward_blocks"), rx));
        Tensor y = linear(ggml_concat(context, fw, bw, 0), p + ".output_proj");
        y = ggml_norm(context, y, 1e-5f);
        y = ggml_add(context, ggml_mul(context, y, w(p + ".norm.weight")), w(p + ".norm.bias"));
        return ggml_add(context, y, x);
    }
    Tensor frequency_shuffle(Tensor x, int ratio) {
        const int64_t f = x->ne[0], t = x->ne[1], channels = x->ne[2] / ratio;
        // Original channel order is [ratio, channels], then [channels,t,f,ratio].
        x = ggml_reshape_4d(context, x, f, t, channels, ratio);
        x = cont(ggml_permute(context, x, 1, 2, 3, 0)); // [ratio,f,t,channels]
        return ggml_reshape_3d(context, x, ratio * f, t, channels);
    }
    Tensor up(Tensor x, const std::string & p, int ratio) {
        return norm_prelu(frequency_shuffle(conv(x, p + ".0.conv", 1, 1, 1), ratio), p);
    }
    Tensor decoder(Tensor x, const std::string & p) {
        x = up(dense(x, p + ".dense_block"), p + ".up_conv1", 2);
        x = up(cont(ggml_transpose(context, x)), p + ".up_conv2", 4);
        return cont(ggml_transpose(context, x));
    }

    void initialize_constants() {
        for (const auto & entry : reversals) {
            const int length = entry.first.first, sequences = entry.first.second;
            std::vector<int32_t> values(length * sequences);
            for (int s = 0; s < sequences; ++s)
                for (int l = 0; l < length; ++l) values[s * length + l] = length - 1 - l;
            ggml_backend_tensor_set(entry.second, values.data(), 0, values.size() * sizeof(int32_t));
        }
        for (const auto & entry : sequence_ids) {
            std::vector<int32_t> values(entry.first);
            for (int i = 0; i < entry.first; ++i) values[i] = i;
            ggml_backend_tensor_set(entry.second, values.data(), 0, values.size() * sizeof(int32_t));
        }
    }

    void build_graph(int frames) {
        ggml_backend_sched_reset(scheduler);
        if (context) ggml_free(context);
        context = nullptr;
        cached_frames = 0;
        zero_states.clear(); reversals.clear(); sequence_ids.clear(); states.clear();
        ggml_init_params params{graph_capacity * ggml_tensor_overhead() * 2 + ggml_graph_overhead_custom(graph_capacity, false), nullptr, true};
        context = ggml_init(params);
        require(context != nullptr, "Graph metadata allocation failed");
        magnitude = ggml_new_tensor_3d(context, GGML_TYPE_F32, bins, frames, 1);
        phase = ggml_new_tensor_3d(context, GGML_TYPE_F32, bins, frames, 1);
        ggml_set_input(magnitude); ggml_set_input(phase);
        Tensor x = ggml_pad(context, ggml_concat(context, magnitude, phase, 2), 2, 2, 0, 0);
        x = norm_prelu(conv(x, "dense_encoder.dense_conv_1.0"), "dense_encoder.dense_conv_1");
        x = dense(x, "dense_encoder.dense_block");
        x = norm_prelu(conv(x, "dense_encoder.dense_conv_2.0", 2, 4), "dense_encoder.dense_conv_2");
        x = cont(ggml_permute(context, x, 2, 1, 0, 3)); // [channels,time,frequency]
        for (int i = 0; i < 30; ++i) {
            const std::string p = "TSMamba." + std::to_string(i);
            x = bidirectional(x, p + ".time_mamba");
            x = cont(ggml_permute(context, x, 0, 2, 1, 3)); // [channels,frequency,time]
            x = bidirectional(x, p + ".freq_mamba");
            x = cont(ggml_permute(context, x, 0, 2, 1, 3));
        }
        x = cont(ggml_permute(context, x, 2, 1, 0, 3)); // [frequency,time,channels]
        Tensor mag = conv(decoder(x, "mask_decoder"), "mask_decoder.final_conv");
        Tensor features = decoder(x, "phase_decoder");
        Tensor real = conv(features, "phase_decoder.phase_conv_r");
        Tensor imag = conv(features, "phase_decoder.phase_conv_i");
        graph = ggml_new_graph_custom(context, graph_capacity, false);
        Tensor decoded[3] = {mag, real, imag};
        for (int i = 0; i < 3; ++i) {
            require(decoded[i]->ne[0] >= bins && decoded[i]->ne[1] >= frames, "Decoder output shape is too small");
            outputs[i] = cont(ggml_view_2d(context, decoded[i], bins, frames, decoded[i]->nb[1], 0));
            ggml_set_output(outputs[i]);
            ggml_build_forward_expand(graph, outputs[i]);
        }
        require(ggml_backend_sched_alloc_graph(scheduler, graph), "Graph buffer allocation failed");
        initialize_constants();
        cached_frames = frames;
    }

    void run(const float * mag, const float * pha, int frames, float * out_mag, float * out_real, float * out_imag) {
        std::lock_guard<std::mutex> lock(run_mutex);
        if (cached_frames != frames) build_graph(frames);
        const size_t bytes = static_cast<size_t>(bins) * frames * sizeof(float);
        ggml_backend_tensor_set(magnitude, mag, 0, bytes);
        ggml_backend_tensor_set(phase, pha, 0, bytes);
        for (Tensor state : zero_states) {
            std::vector<float> zeros(static_cast<size_t>(ggml_nelements(state)), 0.0f);
            ggml_backend_tensor_set(state, zeros.data(), 0, zeros.size() * sizeof(float));
        }
        require(ggml_backend_sched_graph_compute(scheduler, graph) == GGML_STATUS_SUCCESS, "RE-USE graph execution failed");
        float * destinations[3] = {out_mag, out_real, out_imag};
        for (int i = 0; i < 3; ++i) ggml_backend_tensor_get(outputs[i], destinations[i], 0, bytes);
    }
};

// Research/noncommercial spectral streaming driver for Real-time_RE-USE,
// revision 4c0c186272ca853cd18d832fc5c28777b62f32d1. GGML expressions below
// implement causal convolutions, channel normalization and selective state-space
// recurrence; the original checkpoint is loaded without conversion.
struct RealtimeModel : Model {
    int exit_layers, lookahead;
    bool primed = false;
    std::vector<float> input_magnitude, input_phase;
    std::vector<Tensor> persistent, updates;

    RealtimeModel(int layers, int ahead) : exit_layers(layers), lookahead(ahead),
        input_magnitude(3 * bins, 0.0f), input_phase(3 * bins, 0.0f) {}

    Tensor persistent_state(int64_t n0, int64_t n1, int64_t n2, int64_t n3 = 1) {
        Tensor state = ggml_new_tensor_4d(context, GGML_TYPE_F32, n0, n1, n2, n3);
        ggml_set_input(state);
        ggml_set_output(state);
        ggml_backend_sched_set_tensor_backend(scheduler, state, device);
        persistent.push_back(state);
        return state;
    }

    Tensor channel_prelu(Tensor x, const std::string & p, int index = 1) {
        x = cont(ggml_permute(context, x, 1, 2, 0, 3)); // [channels,frequency,time]
        Tensor y = ggml_norm(context, x, 1e-5f);
        const std::string norm = p + "." + std::to_string(index) + ".norm";
        y = ggml_add(context, ggml_mul(context, y, w(norm + ".weight")), w(norm + ".bias"));
        Tensor positive = ggml_relu(context, y);
        y = ggml_add(context, positive, ggml_mul(context, ggml_sub(context, y, positive),
                    w(p + "." + std::to_string(index + 1) + ".weight")));
        return cont(ggml_permute(context, y, 2, 0, 1, 3)); // [frequency,time,channels]
    }

    Tensor with_history(Tensor x, int length) {
        Tensor history = persistent_state(x->ne[0], length, x->ne[2]);
        Tensor joined = ggml_concat(context, history, x, 1);
        Tensor next = cont(ggml_view_3d(context, joined, x->ne[0], length, x->ne[2],
                                      joined->nb[1], joined->nb[2], joined->nb[1]));
        updates.push_back(ggml_cpy(context, next, history));
        return joined;
    }

    Tensor causal_dense(Tensor x, const std::string & p) {
        Tensor skip = x;
        for (int i = 0; i < 4; ++i) {
            const std::string q = p + ".dense_block." + std::to_string(i);
            x = channel_prelu(conv(with_history(skip, 2 << i), q + ".1", 1, 1, 1, 0, 1 << i), q, 2);
            if (i != 3) skip = ggml_concat(context, x, skip, 2);
        }
        return x;
    }

    Tensor causal_decoder(Tensor x, const std::string & p) {
        x = causal_dense(x, p + ".dense_block");
        x = channel_prelu(frequency_shuffle(conv(x, p + ".up_conv1.0.conv", 1, 1, 1), 2), p + ".up_conv1");
        // The saved second kernel is [out,in,1,3]; exchange frequency/time
        // to apply its three taps causally along time, without temporal upsampling.
        x = conv(cont(ggml_transpose(context, with_history(x, 2))), p + ".up_conv2.0.conv");
        return channel_prelu(cont(ggml_transpose(context, x)), p + ".up_conv2");
    }

    void build_stream_graph() {
        ggml_init_params params{graph_capacity * ggml_tensor_overhead() * 2 + ggml_graph_overhead_custom(graph_capacity, false), nullptr, true};
        context = ggml_init(params);
        require(context != nullptr, "Streaming graph metadata allocation failed");
        magnitude = ggml_new_tensor_3d(context, GGML_TYPE_F32, bins, 3, 1);
        phase = ggml_new_tensor_3d(context, GGML_TYPE_F32, bins, 3, 1);
        ggml_set_input(magnitude); ggml_set_input(phase);
        Tensor x = ggml_pad(context, ggml_concat(context, magnitude, phase, 2), 2, 0, 0, 0);
        const std::string first = "dense_encoder.dense_conv_1_" + std::to_string(lookahead + 1);
        x = channel_prelu(conv(x, first + ".0", 1, 1, 1), first);
        x = causal_dense(x, "dense_encoder.dense_block");
        x = channel_prelu(conv(x, "dense_encoder.dense_conv_2.0", 2), "dense_encoder.dense_conv_2");
        x = cont(ggml_permute(context, x, 2, 1, 0, 3)); // [channels,time,frequency]
        for (int i = 0; i < exit_layers; ++i) {
            const std::string p = "TSMamba." + std::to_string(i) + ".time_mamba";
            Tensor convolution = persistent_state(3, 256, 161);
            Tensor state = persistent_state(16, 1, 256, 161);
            Tensor forward = ggml_add(context, mamba(x, p + ".forward_blocks", convolution, state, &updates), x);
            Tensor y = ggml_norm(context, linear(forward, p + ".output_proj"), 1e-5f);
            x = ggml_add(context, x, ggml_add(context, ggml_mul(context, y, w(p + ".norm.weight")), w(p + ".norm.bias")));
            x = cont(ggml_permute(context, x, 0, 2, 1, 3)); // [channels,frequency,time]
            x = bidirectional(x, "TSMamba." + std::to_string(i) + ".freq_mamba");
            x = cont(ggml_permute(context, x, 0, 2, 1, 3));
        }
        x = cont(ggml_permute(context, x, 2, 1, 0, 3));
        const std::string selected = std::to_string(exit_layers - 1);
        const std::string mag = "mask_decoder_list." + selected, pha = "phase_decoder_list." + selected;
        Tensor decoded[3];
        decoded[0] = conv(causal_decoder(x, mag), mag + ".final_conv");
        Tensor features = causal_decoder(x, pha);
        decoded[1] = conv(features, pha + ".phase_conv_r");
        decoded[2] = conv(features, pha + ".phase_conv_i");
        graph = ggml_new_graph_custom(context, graph_capacity, false);
        for (int i = 0; i < 3; ++i) {
            require(decoded[i]->ne[0] == 322 && decoded[i]->ne[1] == 1, "Invalid streaming decoder shape");
            outputs[i] = cont(ggml_view_1d(context, decoded[i], bins, 0));
            ggml_set_output(outputs[i]);
            ggml_build_forward_expand(graph, outputs[i]);
        }
        // Update persistent backend buffers only after every head has consumed
        // the old state. Marking leaves as inputs and outputs prevents reuse.
        for (Tensor update : updates) ggml_build_forward_expand(graph, update);
        // These leaves are constant between runs; retain their device copies.
        for (const auto & entry : reversals) ggml_backend_sched_set_tensor_backend(scheduler, entry.second, device);
        for (const auto & entry : sequence_ids) ggml_backend_sched_set_tensor_backend(scheduler, entry.second, device);
        for (Tensor state : zero_states) ggml_backend_sched_set_tensor_backend(scheduler, state, device);
        require(ggml_backend_sched_alloc_graph(scheduler, graph), "Streaming graph buffer allocation failed");
        initialize_constants();
        clear_stream();
    }

    void clear_stream() {
        for (const auto & group : {zero_states, persistent}) for (Tensor state : group) {
            std::vector<float> zeros(static_cast<size_t>(ggml_nelements(state)), 0.0f);
            ggml_backend_tensor_set(state, zeros.data(), 0, zeros.size() * sizeof(float));
        }
        std::fill(input_magnitude.begin(), input_magnitude.end(), 0.0f);
        std::fill(input_phase.begin(), input_phase.end(), 0.0f);
        primed = false;
    }

    void reset() {
        std::lock_guard<std::mutex> lock(run_mutex);
        clear_stream();
    }

    void run_stream(const float * mag, const float * pha, int frames, float * out_mag, float * out_real, float * out_imag) {
        std::lock_guard<std::mutex> lock(run_mutex);
        require(frames == (primed ? 1 : lookahead + 1), "Real-time RE-USE requires K=lookahead+1 priming frames, then exactly one frame");
        if (primed) {
            std::move(input_magnitude.begin() + bins, input_magnitude.end(), input_magnitude.begin());
            std::move(input_phase.begin() + bins, input_phase.end(), input_phase.begin());
        }
        const int offset = primed ? 2 * bins : (2 - lookahead) * bins;
        std::copy_n(mag, frames * bins, input_magnitude.begin() + offset);
        std::copy_n(pha, frames * bins, input_phase.begin() + offset);
        ggml_backend_tensor_set(magnitude, input_magnitude.data(), 0, input_magnitude.size() * sizeof(float));
        ggml_backend_tensor_set(phase, input_phase.data(), 0, input_phase.size() * sizeof(float));
        require(ggml_backend_sched_graph_compute(scheduler, graph) == GGML_STATUS_SUCCESS, "Real-time RE-USE graph execution failed");
        float * destinations[3] = {out_mag, out_real, out_imag};
        for (int i = 0; i < 3; ++i) ggml_backend_tensor_get(outputs[i], destinations[i], 0, bins * sizeof(float));
        primed = true;
    }
};
}

REUSE_API void * reuse_create(const char * path_utf8, const char * backend, int n_threads) {
    try {
        last_error.clear();
        require(backend != nullptr, "A backend is required");
        const std::string selected(backend);
        require(selected == "cpu" || selected == "cuda", "Unsupported RE-USE backend");
        auto model = std::make_unique<Model>();
        model->load(path_utf8, selected, n_threads);
        return model.release();
    } catch (const std::exception & error) { last_error = error.what(); }
      catch (...) { last_error = "Unknown RE-USE initialization error"; }
    return nullptr;
}

REUSE_API int reuse_run(void * handle, const float * magnitude, const float * phase, int frames,
                        float * out_magnitude, float * out_real, float * out_imag) {
    try {
        last_error.clear();
        require(handle && magnitude && phase && out_magnitude && out_real && out_imag, "RE-USE arguments must be non-null");
        require(frames > 0 && frames <= 801, "RE-USE frame count must be between 1 and 801");
        static_cast<Model *>(handle)->run(magnitude, phase, frames, out_magnitude, out_real, out_imag);
        return 0;
    } catch (const std::exception & error) { last_error = error.what(); }
      catch (...) { last_error = "Unknown RE-USE execution error"; }
    return 1;
}

REUSE_API void reuse_free(void * handle) {
    try { delete static_cast<Model *>(handle); }
    catch (...) { last_error = "RE-USE cleanup failed"; }
}

REUSE_API const char * reuse_last_error(void) { return last_error.c_str(); }

REUSE_API void * reuse_rt_create(const char * path_utf8, const char * backend, int n_threads, int exit_layers, int lookahead) {
    try {
        last_error.clear();
        require(backend != nullptr, "A backend is required");
        const std::string selected(backend);
        require(selected == "cpu" || selected == "cuda", "Unsupported RE-USE backend");
        require(exit_layers >= 3 && exit_layers <= 12, "Real-time RE-USE exit layers must be between 3 and 12");
        require(lookahead >= 0 && lookahead <= 2, "Real-time RE-USE lookahead must be between 0 and 2");
        auto model = std::make_unique<RealtimeModel>(exit_layers, lookahead);
        model->load(path_utf8, selected, n_threads, true);
#ifdef REUSE_USE_CUDA
        if (model->device != model->cpu) {
            const auto registry = ggml_backend_dev_backend_reg(ggml_backend_get_device(model->device));
            const auto use_fp32 = reinterpret_cast<void (*)(ggml_backend_t)>(
                ggml_backend_reg_get_proc_address(registry, "ggml_backend_cuda_use_fp32"));
            require(use_fp32 != nullptr, "CUDA backend lacks RE-USE FP32 math support");
            use_fp32(model->device);
        }
#endif
        model->build_stream_graph();
        return model.release();
    } catch (const std::exception & error) { last_error = error.what(); }
      catch (...) { last_error = "Unknown real-time RE-USE initialization error"; }
    return nullptr;
}
REUSE_API int reuse_rt_run(void * handle, const float * magnitude, const float * phase, int frames,
                           float * out_magnitude, float * out_real, float * out_imag) {
    try {
        last_error.clear();
        require(handle && magnitude && phase && out_magnitude && out_real && out_imag, "RE-USE arguments must be non-null");
        static_cast<RealtimeModel *>(handle)->run_stream(magnitude, phase, frames, out_magnitude, out_real, out_imag);
        return 0;
    } catch (const std::exception & error) { last_error = error.what(); }
      catch (...) { last_error = "Unknown real-time RE-USE execution error"; }
    return 1;
}
REUSE_API int reuse_rt_reset(void * handle) {
    try {
        last_error.clear();
        require(handle != nullptr, "A real-time RE-USE handle is required");
        static_cast<RealtimeModel *>(handle)->reset();
        return 0;
    } catch (const std::exception & error) { last_error = error.what(); }
      catch (...) { last_error = "Unknown real-time RE-USE reset error"; }
    return 1;
}
REUSE_API void reuse_rt_free(void * handle) {
    try { delete static_cast<RealtimeModel *>(handle); }
    catch (...) { last_error = "Real-time RE-USE cleanup failed"; }
}
REUSE_API const char * reuse_rt_last_error(void) { return last_error.c_str(); }
