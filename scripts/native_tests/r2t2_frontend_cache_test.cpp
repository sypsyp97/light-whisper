#include "engine/community_models/confucius4_r2t2/frontend_whisper.h"
#include "engine/framework/audio/conversion.h"
#include "engine/framework/audio/fft.h"
#include "engine/framework/audio/waveform_ops.h"

#include <algorithm>
#include <cmath>
#include <complex>
#include <cstdint>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>



namespace engine::audio {
void r2t2_test_real_fft_forward(
    const TensorShape &, const TensorStrideBytes &, const TensorStrideBytes &,
    size_t, const float *, std::complex<float> *, float, size_t);
}

namespace {
size_t fft_frames = 0;
size_t assertions = 0;
namespace r2t2 = engine::community_models::confucius4_r2t2;
using engine::runtime::AudioBuffer;
using r2t2::R2T2ASRAudioFeatures;
using r2t2::R2T2ASRWhisperFrontend;

void require(bool condition, const std::string & message) {
    ++assertions;
    if (!condition) {
        throw std::runtime_error(message);
    }
}

std::vector<float> signal(size_t count, float gain = 0.25F, size_t offset = 0) {
    std::vector<float> out(count);
    constexpr double pi = 3.14159265358979323846;
    for (size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i + offset) / 16000.0;
        out[i] = gain * static_cast<float>(
            0.63 * std::sin(2.0 * pi * 233.0 * t) +
            0.27 * std::sin(2.0 * pi * 1703.0 * t));
        if ((i + offset) % 7919 == 101) {
            out[i] += gain * 0.1F;
        }
    }
    return out;
}

AudioBuffer mono(const std::vector<float> & samples, size_t count) {
    return {16000, 1, std::vector<float>(samples.begin(), samples.begin() + count)};
}

std::vector<float> normalized(const AudioBuffer & audio) {
    auto samples = engine::audio::convert_interleaved_audio_to_mono_linear_resampled(
        audio.samples, audio.sample_rate, audio.channels, 16000);
    engine::audio::normalize_peak_to_unit_range_and_clamp_in_place(samples);
    if (samples.size() < 8000) {
        samples.resize(8000, 0.0F);
    }
    return samples;
}

engine::audio::WhisperLogMelFeatures reference(const AudioBuffer & audio) {

    return engine::audio::WhisperLogMelExtractor(
        {16000, 400, 160, 128, engine::audio::STFTFamily::Default}).compute(normalized(audio), 1);
}

struct Observation {
    R2T2ASRAudioFeatures features;
    size_t transformed;
};

Observation check(R2T2ASRWhisperFrontend & frontend, const AudioBuffer & audio, const std::string & label) {
    const auto expected = reference(audio);
    fft_frames = 0;
    auto actual = frontend.extract(audio);
    const size_t transformed = fft_frames;
    require(actual.frames == expected.frames, label + ": frame count differs");
    require(actual.mel_bins == expected.mel_bins, label + ": mel bin count differs");
    require(actual.values.size() == expected.values.size(), label + ": feature size differs");
    require(actual.attention_mask == std::vector<int32_t>(static_cast<size_t>(expected.frames), 1),
            label + ": attention mask differs");
    require(actual.encoder_tokens == r2t2::confucius4_r2t2_audio_encoder_token_count(expected.frames),
            label + ": encoder token count differs");


    for (size_t i = 0; i < actual.values.size(); ++i) {
        if (!std::isfinite(actual.values[i]) || actual.values[i] != expected.values[i]) {
            throw std::runtime_error(label + ": feature differs at " + std::to_string(i) +
                " actual=" + std::to_string(actual.values[i]) +
                " expected=" + std::to_string(expected.values[i]));
        }
    }
    ++assertions;
    std::cout << "PARITY " << label << " samples=" << audio.samples.size()
              << " output_frames=" << actual.frames << " fft_frames=" << transformed << '\n';
    return {std::move(actual), transformed};
}

R2T2ASRWhisperFrontend frontend() {
    return R2T2ASRWhisperFrontend(std::make_shared<r2t2::R2T2ASRAssets>());
}

void check_changed_stable_prefix(
    const R2T2ASRAudioFeatures & before, const R2T2ASRAudioFeatures & after,
    const std::string & label) {
    bool changed = false;
    for (int64_t mel = 0; mel < before.mel_bins; ++mel) {
        for (int64_t frame = 0; frame < before.frames - 2; ++frame) {
            const float old_value = before.values[static_cast<size_t>(mel * before.frames + frame)];
            const float new_value = after.values[static_cast<size_t>(mel * after.frames + frame)];
            changed = changed || std::abs(old_value - new_value) > 1.0e-4F;
        }
    }
    require(changed, label + ": fixture did not change stable prefix features");
}

void equivalence_cases() {
    const auto samples = signal(40073);
    auto subject = frontend();
    for (const size_t count : {8000U, 10560U, 16000U, 32000U}) {
        check(subject, mono(samples, count), "append-" + std::to_string(count));
    }
    auto arbitrary_tail = frontend();
    for (const size_t count : {8047U, 10601U, 16019U, 32073U}) {
        check(arbitrary_tail, mono(samples, count), "tail-" + std::to_string(count));
    }
    check(arbitrary_tail, mono(samples, 32073), "identical");

    auto rewritten = mono(samples, 32073);
    rewritten.samples[17] = -0.8F;
    check(arbitrary_tail, rewritten, "prefix-rewrite");
    check(arbitrary_tail, mono(samples, 32073), "same-length-new-content");
    check(arbitrary_tail, mono(samples, 9001), "shrink");
    AudioBuffer sliding{16000, 1, std::vector<float>(samples.begin() + 4000, samples.begin() + 13001)};
    check(arbitrary_tail, sliding, "sliding-window");
    check(arbitrary_tail, mono(samples, 16019), "new-input-after-slide");

    auto loud_subject = frontend();
    auto quiet = signal(8000, 1.0e-6F);
    const auto before_loud = check(loud_subject, {16000, 1, quiet}, "quiet-prefix");
    const auto loud_tail = signal(8000, 0.9F, 8000);
    quiet.insert(quiet.end(), loud_tail.begin(), loud_tail.end());
    const auto after_loud = check(loud_subject, {16000, 1, quiet}, "louder-append-global-floor");
    check_changed_stable_prefix(before_loud.features, after_loud.features, "global-floor");

    auto peak_subject = frontend();
    auto peak_samples = signal(8000);
    const auto before_peak = check(peak_subject, {16000, 1, peak_samples}, "pre-peak-normalization");
    auto peak_tail = signal(8000, 2.8F, 8000);
    peak_tail[83] = 3.0F;
    peak_samples.insert(peak_samples.end(), peak_tail.begin(), peak_tail.end());
    const auto after_peak = check(peak_subject, {16000, 1, peak_samples}, "peak-renormalized-append");
    check_changed_stable_prefix(before_peak.features, after_peak.features, "peak-normalization");

    auto zeros_subject = frontend();
    for (const size_t count : {500U, 7999U, 8000U, 10601U, 16019U}) {
        check(zeros_subject, {16000, 1, std::vector<float>(count, 0.0F)}, "zeros-" + std::to_string(count));
    }
    auto short_subject = frontend();
    for (const size_t count : {1U, 173U, 5000U, 7999U, 8047U}) {
        check(short_subject, mono(samples, count), "padded-short-" + std::to_string(count));
    }

    auto converted_subject = frontend();
    for (const size_t count : {8000U, 10601U, 16019U}) {
        AudioBuffer stereo{16000, 2, {}};
        for (size_t i = 0; i < count; ++i) {
            stereo.samples.push_back(samples[i]);
            stereo.samples.push_back(samples[i] * 0.5F);
        }
        check(converted_subject, stereo, "stereo-" + std::to_string(count));
    }
    for (const size_t count : {4000U, 5301U, 8011U}) {
        auto resampled = mono(samples, count);
        resampled.sample_rate = 8000;
        check(converted_subject, resampled, "resample-" + std::to_string(count));
    }
    check(converted_subject, mono(samples, 16019), "mono-after-conversion");

    auto first_owner = frontend();
    auto second_owner = frontend();
    check(first_owner, mono(samples, 8000), "owner-a");
    auto unrelated = mono(signal(32073, 0.7F, 997), 32073);
    check(second_owner, unrelated, "owner-b");
    check(first_owner, mono(samples, 16019), "owner-a-after-b");
    auto reset_owner = frontend();
    const auto cold = check(reset_owner, mono(samples, 16019), "fresh-owner");
    require(cold.transformed == 1 + 16019 / 160, "FFT counter did not observe full cold extraction");
}

void reuse_cases() {
    const auto samples = signal(32073);
    for (const auto & lengths : std::vector<std::vector<size_t>>{
             {8000, 10560, 16000, 32000}, {8047, 10601, 16019, 32073}}) {
        auto subject = frontend();
        size_t previous = 0;
        for (const size_t count : lengths) {
            const auto observed = check(subject, mono(samples, count), "reuse-" + std::to_string(count));
            const size_t total_frames = 1 + count / 160;
            if (previous == 0) {
                require(observed.transformed == total_frames, "FFT counter missed cold frames");
            } else {



                const size_t stable_previous = 1 + (previous - 200) / 160;
                const size_t maximum_new_frames = total_frames - stable_previous;
                require(observed.transformed <= maximum_new_frames,
                    "append reuse missing: " + std::to_string(previous) + "->" + std::to_string(count) +
                    " transformed " + std::to_string(observed.transformed) +
                    " FFT frames; at most " + std::to_string(maximum_new_frames) +
                    " new/right-boundary frames needed (" + std::to_string(stable_previous) +
                    " stable prefix frames)");
            }
            previous = count;
        }
    }
}
}

namespace engine::audio {
void real_fft_forward(
    const TensorShape & shape, const TensorStrideBytes & input_strides,
    const TensorStrideBytes & output_strides, size_t axis, const float * data_in,
    std::complex<float> * data_out, float scale, size_t threads) {
    size_t transformed = 1;
    for (size_t dim = 0; dim < shape.size(); ++dim) {
        if (dim != axis) {
            transformed *= shape[dim];
        }
    }
    fft_frames += transformed;
    r2t2_test_real_fft_forward(shape, input_strides, output_strides, axis, data_in, data_out, scale, threads);
}
}

int main(int argc, char ** argv) {
    try {
        const std::string mode = argc == 2 ? argv[1] : "all";
        require(mode == "all" || mode == "equivalence" || mode == "reuse", "unknown test mode");
        if (mode != "reuse") {
            equivalence_cases();
        }
        if (mode != "equivalence") {
            reuse_cases();
        }
        std::cout << "PASS mode=" << mode << " assertions=" << assertions << '\n';
        return 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
