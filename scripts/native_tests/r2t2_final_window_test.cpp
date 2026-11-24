#include <algorithm>
#include <cmath>
#include <iostream>
#include <vector>

struct Audio {
    int sample_rate, channels;
    std::vector<float> samples;
};

struct Config {
    bool coalesced_finish;
    double chunk_seconds;
};

int main() {
    int cases = 0;
    for (int rate : {1, 11025, 16000}) {
        for (int ms : {80, 100, 160, 161, 320, 640, 2000}) {
            // Integer oracle for nearest-frame rounding of a positive duration.
            const int block = std::max(1, (rate * std::min(ms, 320) + 500) / 1000);
            for (int channels : {1, 2}) {
                for (int frames : {0, 1, block - 1, block, block + 1, 3 * block - 1}) {
                    for (bool final_flush : {false, true}) {
                        for (bool coalesced : {false, true}) {
                            const Config stream_config_{coalesced, ms / 1000.0};
                            // The logical window includes any ordinary finish context. Distinct
                            // interleaved values expose damage to the retained prefix or channels.
                            std::vector<float> audio_accum_(frames * channels);
                            for (size_t i = 0; i < audio_accum_.size(); ++i) {
                                audio_accum_[i] = static_cast<float>(i + 1);
                            }
                            const auto logical = audio_accum_;
                            Audio accum{rate, channels, {}};
#include "acoustic_input.inc"
                            int expected = frames;
                            if (final_flush && coalesced) {
                                while (expected % block != 0) ++expected;
                            }
                            if (audio_accum_ != logical || accum.samples.size() != size_t(expected * channels) ||
                                !std::equal(logical.begin(), logical.end(), accum.samples.begin()) ||
                                !std::all_of(accum.samples.begin() + logical.size(), accum.samples.end(),
                                             [](float sample) { return sample == 0.0f; })) {
                                std::cerr << "rate=" << rate << " channels=" << channels << " ms=" << ms
                                          << " frames=" << frames << " final=" << final_flush
                                          << " coalesced=" << coalesced << " expected_values=" << expected * channels
                                          << " actual_values=" << accum.samples.size() << '\n';
                                return 1;
                            }
                            ++cases;
                        }
                    }
                }
            }
        }
    }
    std::cout << "verified " << cases << " acoustic input cases\n";
}
