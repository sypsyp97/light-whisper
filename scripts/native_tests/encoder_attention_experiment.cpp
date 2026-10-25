// Test-only numerical experiment at R2T2's post-projection Q/K/V boundary.
// Uses the shipping modules/ggml library, including the encoder's reshape,
// transpose, Explicit SDPA with GGML_PREC_F32, and contiguous context output.
// Numerical differences are measurements, never a model-quality acceptance gate.
#include "engine/community_models/confucius4_r2t2/types.h"
#include "engine/framework/core/backend.h"
#include "engine/framework/modules/attention/scaled_dot_product_attention.h"
#include "engine/framework/modules/structural_modules.h"

#include <ggml-alloc.h>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <limits>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
namespace core = engine::core;
namespace modules = engine::modules;
namespace fs = std::filesystem;
using Clock = std::chrono::steady_clock;
using engine::community_models::confucius4_r2t2::confucius4_r2t2_audio_encoder_token_count;

void require(bool condition, const std::string & message) {
    if (!condition) { throw std::runtime_error(message); }
}

std::string quoted(const std::string & value) {
    std::ostringstream out;
    out << '"';
    for (const unsigned char c : value) {
        if (c == '"' || c == '\\') { out << '\\' << c; }
        else if (c < 32) { out << "\\u" << std::hex << std::setw(4) << std::setfill('0') << int(c); }
        else { out << c; }
    }
    out << '"';
    return out.str();
}

template <typename T> void save(const fs::path & path, const std::vector<T> & values) {
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    require(file.is_open(), "cannot open raw artifact: " + path.string());
    file.write(reinterpret_cast<const char *>(values.data()), static_cast<std::streamsize>(values.size() * sizeof(T)));
    require(file.good(), "cannot write raw artifact: " + path.string());
}

struct Options {
    std::string backend;
    fs::path output;
    int64_t heads = 0, dim = 0, n_window = 0, n_window_infer = 0;
    int warmup = 0, rounds = 0, threads = 0;
    uint32_t seed = 0;
    bool flash = false;
};

Options options(int argc, char ** argv) {
    Options out;
    for (int i = 1; i < argc; ++i) {
        const std::string key = argv[i];
        if (key == "--include-flash") { out.flash = true; continue; }
        require(i + 1 < argc, "missing value for " + key);
        const std::string value = argv[++i];
        if (key == "--backend") { out.backend = value; }
        else if (key == "--output-dir") { out.output = value; }
        else if (key == "--heads") { out.heads = std::stoll(value); }
        else if (key == "--head-dim") { out.dim = std::stoll(value); }
        else if (key == "--n-window") { out.n_window = std::stoll(value); }
        else if (key == "--n-window-infer") { out.n_window_infer = std::stoll(value); }
        else if (key == "--warmup") { out.warmup = std::stoi(value); }
        else if (key == "--rounds") { out.rounds = std::stoi(value); }
        else if (key == "--threads") { out.threads = std::stoi(value); }
        else if (key == "--seed") { out.seed = static_cast<uint32_t>(std::stoul(value)); }
        else { throw std::runtime_error("unknown option: " + key); }
    }
    require(out.backend == "cpu" || out.backend == "cuda", "explicit --backend cpu|cuda required");
    require(!out.output.empty() && out.heads > 0 && out.dim > 0 && out.n_window > 0 &&
        out.n_window_infer >= 2 * out.n_window && out.warmup > 0 && out.rounds > 0 &&
        out.threads > 0 && out.threads <= 2, "positive geometry/warmup/rounds and 1..2 threads required");
    return out;
}

int64_t compacted_tokens(int64_t frames, int64_t chunk_frames) {
    int64_t tokens = 0;
    for (int64_t start = 0; start < frames; start += chunk_frames) {
        tokens += confucius4_r2t2_audio_encoder_token_count(std::min(chunk_frames, frames - start));
    }
    return tokens;
}

// Same valid window end and padded-query self-attention as audio_encoder.cpp.
bool visible(int64_t row, int64_t col, int64_t valid, int64_t window) {
    const int64_t begin = row < valid ? row / window * window : row;
    const int64_t end = row < valid ? std::min(valid, begin + window) : row + 1;
    return col >= begin && col < end;
}

std::vector<float> pattern(size_t count, uint32_t seed) {
    std::vector<float> values(count);
    for (auto & value : values) {
        seed = 1664525U * seed + 1013904223U;
        value = static_cast<float>(static_cast<int32_t>(seed >> 16) - 32768) / 32768.0F;
    }
    return values;
}

struct Input {
    std::vector<float> q, k, v;
};

Input inputs(int64_t tokens, int64_t valid, int64_t hidden, int64_t window, uint32_t seed, bool zero_padded) {
    const auto count = static_cast<size_t>(tokens * hidden);
    Input out{pattern(count, seed), pattern(count, seed + 1), pattern(count, seed + 2)};
    // Window-specific V offsets make a removed window boundary observable.
    for (int64_t token = 0; token < tokens; ++token) {
        for (int64_t d = 0; d < hidden; ++d) {
            const auto index = static_cast<size_t>(token * hidden + d);
            out.v[index] += 0.125F * static_cast<float>(token / window);
            if (zero_padded && token >= valid) { out.q[index] = out.k[index] = out.v[index] = 0.0F; }
        }
    }
    return out;
}

struct Diff {
    size_t count = 0, equal = 0;
    double max_abs = 0.0, max_relative = 0.0, squared_error = 0.0, squared_reference = 0.0;
    std::string json() const {
        std::ostringstream out;
        out << std::setprecision(17) << "{\"count\":" << count << ",\"bitwise_equal_count\":" << equal
            << ",\"max_abs\":" << max_abs << ",\"max_relative_symmetric\":" << max_relative
            << ",\"l2_absolute\":" << std::sqrt(squared_error)
            << ",\"reference_l2\":" << std::sqrt(squared_reference) << ",\"l2_relative\":";
        if (squared_reference > 0) { out << std::sqrt(squared_error / squared_reference); }
        else if (squared_error == 0) { out << 0; }
        else { out << "null"; }
        out << '}';
        return out.str();
    }
};

Diff difference(const std::vector<float> & reference, const std::vector<float> & candidate, size_t begin, size_t end) {
    require(reference.size() == candidate.size() && begin <= end && end <= reference.size(), "output shape mismatch");
    Diff out;
    out.count = end - begin;
    for (size_t i = begin; i < end; ++i) {
        require(std::isfinite(reference[i]) && std::isfinite(candidate[i]), "non-finite attention output");
        out.equal += std::memcmp(&reference[i], &candidate[i], sizeof(float)) == 0;
        const double error = std::abs(static_cast<double>(reference[i]) - candidate[i]);
        const double denominator = std::max(std::abs(static_cast<double>(reference[i])), std::abs(static_cast<double>(candidate[i])));
        out.max_abs = std::max(out.max_abs, error);
        out.max_relative = std::max(out.max_relative, denominator > 0 ? error / denominator : 0.0);
        out.squared_error += error * error;
        out.squared_reference += static_cast<double>(reference[i]) * reference[i];
    }
    return out;
}

struct ContextDeleter { void operator()(ggml_context * p) const { ggml_free(p); } };
struct AllocatorDeleter { void operator()(ggml_gallocr_t p) const { ggml_gallocr_free(p); } };
struct BackendDeleter { void operator()(ggml_backend_t p) const { ggml_backend_free(p); } };
struct Unsupported : std::runtime_error { using std::runtime_error::runtime_error; };

class Runner {
public:
    Runner(const Options & opt, ggml_backend_t backend, int64_t tokens, int64_t window, const std::string & variant)
        : backend_(backend), window_(window), variant_(variant),
          context_(ggml_init({16 * 1024 * 1024, nullptr, true})) {
        require(context_ != nullptr, "cannot allocate attention graph metadata");
        core::ModuleBuildContext ctx{context_.get(), "test_only.encoder_attention", core::backend_type(backend_)};
        const int64_t hidden = opt.heads * opt.dim;
        for (auto * input : {&q_, &k_, &v_}) {
            *input = core::make_tensor(ctx, GGML_TYPE_F32, core::TensorShape::from_dims({1, tokens, hidden}));
            ggml_set_input(input->tensor);
        }
        const bool dense = variant == "dense_explicit";
        const bool flash = variant == "window_flash";
        const int64_t width = dense ? tokens : window;
        for (int64_t start = 0; start < tokens; start += width) {
            const int64_t length = std::min(width, tokens - start);
            const auto heads = [&](const core::TensorValue & projected) {
                auto value = dense ? projected : modules::SliceModule({1, start, length}).build(ctx, projected);
                value = core::ensure_backend_addressable_layout(ctx, value);
                value = core::reshape_tensor(ctx, value, core::TensorShape::from_dims({1, length, opt.heads, opt.dim}));
                return modules::TransposeModule({{0, 2, 1, 3}, value.shape.rank}).build(ctx, value);
            };
            auto mask = core::make_tensor(ctx, flash ? GGML_TYPE_F16 : GGML_TYPE_F32,
                core::TensorShape::from_dims({1, 1, length, length}));
            ggml_set_input(mask.tensor);
            masks_.push_back(mask);
            starts_.push_back(start);
            mask_bytes_ += ggml_nbytes(mask.tensor);
            auto output = modules::ScaledDotProductAttentionModule({opt.dim,
                flash ? modules::ScaledDotProductAttentionLowering::Flash : modules::ScaledDotProductAttentionLowering::Explicit,
                GGML_PREC_F32}).build(ctx, heads(q_), heads(k_), heads(v_), mask);
            output = core::ensure_backend_addressable_layout(ctx, output);
            output = core::reshape_tensor(ctx, output, core::TensorShape::from_dims({1, length, hidden}));
            output_ = output_.valid() ? modules::ConcatModule({1}).build(ctx, output_, output) : output;
        }
        output_ = core::ensure_backend_addressable_layout(ctx, output_);
        core::validate_shape(output_, core::TensorShape::from_dims({1, tokens, hidden}), "attention context");
        ggml_set_output(output_.tensor);
        graph_ = ggml_new_graph_custom(context_.get(), 4096, false);
        ggml_build_forward_expand(graph_, output_.tensor);
        for (int i = 0; i < ggml_graph_n_nodes(graph_); ++i) {
            const auto * node = ggml_graph_node(graph_, i);
            if (!ggml_backend_supports_op(backend_, node)) {
                throw Unsupported(variant + ": backend does not support " + ggml_op_name(node->op));
            }
        }
        allocator_.reset(ggml_gallocr_new(ggml_backend_get_default_buffer_type(backend_)));
        require(allocator_ && ggml_gallocr_alloc_graph(allocator_.get(), graph_), "cannot allocate attention graph buffer");
    }

    ~Runner() { if (graph_) { core::release_backend_graph_resources(backend_, graph_, true); } }

    void upload(const Input & input, int64_t valid, const fs::path & mask_path, bool wrong_window = false) {
        core::write_tensor_f32(q_, input.q);
        core::write_tensor_f32(k_, input.k);
        core::write_tensor_f32(v_, input.v);
        std::vector<std::byte> raw_masks;
        mask_values_.clear();
        for (size_t i = 0; i < masks_.size(); ++i) {
            const auto length = masks_[i].shape.dims[2];
            std::vector<float> values(static_cast<size_t>(length * length), -std::numeric_limits<float>::infinity());
            for (int64_t row = 0; row < length; ++row) {
                for (int64_t col = 0; col < length; ++col) {
                    const int64_t r = starts_[i] + row, c = starts_[i] + col;
                    const bool allowed = wrong_window ? (r < valid ? c < valid : r == c) : visible(r, c, valid, window_);
                    if (allowed) { values[static_cast<size_t>(row * length + col)] = 0.0F; }
                }
            }
            if (masks_[i].type == GGML_TYPE_F16) { core::write_tensor_f16(masks_[i], values); }
            else { core::write_tensor_f32(masks_[i], values); }
            const auto bytes = core::read_tensor_bytes(masks_[i].tensor);
            raw_masks.insert(raw_masks.end(), bytes.begin(), bytes.end());
            mask_values_.push_back(std::move(values));
        }
        save(mask_path, raw_masks);
        ggml_backend_synchronize(backend_);
        check_inputs(input);
    }

    void check_inputs(const Input & input) const {
        for (const auto & part : {std::make_pair(q_.tensor, &input.q), std::make_pair(k_.tensor, &input.k), std::make_pair(v_.tensor, &input.v)}) {
            const auto actual = core::read_tensor_f32(part.first);
            require(actual.size() == part.second->size() &&
                std::memcmp(actual.data(), part.second->data(), actual.size() * sizeof(float)) == 0,
                variant_ + ": input changed during upload/graph compute");
        }
    }

    double compute(const Input & input) {
        // Gallocr may reuse a leaf's storage after its last consumer. Restore
        // every leaf before every run, including warmup, outside the timer.
        core::write_tensor_f32(q_, input.q);
        core::write_tensor_f32(k_, input.k);
        core::write_tensor_f32(v_, input.v);
        for (size_t i = 0; i < masks_.size(); ++i) {
            if (masks_[i].type == GGML_TYPE_F16) { core::write_tensor_f16(masks_[i], mask_values_[i]); }
            else { core::write_tensor_f32(masks_[i], mask_values_[i]); }
        }
        ggml_backend_synchronize(backend_);
        const auto start = Clock::now();
        const auto status = core::compute_backend_graph(backend_, graph_, nullptr, "test_only.encoder_attention");
        ggml_backend_synchronize(backend_);
        const auto end = Clock::now();
        require(status == GGML_STATUS_SUCCESS, "attention graph compute failed: " + std::to_string(status));
        return std::chrono::duration<double, std::milli>(end - start).count();
    }

    std::vector<float> read() const { return core::read_tensor_f32(output_.tensor); }
    const std::string & name() const { return variant_; }
    std::string allocation_json() const {
        std::ostringstream out;
        out << "\"allocated_graph_buffer_bytes\":" << ggml_gallocr_get_buffer_size(allocator_.get(), 0)
            << ",\"mask_bytes\":" << mask_bytes_ << ",\"graph_nodes\":" << ggml_graph_n_nodes(graph_)
            << ",\"metadata_used_bytes\":" << ggml_used_mem(context_.get())
            << ",\"mask_dtype\":" << quoted(variant_ == "window_flash" ? "float16" : "float32")
            << ",\"mask_square_lengths\":[";
        for (size_t i = 0; i < masks_.size(); ++i) { if (i) { out << ','; } out << masks_[i].shape.dims[2]; }
        out << ']';
        return out.str();
    }

private:
    ggml_backend_t backend_;
    int64_t window_;
    std::string variant_;
    std::unique_ptr<ggml_context, ContextDeleter> context_;
    std::unique_ptr<ggml_gallocr, AllocatorDeleter> allocator_;
    core::TensorValue q_, k_, v_, output_;
    std::vector<core::TensorValue> masks_;
    std::vector<std::vector<float>> mask_values_;
    std::vector<int64_t> starts_;
    ggml_cgraph * graph_ = nullptr;
    size_t mask_bytes_ = 0;
};

struct Case { std::string name; int64_t frames; bool zero_padded; };

void group(const Options & opt, ggml_backend_t backend, std::ofstream & report,
    int64_t capacity_frames, int64_t window, const std::vector<Case> & cases, uint32_t seed) {
    const int64_t chunk = opt.n_window * 2;
    const int64_t tokens = compacted_tokens(capacity_frames, chunk);
    const int64_t hidden = opt.heads * opt.dim;
    std::vector<std::unique_ptr<Runner>> runners;
    for (const auto & variant : {"dense_explicit", "window_explicit", "window_flash"}) {
        if (std::string(variant) == "window_flash" && !opt.flash) { continue; }
        try { runners.push_back(std::make_unique<Runner>(opt, backend, tokens, window, variant)); }
        catch (const Unsupported & error) {
            if (std::string(variant) != "window_flash") { throw; }
            report << "{\"kind\":\"unavailable\",\"variant\":\"window_flash\",\"capacity_tokens\":" << tokens
                << ",\"reason\":" << quoted(error.what()) << "}\n";
        }
    }
    for (size_t index = 0; index < cases.size(); ++index) {
        const auto & item = cases[index];
        const int64_t valid = confucius4_r2t2_audio_encoder_token_count(item.frames);
        require(valid <= tokens && valid == compacted_tokens(item.frames, chunk), "fixture compaction/token formula mismatch");
        const uint32_t case_seed = seed + static_cast<uint32_t>(index * 3);
        const auto input = inputs(tokens, valid, hidden, window, case_seed, item.zero_padded);
        for (const auto & part : {std::make_pair("q", &input.q), std::make_pair("k", &input.k), std::make_pair("v", &input.v)}) {
            save(opt.output / (item.name + "." + part.first + ".f32.bin"), *part.second);
        }
        std::vector<std::vector<double>> samples(runners.size());
        for (size_t r = 0; r < runners.size(); ++r) {
            auto & runner = *runners[r];
            runner.upload(input, valid, opt.output / (item.name + "." + runner.name() + ".mask.bin"));
            for (int warm = 0; warm < opt.warmup; ++warm) { runner.compute(input); }
        }
        // Rotate measured order to reduce a fixed reference/candidate order bias.
        for (int round = 0; round < opt.rounds; ++round) {
            for (size_t offset = 0; offset < runners.size(); ++offset) {
                const auto r = (static_cast<size_t>(round) + offset) % runners.size();
                samples[r].push_back(runners[r]->compute(input));
            }
        }
        const auto reference = runners.front()->read();
        require(reference.size() == static_cast<size_t>(tokens * hidden), "reference output shape mismatch");
        const auto valid_count = static_cast<size_t>(valid * hidden);
        for (size_t r = 0; r < runners.size(); ++r) {
            const auto & runner = *runners[r];
            const auto values = runner.read();
            const auto all = difference(reference, values, 0, reference.size());
            if (item.zero_padded) {
                require(std::all_of(values.begin() + valid_count, values.end(), [](float x) { return x == 0.0F; }),
                    "zero-padded self-attention rows produced nonzero output");
            }
            save(opt.output / (item.name + "." + runner.name() + ".output.f32.bin"), values);
            auto sorted = samples[r];
            std::sort(sorted.begin(), sorted.end());
            const double median = sorted.size() % 2 ? sorted[sorted.size() / 2]
                : (sorted[sorted.size() / 2 - 1] + sorted[sorted.size() / 2]) / 2;
            report << std::setprecision(17) << "{\"kind\":\"measurement\",\"case\":" << quoted(item.name)
                << ",\"variant\":" << quoted(runner.name()) << ",\"capacity_frames\":" << capacity_frames
                << ",\"input_frames\":" << item.frames << ",\"capacity_tokens\":" << tokens << ",\"valid_tokens\":" << valid
                << ",\"window_tokens\":" << window << ",\"heads\":" << opt.heads << ",\"head_dim\":" << opt.dim
                << ",\"seed\":" << case_seed << ",\"zero_padded_qkv\":" << (item.zero_padded ? "true" : "false")
                << ",\"reused_graph_step\":" << index << ",\"compute_wall_median_ms\":" << median
                << ",\"compute_wall_samples_ms\":[";
            for (size_t i = 0; i < samples[r].size(); ++i) { if (i) { report << ','; } report << samples[r][i]; }
            report << "]," << runner.allocation_json() << ",\"all_rows\":" << all.json()
                << ",\"valid_rows\":" << difference(reference, values, 0, valid_count).json()
                << ",\"padded_rows\":" << difference(reference, values, valid_count, values.size()).json()
                << ",\"padded_vs_input_v\":" << difference(input.v, values, valid_count, values.size()).json() << "}\n";
            std::cout << item.name << ' ' << runner.name() << " median_ms=" << median << " equal=" << all.equal
                << '/' << all.count << " max_abs=" << all.max_abs << '\n';
        }
        if (item.name == "multiple_windows") {
            auto & runner = *runners.front();
            runner.upload(input, valid, opt.output / "negative_wrong_window.mask.bin", true);
            runner.compute(input);
            const auto wrong = runner.read();
            const auto diff = difference(reference, wrong, 0, reference.size());
            require(diff.max_abs > 0.0 && diff.equal < diff.count, "negative control did not detect removed window boundary");
            save(opt.output / "negative_wrong_window.output.f32.bin", wrong);
            report << "{\"kind\":\"negative_control\",\"case\":\"multiple_windows\",\"mutation\":\"all valid keys visible across windows\","
                << "\"detected\":true,\"all_rows\":" << diff.json() << "}\n";
            std::cout << "NEGATIVE_CONTROL_DETECTED max_abs=" << diff.max_abs << " changed=" << diff.count - diff.equal << '\n';
        }
        report.flush();
        require(report.good(), "cannot write numerical report");
    }
}
}  // namespace

int main(int argc, char ** argv) try {
    const auto opt = options(argc, argv);
    fs::create_directories(opt.output);
    std::ofstream report(opt.output / "results.jsonl", std::ios::trunc);
    require(report.is_open(), "cannot open numerical report");
    const auto type = opt.backend == "cuda" ? core::BackendType::Cuda : core::BackendType::Cpu;
    std::unique_ptr<ggml_backend, BackendDeleter> backend(core::init_backend({type, 0, opt.threads}));
    require(backend && core::backend_type(backend.get()) == type, "requested backend was not selected");
    core::set_backend_threads(backend.get(), opt.threads);
    const int64_t chunk = opt.n_window * 2;
    const int64_t chunks_per_window = opt.n_window_infer / chunk;
    const int64_t window = confucius4_r2t2_audio_encoder_token_count(chunk) * chunks_per_window;
    report << "{\"kind\":\"backend\",\"requested\":" << quoted(opt.backend)
        << ",\"actual\":" << quoted(ggml_backend_name(backend.get())) << ",\"device\":"
        << quoted(ggml_backend_dev_description(ggml_backend_get_device(backend.get())))
        << ",\"chunk_frames\":" << chunk << ",\"chunk_tokens\":" << confucius4_r2t2_audio_encoder_token_count(chunk)
        << ",\"chunks_per_window_integer_division\":" << chunks_per_window << ",\"window_tokens\":" << window << "}\n";
    const auto frames_for = [&](int64_t wanted) {
        for (int64_t frames = 1; frames <= 100 * wanted; ++frames) {
            if (confucius4_r2t2_audio_encoder_token_count(frames) == wanted) { return frames; }
        }
        throw std::runtime_error("cannot construct token boundary fixture");
    };
    const int64_t one = chunks_per_window * chunk;
    require(compacted_tokens(one, chunk) == window, "one-window fixture does not match source formula");
    const auto two_partial = frames_for(2 * window + 1);
    const int64_t chunks = (two_partial + chunk - 1) / chunk;
    const int64_t bucket = (chunks <= 2 ? chunks : (chunks + 3) / 4 * 4) * chunk;
    group(opt, backend.get(), report, one, window, {{"one_window", one, false}}, opt.seed);
    group(opt, backend.get(), report, 2 * one, window, {{"multiple_windows", 2 * one, false}}, opt.seed + 3);
    group(opt, backend.get(), report, frames_for(window + 1), window,
        {{"partial_tail_1", frames_for(window + 1), false}}, opt.seed + 6);
    group(opt, backend.get(), report, two_partial, window, {{"partial_tail_2", two_partial, false}}, opt.seed + 9);
    group(opt, backend.get(), report, bucket, window,
        {{"bucket_initial_nonzero", two_partial, false}, {"bucket_shrink_zero", chunk, true},
         {"bucket_grow_zero", 2 * one, true}, {"bucket_repeat_nonzero", two_partial, false}}, opt.seed + 12);
    std::cout << "PASS synthetic attention experiment; numerical differences are reported without a quality threshold\n";
    return 0;
} catch (const std::exception & error) {
    std::cerr << "FAIL encoder attention experiment: " << error.what() << '\n';
    return 1;
}
