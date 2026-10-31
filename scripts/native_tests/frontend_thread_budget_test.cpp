#include "engine/community_models/confucius4_r2t2/frontend_whisper.h"
#include "engine/models/qwen3_asr/frontend_whisper.h"
#include "engine/framework/audio/conversion.h"
#include "engine/framework/audio/fft.h"
#include "engine/framework/audio/waveform_ops.h"

#include <cmath>
#include <complex>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

// Rename only fft.cpp's symbol. Observe STFT's actual request, then run the
// unchanged FFT with that request; this does not measure an OS thread cap.
namespace engine::audio {
void frontend_budget_real_fft_forward(
    const TensorShape &, const TensorStrideBytes &, const TensorStrideBytes &,
    size_t, const float *, std::complex<float> *, float, size_t);
}

namespace {
namespace audio = engine::audio;
namespace qwen = engine::models::qwen3_asr;
namespace r2t2 = engine::community_models::confucius4_r2t2;
using engine::runtime::AudioBuffer;

struct FFTCall { size_t threads, frames; };
std::vector<FFTCall> calls;
size_t observations = 0, budget_failures = 0;

void require(bool condition, const std::string & message) {
    if (!condition) throw std::runtime_error(message);
}

std::vector<float> signal(size_t count) {
    std::vector<float> samples(count);
    for (size_t i = 0; i < count; ++i) {
        // Bounded below unity: appending cannot renormalize the stable prefix.
        samples[i] = static_cast<float>(static_cast<int>(i % 97) - 48) / 64.0F;
    }
    return samples;
}

void check_features(const audio::WhisperLogMelFeatures & actual,
                    const audio::WhisperLogMelFeatures & expected,
                    const std::string & label) {
    require(actual.frames == expected.frames && actual.mel_bins == expected.mel_bins,
            label + ": feature shape differs from uncached extraction");
    require(actual.values.size() == expected.values.size() &&
            std::memcmp(actual.values.data(), expected.values.data(), actual.values.size() * sizeof(float)) == 0,
            label + ": feature bytes differ from uncached extraction");
    for (float value : actual.values) require(std::isfinite(value), label + ": nonfinite feature");
}

void check_request(const std::string & label, size_t threads, int step, int64_t full_frames) {
    ++observations;
    if (step == 2) {
        require(calls.empty(), label + ": unchanged cache hit performed FFT work");
    } else {
        require(!calls.empty(), label + ": fixture did not exercise FFT work");
        size_t frames = 0;
        for (const auto & call : calls) {
            frames += call.frames;
            if (call.threads != threads) {
                ++budget_failures;
                std::cout << "THREAD_BUDGET_FAIL " << label << " requested=" << threads
                          << " observed=" << call.threads << '\n';
            }
        }
        require(step == 0 ? frames == static_cast<size_t>(full_frames) :
                           frames > 0 && frames < static_cast<size_t>(full_frames),
                label + ": fixture did not exercise the expected full/tail branch");
    }
    std::cout << "OBSERVE " << label << " requested=" << threads << " fft_calls=" << calls.size() << '\n';
}

const char * step_name(int step) {
    return step == 0 ? "full" : step == 1 ? "appended-tail" : "unchanged";
}

void dsp_case(size_t threads, bool use_default) {
    const audio::WhisperLogMelConfig config{16000, 400, 160, 128, audio::STFTFamily::Default};
    const audio::WhisperLogMelExtractor subject(config), oracle(config);
    for (int step = 0; step < 3; ++step) {
        const auto samples = signal(step == 0 ? 8000 : 11200);
        const auto expected = oracle.compute(samples, threads);
        calls.clear();
        const auto actual = use_default ? subject.compute_cached(samples) : subject.compute_cached(samples, threads);
        const auto label = "dsp/" + std::string(use_default ? "default0" : std::to_string(threads)) + "/" + step_name(step);
        check_features(actual, expected, label);
        check_request(label, threads, step, expected.frames + 1);
    }
}

template <typename Frontend, typename Assets, typename TokenCount>
void frontend_case(const std::string & family, size_t threads, bool use_default, TokenCount token_count) {
    const auto assets = std::make_shared<Assets>();
    const auto & config = assets->config.frontend;
    const Frontend subject = use_default ? Frontend(assets) : Frontend(assets, threads);
    const audio::WhisperLogMelExtractor oracle({config.sample_rate, config.n_fft, config.hop_length,
                                               config.feature_size, audio::STFTFamily::Default});
    for (int step = 0; step < 3; ++step) {
        const AudioBuffer input{16000, 1, signal(step == 0 ? 8000 : 11200)};
        auto samples = audio::convert_interleaved_audio_to_mono_linear_resampled(
            input.samples, input.sample_rate, input.channels, config.sample_rate);
        audio::normalize_peak_to_unit_range_and_clamp_in_place(samples);
        const auto expected = oracle.compute(samples, threads);
        calls.clear();
        const auto actual = subject.extract(input);
        const auto label = family + "/" + (use_default ? "default0" : std::to_string(threads)) + "/" + step_name(step);
        check_features({actual.values, actual.mel_bins, actual.frames}, expected, label);
        require(actual.attention_mask == std::vector<int32_t>(static_cast<size_t>(expected.frames), 1),
                label + ": attention mask differs");
        require(actual.encoder_tokens == token_count(expected.frames), label + ": encoder token count differs");
        check_request(label, threads, step, expected.frames + 1);
    }
}
}  // namespace

namespace engine::audio {
void real_fft_forward(
    const TensorShape & shape, const TensorStrideBytes & input_strides,
    const TensorStrideBytes & output_strides, size_t axis, const float * data_in,
    std::complex<float> * data_out, float scale, size_t threads) {
    size_t frames = 1;
    for (size_t dim = 0; dim < shape.size(); ++dim) if (dim != axis) frames *= shape[dim];
    calls.push_back({threads, frames});
    frontend_budget_real_fft_forward(shape, input_strides, output_strides, axis, data_in, data_out, scale, threads);
}
}  // namespace engine::audio

int main() {
    try {
        for (const size_t threads : {1U, 3U, 0U}) {
            const bool use_default = threads == 0;
            dsp_case(threads, use_default);
            frontend_case<qwen::Qwen3ASRWhisperFrontend, qwen::Qwen3ASRAssets>(
                "qwen", threads, use_default, qwen::qwen3_asr_audio_encoder_token_count);
            frontend_case<r2t2::R2T2ASRWhisperFrontend, r2t2::R2T2ASRAssets>(
                "r2t2", threads, use_default, r2t2::confucius4_r2t2_audio_encoder_token_count);
        }
        std::cout << "NUMERICAL_PASS observations=" << observations << '\n';
        if (budget_failures) {
            std::cout << "THREAD_BUDGET_RED failures=" << budget_failures << '\n';
            return 2;
        }
        std::cout << "PASS forwarded FFT budgets 1/3/default0; full/tail/cache-hit; DSP/Qwen/R2T2\n";
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
