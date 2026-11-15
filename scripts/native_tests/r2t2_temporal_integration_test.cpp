
#define main temporal_unused_shared_main
#include "qwen_shared_decoder_test.cpp"
#undef main
#include "r2t2_temporal_observer.h"

namespace integration {
namespace obs = temporal_observer;
size_t calls = 0, verify_graphs = 0, token_graphs = 0, prompt_graphs = 0;
size_t dense_graphs = 0, scalar_graphs = 0;
size_t negative_draft_calls = 0, qualified_calls = 0;
struct ObservedWork { size_t verify = 0, token = 0, prompt = 0; };
ObservedWork observed_work() {
    ObservedWork result;
    for (const auto & graph : obs::graphs) {
        if (graph.verification) {
            ++result.verify;
            if (graph.scalar_rows) { ++scalar_graphs; } else { ++dense_graphs; }
        }
        else if (graph.input_ids.size() == 1 && graph.positions.size() == 1 && graph.caches.size() == 4) { ++result.token; }
        else if (!graph.positions.empty()) { ++result.prompt; }
    }
    return result;
}
runtime::GreedyQwenDecoderRuntime::Prompt input(int64_t steps, const std::vector<int32_t> & positions,
                                               uint32_t seed = 701) {
    const auto prompt = make_prompt(steps, false);
    return {prompt.input_ids, {pattern(positions.size() * width, seed, 0.75F),
                              static_cast<int64_t>(positions.size()), positions}};
}
void generate(runtime::GreedyQwenDecoderRuntime & subject, runtime::GreedyQwenDecoderRuntime & fresh,
              const runtime::GreedyQwenDecoderRuntime::Prompt & prompt, int64_t budget, std::ofstream & trace,
              bool forbid_verify = false, bool qualify = false, const char * label = nullptr) {
    obs::reset();
    const auto actual = subject.generate(prompt, budget, true);
    const auto measured = observed_work();
    require(obs::pending.empty(), "public generate leaked queued input storage");
    obs::reset();
    const auto expected = fresh.generate(prompt, budget, true);
    if (label) {
        std::cout << label << " actual=";
        for (const auto id : actual) { std::cout << id << ','; }
        std::cout << " serial=";
        for (const auto id : expected) { std::cout << id << ','; }
        std::cout << " verification_graphs=" << measured.verify << '\n';
    }
    require(actual == expected, "public reused generate trace differs from fresh serial owner");
    require(actual.size() <= static_cast<size_t>(budget), "public generate exceeded budget");
    if (forbid_verify) { require(measured.verify == 0, "incompatible draft/budget entered verify graph"); ++negative_draft_calls; }
    if (qualify) { ++qualified_calls; }
    verify_graphs += measured.verify; token_graphs += measured.token; prompt_graphs += measured.prompt;
    record(trace, static_cast<uint64_t>(calls++)); record(trace, budget); record_tokens(trace, actual);
}
void suite(core::ExecutionContext & execution, std::ofstream & trace) {
    const auto assets = make_assets(false, false);
    auto spec = decoder_spec(*assets);
    spec.max_position_embeddings = 256;
    spec.decoder.stack.runtime.static_cache.set_rows_mode = engine::modules::QwenDecoderStaticCacheSetRowsMode::BackendViewOptimized;
    spec.decoder.stack.runtime.attention.prefill_mode = engine::modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    spec.decoder.stack.runtime.attention.static_mode = engine::modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    runtime::GreedyQwenDecoderRuntime subject(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    auto check = [&](const runtime::GreedyQwenDecoderRuntime::Prompt & prompt, int64_t budget,
                     bool forbid = false, bool qualify = false) {
        runtime::GreedyQwenDecoderRuntime fresh(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
        generate(subject, fresh, prompt, budget, trace, forbid, qualify);
    };
    const auto base = input(65, {0, 16});
    check(base, 12, true);
    check(base, 12, false, true);
    check(base, 12, false, true);
    for (const int64_t budget : {1, 2, 3, 4, 5, 8, 9, 12}) {
        check(base, budget, budget <= 2); check(base, budget, budget <= 2, budget >= 5);
    }


    for (const uint32_t seed : {9U, 701U, 20261019U}) {
        const auto changed_audio = input(65, {0, 16}, seed);
        check(changed_audio, 12); check(changed_audio, 12, false, true);
    }


    for (const auto & positions : std::vector<std::vector<int32_t>>{{1, 17}, {0, 16, 16}, {16, 0}, {0, 64}, {}}) {
        auto shifted = input(65, positions);
        check(shifted, 12); check(shifted, 12, false, true);
    }
    auto placeholders = base;
    for (const auto position : placeholders.injection.positions) { placeholders.input_ids[position] = (placeholders.input_ids[position] + 1) % vocabulary; }
    check(placeholders, 12); check(placeholders, 12, false, true);
    auto changed_tail = base; changed_tail.input_ids.back() = (changed_tail.input_ids.back() + 1) % vocabulary;
    check(changed_tail, 12); check(changed_tail, 12, false, true);
    for (const int64_t steps : {1, 63, 64, 65, 127, 128, 129, 3, 244}) {
        const auto prompt = input(steps, steps > 1 ? std::vector<int32_t>{0, 1} : std::vector<int32_t>{0});
        check(prompt, 12); check(prompt, 12, false, true);
    }

    runtime::GreedyQwenDecoderRuntime scout(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    const auto generated = scout.generate(base, 12, true);
    require(!generated.empty(), "EOS fixture empty");
    for (const auto eos : {generated.front(), generated[std::min<size_t>(2, generated.size() - 1)]}) {
        auto eos_spec = spec; eos_spec.eos_token_ids = {eos};
        runtime::GreedyQwenDecoderRuntime owner(assets->model_weights, eos_spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
        for (int repeat = 0; repeat < 3; ++repeat) {
            runtime::GreedyQwenDecoderRuntime fresh(assets->model_weights, eos_spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
            generate(owner, fresh, base, 12, trace);
        }
    }
    auto bounded_spec = spec; bounded_spec.max_position_embeddings = 69;
    runtime::GreedyQwenDecoderRuntime bounded(assets->model_weights, bounded_spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    for (int repeat = 0; repeat < 3; ++repeat) {
        runtime::GreedyQwenDecoderRuntime fresh(assets->model_weights, bounded_spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
        generate(bounded, fresh, base, 4, trace);
    }
    bool failed = false;
    try { bounded.generate(base, 5, true); } catch (const std::exception &) { failed = true; }
    require(failed, "public generation exceeded positional capacity");
    auto incompatible = base;
    incompatible.input_ids.assign(base.input_ids.size(), 96);
    check(incompatible, 12); check(incompatible, 12, false, true);
}
void eos_prompt_tail_transition(core::ExecutionContext & execution, std::ofstream & trace) {
    const auto assets = make_assets(false, false);
    auto spec = decoder_spec(*assets);
    spec.max_position_embeddings = 256;
    spec.decoder.stack.runtime.static_cache.set_rows_mode = engine::modules::QwenDecoderStaticCacheSetRowsMode::BackendViewOptimized;
    spec.decoder.stack.runtime.attention.prefill_mode = engine::modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    spec.decoder.stack.runtime.attention.static_mode = engine::modules::QwenDecoderAttentionMode::FlashGroupedViewKV;
    const auto short_prompt = input(65, {0, 16}, 9);
    runtime::GreedyQwenDecoderRuntime scout(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    const auto sequence = scout.generate(short_prompt, 12, true);
    require(sequence.size() >= 4 && sequence[0] != sequence[2] && sequence[1] != sequence[2],
            "EOS transition fixture needs two non-EOS tokens before its third-token EOS");
    spec.eos_token_ids = {sequence[2]};
    auto old_prompt = short_prompt;
    old_prompt.input_ids.insert(old_prompt.input_ids.end(), sequence.begin(), sequence.begin() + 4);
    const size_t tail_start = 17;
    require(std::equal(short_prompt.input_ids.begin() + tail_start, short_prompt.input_ids.end(),
                       old_prompt.input_ids.begin() + tail_start), "shortened tail is not the exact previous-tail prefix");
    const std::vector<int32_t> proposed(old_prompt.input_ids.begin() + short_prompt.input_ids.size(), old_prompt.input_ids.end());
    require(proposed.size() == 4 && proposed.front() == sequence.front() && proposed[2] == spec.eos_token_ids.front(),
            "prompt-tail draft must qualify its first match and contain EOS inside a full block");
    std::cout << "EOS_TRANSITION_FIXTURE old_steps=" << old_prompt.input_ids.size()
              << " short_steps=" << short_prompt.input_ids.size() << " tail_start=" << tail_start
              << " eos=" << spec.eos_token_ids.front() << " proposed=";
    for (const auto id : proposed) { std::cout << id << ','; }
    std::cout << '\n';
    runtime::GreedyQwenDecoderRuntime owner(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    runtime::GreedyQwenDecoderRuntime old_fresh(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
    generate(owner, old_fresh, old_prompt, 12, trace);
    for (int repeat = 0; repeat < 2; ++repeat) {
        runtime::GreedyQwenDecoderRuntime fresh(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
        runtime::GreedyQwenDecoderRuntime expected_owner(assets->model_weights, spec, execution, arena, arena, arena, assets::TensorStorageType::F32);
        require(expected_owner.generate(short_prompt, 12, true) == std::vector<int32_t>(sequence.begin(), sequence.begin() + 2),
                "fresh serial EOS transition must stop before the proposed third token");
        generate(owner, fresh, short_prompt, 12, trace, false, false, "EOS_PROMPT_TAIL_TRANSITION");
    }
}
}

int main(int argc, char ** argv) {
    try {
        require(argc == 3, "usage: integration_test.exe equivalence|work trace.bin");
        const bool enforce = std::string(argv[1]) == "work";
        require(enforce || std::string(argv[1]) == "equivalence", "invalid mode");
        std::ofstream trace(argv[2], std::ios::binary | std::ios::trunc);
        require(trace.is_open(), "cannot open integration trace");
        core::ExecutionContext execution{{core::BackendType::Cpu, 0, 1}};
        integration::suite(execution, trace);
        integration::eos_prompt_tail_transition(execution, trace);
        require(trace.good(), "integration trace write failed");
        require(temporal_observer::live_verification.empty(), "public owner leaked verification graphs");
        std::cout << "INTEGRATION_NUMERICAL_GREEN calls=" << integration::calls
                  << " qualified_calls=" << integration::qualified_calls
                  << " negative_draft_calls=" << integration::negative_draft_calls
                  << " verification_graphs=" << integration::verify_graphs
                  << " dense_graphs=" << integration::dense_graphs << " scalar_graphs=" << integration::scalar_graphs
                  << " token_graphs=" << integration::token_graphs
                  << " prefill_graphs=" << integration::prompt_graphs << " synthetic_only=1\n";
        if (enforce && !integration::verify_graphs) {
            std::cerr << "WORK_RED public Greedy owner executed zero temporal verification graphs\n"; return 2;
        }
        return 0;
    } catch (const std::exception & error) { std::cerr << "FAIL " << error.what() << '\n'; return 1; }
}
