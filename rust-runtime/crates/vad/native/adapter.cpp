#include "kaldi-native-fbank/csrc/online-feature.h"
#define ORT_API_MANUAL_INIT
#include "onnxruntime_cxx_api.h"

#include <algorithm>
#include <cstdio>
#include <stdexcept>
#include <string>

namespace {
// Copy the exception before unwinding. Rust consumes it on this thread before
// the next native call. No allocator ownership crosses the language boundary.
template <class F>
const char* checked(F action) noexcept {
    try {
        action();
        return nullptr;
    } catch (const std::exception& e) {
        static thread_local char message[1024];
        std::snprintf(message, sizeof(message), "%s", e.what());
        return message;
    } catch (...) {
        return "unknown native VAD error";
    }
}

struct Model {
    Ort::Env env{ORT_LOGGING_LEVEL_FATAL, "fireredvad"};
    Ort::Session session{nullptr};
    std::string output_name;
    Ort::MemoryInfo memory = Ort::MemoryInfo::CreateCpu(OrtDeviceAllocator, OrtMemTypeDefault);

    Model(const uint8_t* graph, size_t bytes) {
        Ort::SessionOptions options;
        options.SetInterOpNumThreads(1).SetIntraOpNumThreads(1).DisableCpuMemArena().SetLogSeverityLevel(4);
        session = Ort::Session(env, graph, bytes, options);
        output_name = session.GetOutputNameAllocated(0, Ort::AllocatorWithDefaultOptions{}).get();
    }
};
}

extern "C" {
const char* lw_vad_open(const uint8_t* bytes, size_t len, Model** out) noexcept {
    return checked([&] {
        // Windows can resolve an older system copy if the SDK DLL was omitted
        // from the application directory. Check before calling any C++ wrapper.
        static const auto* api = OrtGetApiBase()->GetApi(ORT_API_VERSION);
        if (!api) throw std::runtime_error("FireRedVAD requires ONNX Runtime 1.24.1 next to the executable");
        static const bool initialized = (Ort::InitApi(api), true);
        (void)initialized;
        *out = new Model(bytes, len);
    });
}
void lw_vad_close(Model* model) noexcept { delete model; }

const char* lw_vad_run(Model* model, float* features, size_t frames,
                       void (*receive)(void*, const float*, size_t), void* dest) noexcept {
    return checked([&] {
        const int64_t shape[] = {1, static_cast<int64_t>(frames), 80};
        auto input = Ort::Value::CreateTensor<float>(model->memory, features, frames * 80, shape, 3);
        const char* input_name = "feat";
        const char* output_name = model->output_name.c_str();
        auto output = model->session.Run(Ort::RunOptions{nullptr}, &input_name, &input, 1, &output_name, 1);
        auto info = output[0].GetTensorTypeAndShapeInfo();
        if (info.GetElementType() != ONNX_TENSOR_ELEMENT_DATA_TYPE_FLOAT)
            throw std::runtime_error("expected float32 FireRedVAD posterior tensor");
        receive(dest, output[0].GetTensorData<float>(), info.GetElementCount());
    });
}

const char* lw_fbank_open(knf::OnlineFbank** out) noexcept {
    return checked([&] {
        knf::FbankOptions options;
        options.frame_opts.samp_freq = 16000;
        options.frame_opts.frame_length_ms = 25;
        options.frame_opts.frame_shift_ms = 10;
        options.frame_opts.dither = 0;
        options.frame_opts.snip_edges = true;
        options.mel_opts.num_bins = 80;
        options.mel_opts.debug_mel = false;
        *out = new knf::OnlineFbank(options);
    });
}
void lw_fbank_close(knf::OnlineFbank* bank) noexcept { delete bank; }

const char* lw_fbank_append(knf::OnlineFbank* bank, const float* pcm, int32_t len,
                            float* dest, size_t new_frames) noexcept {
    return checked([&] {
        const int32_t first = bank->NumFramesReady();
        bank->AcceptWaveform(16000, pcm, len);
        const int32_t end = bank->NumFramesReady();
        if (static_cast<size_t>(end - first) != new_frames)
            throw std::runtime_error("unexpected Kaldi frame count");
        for (int32_t i = first; i < end; ++i) {
            dest = std::copy_n(bank->GetFrame(i), 80, dest);
        }
    });
}
}
