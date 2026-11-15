#include "engine/community_models/confucius4_r2t2/frontend_whisper.h"
#include "engine/framework/audio/wav_reader.h"

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <memory>
#include <numeric>
#include <stdexcept>
#include <string>
#include <vector>

namespace r2t2 = engine::community_models::confucius4_r2t2;
using Clock = std::chrono::steady_clock;

int main(int argc, char ** argv) {
    try {
        if (argc != 3) {
            throw std::runtime_error("usage: r2t2_frontend_bench WAV measured_iterations");
        }
        const int iterations = std::stoi(argv[2]);
        if (iterations < 1) {
            throw std::runtime_error("iterations must be positive");
        }
        const auto wav = engine::audio::read_wav_f32(std::filesystem::path(argv[1]));
        if (wav.sample_rate != 16000 || wav.channels != 1 || wav.samples.empty()) {
            throw std::runtime_error("expected nonempty mono 16 kHz WAV");
        }
        constexpr size_t sample_rate = 16000;
        constexpr size_t chunk = 2560;
        constexpr size_t duration = 36 * sample_rate;
        constexpr size_t window_limit = 16 * sample_rate;
        constexpr size_t trim_samples = 8 * sample_rate;
        constexpr int warmups = 2;
        std::vector<float> repeated(duration);
        for (size_t i = 0; i < repeated.size(); ++i) {
            repeated[i] = wav.samples[i % wav.samples.size()];
        }
        std::cout << std::setprecision(17);
        for (int iteration = -warmups; iteration < iterations; ++iteration) {
            r2t2::R2T2ASRWhisperFrontend frontend(std::make_shared<r2t2::R2T2ASRAssets>());
            engine::runtime::AudioBuffer audio{16000, 1, {}};
            struct Sample {
                size_t captured;
                size_t window;
                int64_t nanoseconds;
                bool trimmed;
            };
            std::vector<Sample> timings;
            int64_t total_ns = 0;
            double checksum = 0.0;
            for (size_t start = 0; start < repeated.size(); start += chunk) {
                const size_t end = std::min(repeated.size(), start + chunk);
                audio.samples.insert(audio.samples.end(), repeated.begin() + start, repeated.begin() + end);
                const bool trimmed = audio.samples.size() > window_limit;
                if (trimmed) {
                    audio.samples.erase(audio.samples.begin(), audio.samples.begin() + trim_samples);
                }
                const auto before = Clock::now();
                const auto features = frontend.extract(audio);
                const auto after = Clock::now();
                const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(after - before).count();
                total_ns += elapsed;
                timings.push_back({end, audio.samples.size(), elapsed, trimmed});


                checksum += std::accumulate(features.values.begin(), features.values.end(), 0.0);
                checksum += static_cast<double>(features.frames + features.encoder_tokens);
            }
            std::cout << "{\"iteration\":" << iteration << ",\"warmup\":"
                      << (iteration < 0 ? "true" : "false") << ",\"total_ns\":" << total_ns
                      << ",\"checksum\":" << checksum << ",\"extracts\":[";
            for (size_t i = 0; i < timings.size(); ++i) {
                const auto & value = timings[i];
                if (i != 0) {
                    std::cout << ',';
                }
                std::cout << "{\"captured_samples\":" << value.captured << ",\"window_samples\":"
                          << value.window << ",\"ns\":" << value.nanoseconds << ",\"trimmed\":"
                          << (value.trimmed ? "true" : "false") << '}';
            }
            std::cout << "]}" << std::endl;
        }
        return 0;
    } catch (const std::exception & error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
