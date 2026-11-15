"""Measure repeated Qwen offline inference through one audio.cpp C ABI session.

One warmup per fixture precedes at least three ordered measured sweeps. Only
audiocpp_session_run is timed. Raw WAV input bypasses product VAD/cropping;
packaged incumbent results are text context only, never a latency reference.
Run baseline/candidate captures in separate fresh processes; --reference points
to a prior report.json from this helper. Text differences remain visible.
"""
import argparse
from array import array
from collections import Counter
from contextlib import ExitStack
import ctypes as ct
from datetime import datetime, timezone
import json
from pathlib import Path
import statistics
import sys
import time

from test_shared_asr_native import (
    BackendConfig, CheckFailure, FakeAbi, ModelConfig, SAMPLE_RATE,
    load_abi, read_wav, require, sha256,
)


ROOT = Path(__file__).resolve().parents[1]
SAMPLES = ROOT / "tmp/asr-engine-20261017/upstream/samples"
FIXTURE_ROOT = ROOT / "tmp/shared-native-20261017/fixtures"
FIXTURES = {
    "qwen_zh_4s": ROOT / "tmp/asr-caption-20261017/fixtures/asr_zh.wav",
    "fleurs_zh": SAMPLES / "fleurs-zh.wav",
    "fleurs_en": SAMPLES / "fleurs-en.wav",
    "jfk": SAMPLES / "jfk.wav",
    "qwen_zh_repeat_16s": FIXTURE_ROOT / "qwen_zh_repeat_16s.wav",
    "qwen_zh_repeat_32s": FIXTURE_ROOT / "qwen_zh_repeat_32s.wav",
    "silence_1s": FIXTURE_ROOT / "silence_1s.wav",
}
SCHEMA = "qwen-shared-native-warm-v1"
REQUEST_OPTIONS = {"return_timestamps": "false", "audio_chunk_mode": "none", "max_tokens": "512"}
INCUMBENT = ROOT / "tmp/shared-native-20261017/incumbent-offline.json"


def fixture_inputs(names):
    pcms, metadata = {}, {}
    for name in names:
        path = FIXTURES[name]
        pcm = read_wav(path)
        pcms[name] = pcm
        metadata[name] = {"name": path.name, "sha256": sha256(path), "samples": len(pcm),
                          "sample_rate": SAMPLE_RATE, "channels": 1, "pcm_type": "float32",
                          "duration_seconds": len(pcm) / SAMPLE_RATE,
                          "silent": all(sample == 0 for sample in pcm)}
    return pcms, metadata


def incumbent_context():
    if not INCUMBENT.is_file():
        return {"available": False, "latency_comparable": False}
    source = json.loads(INCUMBENT.read_text(encoding="utf-8"))
    texts = {}
    for record in source["records"]:
        response = record.get("response", {})
        text = response.get("raw_text", response.get("text"))
        if isinstance(text, str):
            bucket = texts.setdefault(record["fixture"], [])
            if text not in bucket:
                bucket.append(text)
    return {"available": True, "name": INCUMBENT.name, "sha256": sha256(INCUMBENT),
            "latency_comparable": False, "raw_texts_by_fixture": texts,
            "reason": "Packaged incumbent includes product server/VAD/cropping and a different model package; native runs use full raw WAV input."}


def validate_reference(reference, report):
    require(reference["schema"] == SCHEMA and reference["status"] == "complete",
            "reference must be a completed capture from this helper")
    for field in ("config", "fixture_order", "fixtures", "script_sha256", "cabi_helper_sha256"):
        require(reference[field] == report[field], f"reference {field} differs; latency/text inputs are not identical")
    require(reference["model"]["sha256"] == report["model"]["sha256"], "reference model SHA256 differs")
    require(reference["model"]["bytes"] == report["model"]["bytes"], "reference model size differs")
    for name in report["fixture_order"]:
        measured = [run for run in reference["runs"] if run["fixture"] == name and run["stage"] == "measured"]
        require(len(measured) >= 3 and all(run["native_status"] == 0 for run in measured),
                f"reference lacks three successful measured runs for {name}")


def run_request(api, session, args, pcm, name, stage, sweep, report):
    with ExitStack() as handles:
        request = ct.c_void_p()
        handles.callback(api["request_free"], request)
        request.value = api["request_create"]()
        require(bool(request), "request_create returned a null handle")
        if args.language:
            api.call("request_set_text", request, b"", args.language.encode("utf-8"))
        for key, value in REQUEST_OPTIONS.items():
            api.call("request_set_option", request, key.encode("utf-8"), value.encode("utf-8"))
        samples = (ct.c_float * len(pcm)).from_buffer(pcm)
        api.call("request_set_audio", request, samples, len(pcm), SAMPLE_RATE, 1)
        result = ct.c_void_p()
        handles.callback(api["result_free"], result)
        record = {"fixture": name, "stage": stage, "sweep": sweep,
                  "session_index": 0, "generated_token_ids": None}

        report["runs"].append(record)
        report["stage"] = {"operation": "session_run", "fixture": name, "run_stage": stage, "sweep": sweep}
        start = time.perf_counter_ns()
        status = api["session_run"](session, request, ct.byref(result))
        elapsed_ns = time.perf_counter_ns() - start
        record.update(session_run_ms=elapsed_ns / 1_000_000, native_status=status)
        if status:
            detail = (api["last_error"]() or b"").decode("utf-8", errors="replace")
            raise CheckFailure(f"audiocpp_session_run failed ({status}): {detail}", "native_runtime",
                               native_function="session_run", native_status=status)
        require(bool(result), "session_run returned a null result")
        text, language = ct.c_char_p(), ct.c_char_p()
        report["stage"]["operation"] = "result_text"
        text_status = api["result_text"](result, ct.byref(text), ct.byref(language))
        if text_status not in (0, 7):
            detail = (api["last_error"]() or b"").decode("utf-8", errors="replace")
            raise CheckFailure(f"audiocpp_result_text failed ({text_status}): {detail}", "native_runtime",
                               native_function="result_text", native_status=text_status)
        record.update(text=(text.value or b"").decode("utf-8") if text_status == 0 else None,
                      language=(language.value or b"").decode("utf-8") or None,
                      text_status=text_status)
        return record


def benchmark(api, args, pcms, report, save=lambda: None):
    version = api["abi_version"]()
    require(version >> 16 == 0 and ((version >> 8) & 255) >= 4,
            f"expected audio.cpp ABI >= 0.4, got {version:#x}")
    report["abi"] = {"packed_version": version, "build_version": (api["build_version"]() or b"").decode("utf-8")}
    with ExitStack() as handles:
        def owned(kind):
            handle = ct.c_void_p()
            handles.callback(api[kind + "_free"], handle)
            return handle

        def setup_call(name, *arguments):
            report["stage"] = {"operation": name}
            start = time.perf_counter_ns()
            try:
                api.call(name, *arguments)
            finally:
                report["setup_ms"][name] = (time.perf_counter_ns() - start) / 1_000_000

        registry = owned("registry")
        setup_call("registry_create", None, ct.byref(registry))
        require(bool(registry), "registry_create returned a null handle")
        model = owned("model")
        model_config = ModelConfig(b"qwen3_asr", None, None, None)
        setup_call("model_load", registry, str(args.model.resolve()).encode("utf-8"),
                   ct.byref(model_config), None, ct.byref(model))
        require(bool(model), "model_load returned a null handle")
        require(api["model_family"](model) == b"qwen3_asr", "loaded family is not qwen3_asr")
        require(api["model_supports"](model, b"asr", b"offline") == 1, "model does not support offline ASR")
        session = owned("session")
        backend = BackendConfig(args.backend.encode("ascii"), 0, args.threads)
        setup_call("session_create", model, b"asr", b"offline", ct.byref(backend), None, ct.byref(session))
        require(bool(session), "session_create returned a null handle")
        report["session_count"] = 1
        for name in report["fixture_order"]:
            run_request(api, session, args, pcms[name], name, "warmup", None, report)
            save()
        for sweep in range(args.repeats):
            for name in report["fixture_order"]:
                record = run_request(api, session, args, pcms[name], name, "measured", sweep, report)
                save()
                print(f"MEASURED sweep={sweep} fixture={name} session_run_ms={record['session_run_ms']:.3f}", flush=True)


def summarize(report, reference):
    report["summary"] = {}
    report["comparison"] = {} if reference else None
    incumbent = report["incumbent_context"].get("raw_texts_by_fixture", {})
    for name in report["fixture_order"]:
        records = [run for run in report["runs"] if run["fixture"] == name and run["stage"] == "measured"]
        times = [run["session_run_ms"] for run in records]
        texts = list(dict.fromkeys(run["text"] for run in records))
        languages = list(dict.fromkeys(run["language"] for run in records))
        silent = report["fixtures"][name]["silent"]
        diagnostics = []
        if silent and any(text and text.strip() for text in texts):
            diagnostics.append("raw_silence_produced_text; bare native call bypasses product VAD")
        if not silent and any(text is None or not text.strip() for text in texts):
            diagnostics.append("speech_fixture_missing_or_empty_text")
        if len(texts) > 1 or len(languages) > 1:
            diagnostics.append("repeated_native_output_changed")
        context_texts = incumbent.get(name)
        report["summary"][name] = {
            "measured_runs": len(records), "session_run_ms": times,
            "median_ms": statistics.median(times), "min_ms": min(times), "max_ms": max(times),
            "raw_texts": texts, "languages": languages, "output_stable": len(texts) == 1 and len(languages) == 1,
            "diagnostics": diagnostics, "incumbent_text_context": {
                "available": context_texts is not None, "incumbent_raw_texts": context_texts,
                "exact_text_match": texts == context_texts if context_texts is not None else None,
                "semantic_quality_verified": False,
                "note": "Raw text differences, including orthography, remain unresolved context; no common-input quality or latency claim."},
        }
        if reference:
            before = [run for run in reference["runs"] if run["fixture"] == name and run["stage"] == "measured"]
            old_times = [run["session_run_ms"] for run in before]
            old_median = statistics.median(old_times)
            old_texts = list(dict.fromkeys(run["text"] for run in before))
            old_languages = list(dict.fromkeys(run["language"] for run in before))
            stable = len(old_texts) == len(texts) == 1 and len(old_languages) == len(languages) == 1
            exact = stable and old_texts == texts and old_languages == languages
            report["comparison"][name] = {
                "reference_session_run_ms": old_times, "candidate_session_run_ms": times,
                "reference_median_ms": old_median, "candidate_median_ms": statistics.median(times),
                "median_change_percent": (statistics.median(times) / old_median - 1) * 100 if old_median > 0 else None,
                "baseline_raw_texts": old_texts, "candidate_raw_texts": texts,
                "baseline_languages": old_languages, "candidate_languages": languages,
                "both_outputs_stable": stable, "exact_text_and_language_match": exact,
                "quality_scope": "exact measured native transcript regression only; no independent semantic ASR quality proof",
            }


def self_test():
    args = argparse.Namespace(model=Path("test-only.gguf"), backend="cpu", threads=1, language=None, repeats=3)
    for failure in (None, "model_load", "session_create", "request_set_audio", "session_run", "result_text"):
        api = FakeAbi(failure)
        report = {"fixture_order": ["test-only"], "runs": [], "setup_ms": {}}
        failed = False
        try:
            benchmark(api, args, {"test-only": array("f", [0.5])}, report)
        except CheckFailure:
            failed = True
        require(failed == (failure is not None), "fake failure did not reach expected branch")
        require(Counter(api.allocated) == Counter(api.freed), "native handle leaked or freed twice")
        if failure is None:
            require(len(report["runs"]) == 4 and report["session_count"] == 1,
                    "expected one warmup and three measured sweeps using one session")
            require([run["stage"] for run in report["runs"]] == ["warmup", "measured", "measured", "measured"],
                    "warmup entered measured sweep")
    print("PASS fake ABI: one-session sweep schedule and cleanup across five injected failures; no native library loaded")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("library", "model", "output-dir", "reference"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--dependencies", type=Path, action="append", default=[])
    parser.add_argument("--backend", choices=("cpu", "cuda"), default="cuda")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--language", help="Omit for automatic language detection")
    parser.add_argument("--fixture", action="append", choices=tuple(FIXTURES), help="Optional fixture subset; may repeat")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    for name in ("library", "model", "output_dir"):
        if getattr(args, name) is None:
            parser.error(f"--{name.replace('_', '-')} is required")
    if args.repeats < 3 or args.threads < 1:
        parser.error("--repeats must be >=3 and --threads must be positive")
    if args.output_dir.exists():
        parser.error("--output-dir must be fresh")
    args.output_dir.mkdir(parents=True, exist_ok=False)
    report_path = args.output_dir / "report.json"
    report = {"schema": SCHEMA, "created_utc": datetime.now(timezone.utc).isoformat(),
              "status": "running", "stage": {"operation": "inputs"}, "runs": [], "setup_ms": {}}

    def save():
        report_path.write_text(json.dumps(report, ensure_ascii=False, allow_nan=False, indent=2) + "\n", encoding="utf-8")

    try:
        selected = args.fixture or list(FIXTURES)
        if len(selected) != len(set(selected)):
            raise ValueError("duplicate --fixture selection")
        pcms, fixtures = fixture_inputs(selected)
        report.update(
            config={"family": "qwen3_asr", "task": "asr", "mode": "offline", "backend": args.backend,
                    "device": 0, "threads": args.threads, "language": args.language,
                    "request_options": REQUEST_OPTIONS},
            schedule={"warmups_per_fixture": 1, "measured_sweeps": args.repeats,
                      "order": "all fixture warmups, then ordered full-fixture sweeps", "session_policy": "one resident session"},
            measurement={"timed_operation": "audiocpp_session_run", "timer": "perf_counter_ns",
                         "input_stage": "raw full WAV; no product VAD, crop, padding or text normalization",
                         "warm_state": "same session after each fixture warmup; changing fixture shapes may rebuild native graphs",
                         "excluded_from_timing": ["hashing", "model load", "session creation", "request creation",
                                                  "audio upload to request", "result text readout", "JSON serialization"],
                         "generated_token_ids_available": False,
                         "token_ids_reason": "Public audio.cpp ABI 0.4 exposes transcript text but no generated-token ID accessor"},
            fixture_order=selected, fixtures=fixtures,
            library={"name": args.library.name, "sha256": sha256(args.library)},
            model={"name": args.model.name, "sha256": sha256(args.model), "bytes": args.model.stat().st_size},
            script_sha256=sha256(Path(__file__)), cabi_helper_sha256=sha256(ROOT / "scripts/test_shared_asr_native.py"),
            incumbent_context=incumbent_context(),
            reference={"name": args.reference.name, "sha256": sha256(args.reference)} if args.reference else None,
        )
        reference = json.loads(args.reference.read_text(encoding="utf-8")) if args.reference else None
        if reference:
            validate_reference(reference, report)
        save()
        with load_abi(args.library, args.dependencies) as api:
            benchmark(api, args, pcms, report, save)
        summarize(report, reference)
        report["status"], report["stage"] = "complete", {"operation": "complete"}
    except Exception as error:
        report["status"] = "failed"
        report["failure_kind"] = error.kind if isinstance(error, CheckFailure) else "infrastructure"
        report["error"] = str(error)
        if isinstance(error, CheckFailure):
            report.update(error.details)
    save()
    print(f"{report['status'].upper()} report={report_path}", flush=True)
    return 0 if report["status"] == "complete" else 1


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
