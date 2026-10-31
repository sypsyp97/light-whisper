#include "engine/models/qwen3_asr/frontend_whisper.h"
#include "engine/framework/audio/fft.h"
#include "engine/framework/audio/wav_reader.h"

#include <windows.h>
#include <bcrypt.h>
#pragma comment(lib, "bcrypt.lib")

#include <algorithm>
#include <array>
#include <chrono>
#include <complex>
#include <cstdint>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace engine::audio {
void r2t2_test_real_fft_forward(
    const TensorShape &, const TensorStrideBytes &, const TensorStrideBytes &,
    size_t, const float *, std::complex<float> *, float, size_t);
}

namespace {
namespace qwen = engine::models::qwen3_asr;
using Clock = std::chrono::steady_clock;
using engine::runtime::AudioBuffer;
constexpr size_t sample_rate = 16000, preview_step = 3200, startup = 1920;
constexpr size_t window_limit = 12 * sample_rate, duration = 36 * sample_rate;
bool count_work = false;
size_t fft_frames = 0, mel_frames = 0, mel_cells = 0;

void crypto_check(NTSTATUS status) {
    if (status < 0) throw std::runtime_error("Windows SHA256 operation failed");
}

// Platform SHA256 runs outside every timed extract; no feature arithmetic here.
class Sha256 {
public:
    Sha256() {
        crypto_check(BCryptOpenAlgorithmProvider(&algorithm_, BCRYPT_SHA256_ALGORITHM, nullptr, 0));
        DWORD length = 0, written = 0;
        const auto status = BCryptGetProperty(algorithm_, BCRYPT_OBJECT_LENGTH,
            reinterpret_cast<PUCHAR>(&length), sizeof(length), &written, 0);
        if (status < 0) {
            BCryptCloseAlgorithmProvider(algorithm_, 0);
            crypto_check(status);
        }
        object_.resize(length);
    }
    ~Sha256() { BCryptCloseAlgorithmProvider(algorithm_, 0); }
    std::string digest(const void * metadata, size_t metadata_bytes,
                       const void * values, size_t value_bytes,
                       const void * mask = nullptr, size_t mask_bytes = 0) {
        BCRYPT_HASH_HANDLE raw = nullptr;
        crypto_check(BCryptCreateHash(algorithm_, &raw, object_.data(),
            static_cast<ULONG>(object_.size()), nullptr, 0, 0));
        const std::unique_ptr<void, decltype(&BCryptDestroyHash)> hash(raw, BCryptDestroyHash);
        auto add = [&](const void * data, size_t bytes) {
            if (bytes != 0) crypto_check(BCryptHashData(hash.get(),
                static_cast<PUCHAR>(const_cast<void *>(data)), static_cast<ULONG>(bytes), 0));
        };
        add(metadata, metadata_bytes); add(values, value_bytes); add(mask, mask_bytes);
        std::array<unsigned char, 32> output{};
        crypto_check(BCryptFinishHash(hash.get(), output.data(), static_cast<ULONG>(output.size()), 0));
        std::ostringstream hex;
        hex << std::hex << std::setfill('0');
        for (auto byte : output) hex << std::setw(2) << static_cast<unsigned>(byte);
        return hex.str();
    }
private:
    BCRYPT_ALG_HANDLE algorithm_ = nullptr;
    std::vector<unsigned char> object_;
};

struct Observation {
    std::string scenario, label, pcm_sha256, feature_sha256;
    size_t captured, input_samples, fft, mel, cells;
    int64_t nanoseconds, frames, tokens;
};

AudioBuffer crop(const std::vector<float> & pcm, size_t begin, size_t end) {
    return {static_cast<int>(sample_rate), 1, std::vector<float>(pcm.begin() + begin, pcm.begin() + end)};
}

std::vector<Observation> replay(const std::vector<float> & pcm, const std::vector<float> & public_pcm, Sha256 & sha) {
    std::vector<Observation> observations;
    auto assets = std::make_shared<qwen::Qwen3ASRAssets>();
    const auto & config = assets->config.frontend;
    if (config.sample_rate != sample_rate || config.n_fft != 400 ||
            config.hop_length != 160 || config.feature_size != 128) {
        throw std::runtime_error("Qwen asset frontend defaults differ from the measured product configuration");
    }
    auto extract = [&](const qwen::Qwen3ASRWhisperFrontend & frontend, const AudioBuffer & audio,
                       const std::string & scenario, const std::string & label, size_t captured) {
        const auto pcm_hash = sha.digest(nullptr, 0, audio.samples.data(), audio.samples.size() * sizeof(float));
        fft_frames = mel_frames = mel_cells = 0;
        count_work = true;
        const auto before = Clock::now();
        const auto features = frontend.extract(audio);
        const auto after = Clock::now();
        count_work = false;
        const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(after - before).count();
        const std::array<int64_t, 5> metadata{
            features.mel_bins, features.frames, features.encoder_tokens,
            static_cast<int64_t>(features.values.size()), static_cast<int64_t>(features.attention_mask.size()),
        };
        observations.push_back({scenario, label, pcm_hash,
            sha.digest(metadata.data(), sizeof(metadata), features.values.data(), features.values.size() * sizeof(float),
                       features.attention_mask.data(), features.attention_mask.size() * sizeof(int32_t)),
            captured, audio.samples.size(), fft_frames, mel_frames, mel_cells,
            elapsed, features.frames, features.encoder_tokens});
    };

    // The first resident decode primes this owner outside measured extraction.
    qwen::Qwen3ASRWhisperFrontend resident(assets);
    const AudioBuffer resident_pcm{static_cast<int>(sample_rate), 1, public_pcm};
    resident.extract(resident_pcm);
    for (size_t repeat = 0; repeat < 20; ++repeat)
        extract(resident, resident_pcm, "identical_resident", "repeat-" + std::to_string(repeat), public_pcm.size());

    qwen::Qwen3ASRWhisperFrontend growing(assets);
    size_t last_growing = 0;
    for (size_t end = startup; end <= window_limit; end += preview_step) {
        extract(growing, crop(pcm, 0, end), "growing_preview", "capture-" + std::to_string(end), end);
        last_growing = end;
    }
    // Continue the same 120-ms startup/200-ms steady cadence. Every subsequent
    // preview contains exactly the latest 12 s, rather than the R2T2 trim policy.
    qwen::Qwen3ASRWhisperFrontend sliding(assets);
    sliding.extract(crop(pcm, 0, last_growing));
    size_t last_preview = last_growing;
    for (size_t end = last_growing + preview_step; end <= pcm.size(); end += preview_step) {
        extract(sliding, crop(pcm, end - window_limit, end), "sliding_preview_12s",
                "capture-" + std::to_string(end), end);
        last_preview = end;
    }
    // Finish receives the entire captured 36 s, including the final sub-interval
    // tail absent from the last preview. The next input rewrites that same size.
    qwen::Qwen3ASRWhisperFrontend final(assets);
    final.extract(crop(pcm, last_preview - window_limit, last_preview));
    extract(final, crop(pcm, 0, pcm.size()), "final_and_rewrite", "final-whole-clip", pcm.size());
    auto rewritten = pcm;
    std::rotate(rewritten.begin(), rewritten.begin() + preview_step, rewritten.end());
    if (rewritten == pcm) throw std::runtime_error("public PCM rotation did not rewrite the fixture");
    extract(final, {static_cast<int>(sample_rate), 1, std::move(rewritten)},
            "final_and_rewrite", "rewrite-rotate-200ms", pcm.size());
    return observations;
}
}  // namespace

namespace engine::audio {
AudioTensor r2t2_test_counted_sparse(
    const std::vector<float> & magnitude, int64_t batch, int64_t bins,
    int64_t stft_frames, int64_t output_frames, const SparseMelFilterbank & filterbank) {
    auto out = MelFilterbank().compute_custom_sparse_from_magnitude(
        magnitude, batch, bins, stft_frames, output_frames, filterbank);
    if (count_work) {
        mel_frames += static_cast<size_t>(batch * output_frames);
        mel_cells += out.values.size();
    }
    return out;
}
void real_fft_forward(
    const TensorShape & shape, const TensorStrideBytes & input_strides,
    const TensorStrideBytes & output_strides, size_t axis, const float * data_in,
    std::complex<float> * data_out, float scale, size_t threads) {
    if (count_work) {
        size_t frames = 1;
        for (size_t dim = 0; dim < shape.size(); ++dim) if (dim != axis) frames *= shape[dim];
        fft_frames += frames;
    }
    // Bound both real implementations to one FFT worker for this CPU benchmark.
    r2t2_test_real_fft_forward(shape, input_strides, output_strides, axis, data_in, data_out, scale, 1);
}
}  // namespace engine::audio

int main(int argc, char ** argv) {
    try {
        if (argc != 4) throw std::runtime_error("usage: qwen_frontend_bench WAV warmups measured_iterations");
        const int warmups = std::stoi(argv[2]), iterations = std::stoi(argv[3]);
        if (warmups < 1 || iterations < 1) throw std::runtime_error("warmups and iterations must be positive");
        const auto wav = engine::audio::read_wav_f32(std::filesystem::path(argv[1]));
        if (wav.sample_rate != sample_rate || wav.channels != 1 || wav.samples.empty())
            throw std::runtime_error("expected nonempty mono 16 kHz public WAV");
        std::vector<float> repeated(duration);
        for (size_t i = 0; i < repeated.size(); ++i) repeated[i] = wav.samples[i % wav.samples.size()];
        Sha256 sha;
        const auto repeated_hash = sha.digest(nullptr, 0, repeated.data(), repeated.size() * sizeof(float));
        for (int iteration = -warmups; iteration < iterations; ++iteration) {
            const auto observations = replay(repeated, wav.samples, sha);
            std::cout << "{\"iteration\":" << iteration << ",\"warmup\":" << (iteration < 0 ? "true" : "false")
                      << ",\"source_samples\":" << wav.samples.size() << ",\"derived_samples\":" << repeated.size()
                      << ",\"derived_pcm_sha256\":\"" << repeated_hash << "\",\"extracts\":[";
            for (size_t i = 0; i < observations.size(); ++i) {
                if (i) std::cout << ',';
                const auto & row = observations[i];
                std::cout << "{\"scenario\":\"" << row.scenario << "\",\"label\":\"" << row.label
                          << "\",\"captured_samples\":" << row.captured << ",\"input_samples\":" << row.input_samples
                          << ",\"ns\":" << row.nanoseconds << ",\"fft_frames\":" << row.fft
                          << ",\"mel_frames\":" << row.mel << ",\"mel_cells\":" << row.cells
                          << ",\"frames\":" << row.frames << ",\"encoder_tokens\":" << row.tokens
                          << ",\"pcm_sha256\":\"" << row.pcm_sha256 << "\",\"feature_sha256\":\""
                          << row.feature_sha256 << "\"}";
            }
            std::cout << "]}" << std::endl;
        }
        return 0;
    } catch (const std::exception & error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
