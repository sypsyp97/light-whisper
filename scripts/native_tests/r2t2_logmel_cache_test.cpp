#ifdef LIGHT_WHISPER_QWEN_FRONTEND_TEST
#include "engine/models/qwen3_asr/frontend_whisper.h"
#else
#include "engine/community_models/confucius4_r2t2/frontend_whisper.h"
#endif
#include "engine/framework/audio/conversion.h"
#include "engine/framework/audio/fft.h"
#include "engine/framework/audio/waveform_ops.h"

#include <algorithm>
#include <cmath>
#include <complex>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>
#ifdef LIGHT_WHISPER_QWEN_FRONTEND_TEST
#include <condition_variable>
#include <exception>
#include <mutex>
#include <thread>
#endif

namespace engine::audio {
void r2t2_test_real_fft_forward(
    const TensorShape &, const TensorStrideBytes &, const TensorStrideBytes &,
    size_t, const float *, std::complex<float> *, float, size_t);
}

namespace {
namespace audio = engine::audio;
#ifdef LIGHT_WHISPER_QWEN_FRONTEND_TEST
namespace family = engine::models::qwen3_asr;
using FrontendConfig = family::Qwen3ASRFrontendConfig;
using FrontendAssets = family::Qwen3ASRAssets;
using AudioFeatures = family::Qwen3ASRAudioFeatures;
using WhisperFrontend = family::Qwen3ASRWhisperFrontend;
constexpr auto encoder_token_count = family::qwen3_asr_audio_encoder_token_count;
#else
namespace r2t2 = engine::community_models::confucius4_r2t2;
using FrontendConfig = r2t2::R2T2ASRFrontendConfig;
using FrontendAssets = r2t2::R2T2ASRAssets;
using AudioFeatures = r2t2::R2T2ASRAudioFeatures;
using WhisperFrontend = r2t2::R2T2ASRWhisperFrontend;
constexpr auto encoder_token_count = r2t2::confucius4_r2t2_audio_encoder_token_count;
#endif
using engine::runtime::AudioBuffer;
// Concurrent Qwen calls leave instrumentation disabled in their own threads.
thread_local bool count_work = false;
size_t fft_frames = 0, mel_frames = 0, mel_cells = 0, assertions = 0, observations = 0, work_failures = 0;
std::ofstream trace;

void require(bool condition, const std::string & message) {
    ++assertions;
    if (!condition) throw std::runtime_error(message);
}

template <typename T> void write(const T & value) {
    trace.write(reinterpret_cast<const char *>(&value), sizeof(value));
}
template <typename T> void write_vector(const std::vector<T> & values) {
    write(static_cast<uint64_t>(values.size()));
    trace.write(reinterpret_cast<const char *>(values.data()), static_cast<std::streamsize>(values.size() * sizeof(T)));
}

std::vector<float> signal(size_t count, float gain = 0.25F, size_t offset = 0) {
    std::vector<float> out(count);
    constexpr double pi = 3.14159265358979323846;
    for (size_t i = 0; i < count; ++i) {
        const double t = static_cast<double>(i + offset) / 16000.0;
        out[i] = gain * static_cast<float>(0.63 * std::sin(2.0 * pi * 233.0 * t) +
                                         0.27 * std::sin(2.0 * pi * 1703.0 * t));
        if ((i + offset) % 7919 == 101) out[i] += gain * 0.1F;
    }
    return out;
}
AudioBuffer mono(const std::vector<float> & samples, size_t count) {
    return {16000, 1, std::vector<float>(samples.begin(), samples.begin() + count)};
}
std::vector<float> normalized(const AudioBuffer & input, int target_rate) {
    auto samples = audio::convert_interleaved_audio_to_mono_linear_resampled(
        input.samples, input.sample_rate, input.channels, target_rate);
    audio::normalize_peak_to_unit_range_and_clamp_in_place(samples);
    samples.resize(std::max<size_t>(8000, samples.size()), 0.0F);
    return samples;
}

// Only models the publicly observable reuse boundary, never feature arithmetic.
// Shared frontend copies receive the same ExpectedCache object.
struct ExpectedCache { std::vector<float> samples; int64_t frames = 0; };
struct Subject {
    FrontendConfig config;
    WhisperFrontend frontend;
    std::shared_ptr<ExpectedCache> cache = std::make_shared<ExpectedCache>();
    static std::shared_ptr<FrontendAssets> assets(FrontendConfig config) {
        auto out = std::make_shared<FrontendAssets>();
        out->config.frontend = config;
        return out;
    }
    explicit Subject(FrontendConfig value) : config(value), frontend(assets(value)) {}
};

AudioFeatures check(Subject & subject, const AudioBuffer & input, const std::string & label, bool enforce_work) {
    const auto & config = subject.config;
    auto samples = normalized(input, config.sample_rate);
    count_work = false;
    const auto expected = audio::WhisperLogMelExtractor({
        config.sample_rate, config.n_fft, config.hop_length, config.feature_size, audio::STFTFamily::Default
    }).compute(samples, 1);
    const int64_t stft_frames = 1 + (static_cast<int64_t>(samples.size()) +
        2 * (config.n_fft / 2) - config.n_fft) / config.hop_length;
    int64_t reusable = 0;
    const auto & previous = subject.cache->samples;
    const bool identical = previous.size() == samples.size() && !previous.empty() &&
        std::memcmp(previous.data(), samples.data(), previous.size() * sizeof(float)) == 0;
    if (identical) {
        reusable = subject.cache->frames;
    } else if (previous.size() >= static_cast<size_t>(config.n_fft) && samples.size() >= previous.size() &&
               std::memcmp(previous.data(), samples.data(), previous.size() * sizeof(float)) == 0) {
        const int64_t stable = 1 + (static_cast<int64_t>(previous.size()) -
            (config.n_fft - config.n_fft / 2)) / config.hop_length;
        // Last STFT column was dropped before mel/log extraction. It cannot be copied.
        reusable = std::min(stable, subject.cache->frames);
    }
    const size_t needed_mel_frames = static_cast<size_t>(expected.frames - reusable);
    const size_t needed_fft_frames = needed_mel_frames == 0 ? 0 : static_cast<size_t>(stft_frames - reusable);
    fft_frames = mel_frames = mel_cells = 0;
    count_work = true;
    const WhisperFrontend & frontend = subject.frontend;
    auto actual = frontend.extract(input);
    count_work = false;
    require(actual.frames == expected.frames && actual.frames == stft_frames - 1, label + ": frame count");
    require(actual.mel_bins == expected.mel_bins && actual.mel_bins == config.feature_size, label + ": mel bins");
    require(actual.values.size() == expected.values.size(), label + ": feature size");
    require(actual.attention_mask == std::vector<int32_t>(static_cast<size_t>(actual.frames), 1), label + ": mask");
    require(actual.encoder_tokens == encoder_token_count(actual.frames), label + ": tokens");
    for (size_t i = 0; i < actual.values.size(); ++i) {
        require(std::isfinite(actual.values[i]), label + ": nonfinite feature");
        if (std::memcmp(&actual.values[i], &expected.values[i], sizeof(float)) != 0) {
            throw std::runtime_error(label + ": exact feature mismatch at " + std::to_string(i));
        }
    }
    ++assertions;
    ++observations;
    write(static_cast<uint64_t>(label.size()));
    trace.write(label.data(), static_cast<std::streamsize>(label.size()));
    write(actual.mel_bins); write(actual.frames); write(actual.encoder_tokens);
    write_vector(actual.attention_mask); write_vector(actual.values);
    require(trace.good(), label + ": trace write");
    std::cout << "OBS " << label << " frames=" << actual.frames << " reusable=" << reusable
              << " fft=" << fft_frames << "/" << needed_fft_frames
              << " mel_frames=" << mel_frames << "/" << needed_mel_frames
              << " mel_cells=" << mel_cells << "/" << needed_mel_frames * config.feature_size << '\n';
    // Tail FFT work may improve further (for example by avoiding the dropped
    // final STFT column). It must never transform any cached stable prefix.
    const bool fft_ok = reusable == 0 ? fft_frames == needed_fft_frames : fft_frames <= needed_fft_frames;
    if (enforce_work && (!fft_ok || mel_frames != needed_mel_frames ||
                        mel_cells != needed_mel_frames * static_cast<size_t>(config.feature_size))) {
        ++work_failures;
        std::cout << "WORK_RED " << label << " computed mel_cells=" << mel_cells
                  << " expected=" << needed_mel_frames * config.feature_size
                  << " fft=" << fft_frames << "/" << needed_fft_frames << '\n';
    }
    subject.cache->samples = std::move(samples);
    subject.cache->frames = actual.frames;
    return actual;
}

void changed_prefix(const AudioFeatures & before, const AudioFeatures & after, const std::string & label) {
    bool changed = false;
    for (int64_t mel = 0; mel < before.mel_bins; ++mel) {
        for (int64_t frame = 0; frame < before.frames - 2; ++frame) {
            changed = changed || std::abs(before.values[static_cast<size_t>(mel * before.frames + frame)] -
                after.values[static_cast<size_t>(mel * after.frames + frame)]) > 1.0e-4F;
        }
    }
    require(changed, label + ": fixture failed to change old stable features");
}

#ifdef LIGHT_WHISPER_QWEN_FRONTEND_TEST
void concurrent_copies(FrontendConfig config, const std::string & name) {
    constexpr size_t worker_count = 3, rounds = 6;
    const auto samples = signal(64073);
    const std::vector<AudioBuffer> inputs{
        mono(samples, 64073), mono(samples, 16019),
        {16000, 1, signal(48019, 0.7F, 997)},
        {16000, 1, std::vector<float>(8000, 0.0F)},
        {16000, 1, signal(32073, 1.0e-6F)}, mono(samples, 32073),
    };
    std::vector<audio::WhisperLogMelFeatures> expected;
    for (const auto & input : inputs) {
        expected.push_back(audio::WhisperLogMelExtractor({
            config.sample_rate, config.n_fft, config.hop_length, config.feature_size,
            audio::STFTFamily::Default,
        }).compute(normalized(input, config.sample_rate), 1));
    }
    Subject original(config);
    const std::vector<WhisperFrontend> owners(worker_count, original.frontend);
    struct Result { AudioFeatures features; std::exception_ptr error; };
    std::vector<Result> results(worker_count * rounds);
    std::mutex mutex;
    std::condition_variable ready;
    size_t arrived = 0, generation = 0;
    auto rendezvous = [&] {
        std::unique_lock<std::mutex> lock(mutex);
        const auto current = generation;
        if (++arrived == worker_count) {
            arrived = 0;
            ++generation;
            ready.notify_all();
        } else {
            ready.wait(lock, [&] { return generation != current; });
        }
    };
    std::vector<std::thread> workers;
    for (size_t worker = 0; worker < worker_count; ++worker) {
        workers.emplace_back([&, worker] {
            const WhisperFrontend & frontend = owners[worker];
            for (size_t round = 0; round < rounds; ++round) {
                auto & result = results[worker * rounds + round];
                // Release all copied owners together for every call; no timing
                // threshold or global work/trace mutation in worker threads.
                rendezvous();
                try { result.features = frontend.extract(inputs[(worker + round) % inputs.size()]); }
                catch (...) { result.error = std::current_exception(); }
            }
        });
    }
    for (auto & worker : workers) worker.join();
    for (size_t worker = 0; worker < worker_count; ++worker) {
        for (size_t round = 0; round < rounds; ++round) {
            const auto & result = results[worker * rounds + round];
            if (result.error) std::rethrow_exception(result.error);
            const auto & actual = result.features;
            const auto & fresh = expected[(worker + round) % inputs.size()];
            const auto label = name + "/concurrent-copy-" + std::to_string(worker) + "-" + std::to_string(round);
            require(actual.frames == fresh.frames, label + ": frame count");
            require(actual.mel_bins == fresh.mel_bins, label + ": mel bins");
            require(actual.values.size() == fresh.values.size(), label + ": feature size");
            require(std::memcmp(actual.values.data(), fresh.values.data(), fresh.values.size() * sizeof(float)) == 0,
                    label + ": exact features");
            require(actual.attention_mask == std::vector<int32_t>(static_cast<size_t>(fresh.frames), 1), label + ": mask");
            require(actual.encoder_tokens == encoder_token_count(fresh.frames), label + ": tokens");
            ++observations;
            write(static_cast<uint64_t>(label.size()));
            trace.write(label.data(), static_cast<std::streamsize>(label.size()));
            write(actual.mel_bins); write(actual.frames); write(actual.encoder_tokens);
            write_vector(actual.attention_mask); write_vector(actual.values);
            require(trace.good(), label + ": trace write");
        }
    }
    std::cout << "CONCURRENT_PASS " << name << " workers=" << worker_count << " rounds=" << rounds << '\n';
}
#endif

void cases(FrontendConfig config, const std::string & name, bool enforce_work) {
    const auto samples = signal(40073);
    auto observe = [&](Subject & subject, const AudioBuffer & input, const std::string & label) {
        return check(subject, input, name + "/" + label, enforce_work);
    };
    for (const auto & lengths : std::vector<std::vector<size_t>>{
             {8000, 10560, 16000, 32000}, {8047, 10601, 16019, 32073}}) {
        Subject subject(config);
        const auto prefix = lengths.front() == 8000 ? "append-" : "tail-";
        for (const auto count : lengths) observe(subject, mono(samples, count), prefix + std::to_string(count));
        observe(subject, mono(samples, lengths.back()), prefix + std::string("identical"));
        // Append without adding an output frame: right-reflected columns may still change.
        observe(subject, mono(samples, lengths.back() + 1), prefix + std::string("one-sample"));
    }
    Subject reset(config);
    observe(reset, mono(samples, 32073), "reset-initial");
    auto rewritten = mono(samples, 32073); rewritten.samples[17] = -0.8F;
    observe(reset, rewritten, "prefix-rewrite");
    observe(reset, mono(samples, 32073), "same-length-new-content");
    observe(reset, mono(samples, 9001), "shrink");
    observe(reset, {16000, 1, std::vector<float>(samples.begin() + 4000, samples.begin() + 13001)}, "shift");
    observe(reset, mono(samples, 16019), "after-shift");

    Subject floor_subject(config);
    auto quiet = signal(8000, 1.0e-6F);
    const auto before_floor = observe(floor_subject, {16000, 1, quiet}, "quiet");
    auto loud = signal(8000, 0.9F, 8000); quiet.insert(quiet.end(), loud.begin(), loud.end());
    const auto after_floor = observe(floor_subject, {16000, 1, quiet}, "loud-append-global-floor");
    changed_prefix(before_floor, after_floor, name + "/global-floor");

    Subject peak_subject(config);
    auto peak = signal(8000);
    const auto before_peak = observe(peak_subject, {16000, 1, peak}, "before-peak");
    auto peak_tail = signal(8000, 2.8F, 8000); peak_tail[83] = 3.0F;
    peak.insert(peak.end(), peak_tail.begin(), peak_tail.end());
    const auto after_peak = observe(peak_subject, {16000, 1, peak}, "peak-renormalized-append");
    changed_prefix(before_peak, after_peak, name + "/peak-renormalization");

    Subject short_subject(config), zero_subject(config);
    for (const size_t count : {1U, 173U, 5000U, 7999U, 8047U})
        observe(short_subject, mono(samples, count), "padded-short-" + std::to_string(count));
    for (const size_t count : {500U, 7999U, 8000U, 10601U, 16019U})
        observe(zero_subject, {16000, 1, std::vector<float>(count, 0.0F)}, "zero-" + std::to_string(count));

    Subject converted(config);
    for (const size_t count : {8000U, 10601U, 16019U}) {
        AudioBuffer stereo{16000, 2, {}};
        for (size_t i = 0; i < count; ++i) {
            stereo.samples.push_back(samples[i]); stereo.samples.push_back(samples[i] * 0.5F);
        }
        observe(converted, stereo, "stereo-" + std::to_string(count));
    }
    for (const size_t count : {4000U, 5301U, 8011U}) {
        auto resampled = mono(samples, count); resampled.sample_rate = 8000;
        observe(converted, resampled, "resample-" + std::to_string(count));
    }
    observe(converted, mono(samples, 16019), "mono-after-conversion");

    Subject first(config), second(config);
    observe(first, mono(samples, 8000), "owner-a");
    observe(second, mono(signal(32073, 0.7F, 997), 32073), "owner-b");
    observe(first, mono(samples, 16019), "owner-a-after-b");
    auto copy = first;
    observe(copy, mono(samples, 32073), "shared-copy-append");
    observe(first, mono(samples, 32073), "original-after-shared-copy");
    observe(copy, mono(samples, 9001), "shared-copy-shrink");
    observe(first, mono(samples, 16019), "original-after-shared-shrink");
    Subject fresh(config);
    observe(fresh, mono(samples, 16019), "fresh-owner");
#ifdef LIGHT_WHISPER_QWEN_FRONTEND_TEST
    // A one-sample append can change reflected data without adding a frame;
    // another can leave every available output column stable.
    Subject reflected(config), stable(config);
    observe(reflected, mono(samples, 8000), "boundary-8000");
    observe(reflected, mono(samples, 8001), "boundary-8001");
    observe(stable, mono(samples, 8047), "boundary-8047");
    observe(stable, mono(samples, 8048), "boundary-8048");

    // The cache key is the PCM after conversion, peak normalization and padding.
    Subject canonical(config);
    observe(canonical, mono(samples, 8000), "canonical-mono");
    AudioBuffer stereo_identity{16000, 2, {}};
    for (size_t i = 0; i < 8000; ++i) {
        stereo_identity.samples.push_back(samples[i]);
        stereo_identity.samples.push_back(samples[i]);
    }
    observe(canonical, stereo_identity, "canonical-stereo-identical");
    observe(canonical, {16000, 1, std::vector<float>(8000, 1.0F)}, "canonical-unit");
    observe(canonical, {16000, 1, std::vector<float>(8000, 2.0F)}, "canonical-peak-identical");
    observe(canonical, {16000, 1, std::vector<float>(8000, 0.0F)}, "canonical-zero");
    observe(canonical, {8000, 1, std::vector<float>(4000, 0.0F)}, "canonical-resampled-identical");

    for (const auto & input : std::vector<AudioBuffer>{
             {0, 1, {0.1F}}, {16000, 0, {0.1F}}, {16000, 1, {}}, {16000, 2, {0.1F}}}) {
        bool rejected = false;
        try { canonical.frontend.extract(input); }
        catch (const std::runtime_error &) { rejected = true; }
        require(rejected, name + ": invalid audio accepted");
    }
    observe(canonical, {16000, 1, std::vector<float>(8000, 0.0F)}, "canonical-after-invalid");
    concurrent_copies(config, name);
#endif
}
}  // namespace

namespace engine::audio {
// Generated dsp.cpp redirects its sole sparse invocation here. Real arithmetic
// remains in the unchanged sparse member; count only successfully computed cells.
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
    r2t2_test_real_fft_forward(shape, input_strides, output_strides, axis, data_in, data_out, scale, threads);
}
}  // namespace engine::audio

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: test <parity|all> <trace.bin>");
        const std::string mode = argv[1];
        require(mode == "parity" || mode == "all", "unknown mode");
        trace.open(argv[2], std::ios::binary);
        require(trace.good(), "trace open");
        cases({16000, 128, 160, 400}, "fft400-hop160-mel128", mode == "all");
        cases({16000, 64, 512, 400}, "fft400-hop512-mel64", mode == "all");
        cases({16000, 80, 128, 512}, "fft512-hop128-mel80", mode == "all");
        trace.close();
        require(!trace.fail(), "trace close");
        std::cout << "NUMERICAL_PASS observations=" << observations << " assertions=" << assertions << '\n';
        std::cout << (work_failures ? "WORK_FAIL" : "PASS") << " mode=" << mode
                  << " work_failures=" << work_failures << '\n';
        return work_failures ? 2 : 0;
    } catch (const std::exception & error) {
        std::cerr << "FAIL " << error.what() << '\n';
        return 1;
    }
}
