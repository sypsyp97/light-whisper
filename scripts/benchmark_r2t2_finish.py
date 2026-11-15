"""Real CUDA Stop comparison using isolated baseline/candidate Python wrappers.

The controller optionally captures a fresh reference with the baseline wrapper
and DLL, then runs sequential ABBA/BAAB blocks. Each block warms every selected
case once before at least three measured sweeps. No microphone or audio sleeps.
--self-test and --help do not load a model or DLL.
"""
import argparse
import copy
import ctypes
import hashlib
import importlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
from unittest.mock import Mock

import numpy as np

import benchmark_r2t2_prefill as shared
import test_r2t2_native_cache as regression


ROOT, CONFIG = regression.ROOT, shared.CONFIG
DEFAULT_CASES = shared.WARM_CASES
require, sha256, save, now = regression.require, regression.sha256, shared.save, shared.now
CUDA_DLLS = ("cublasLt64_12.dll", "cublas64_12.dll", "cudart64_12.dll")
METRICS = ("finish_p50_ms", "finish_p95_ms", "feed_p50_ms", "feed_p95_ms",
           "first_caption_compute_p50_ms")


def schedule(rounds, order):
    result = []
    for round_id in range(rounds):
        selected = order if round_id % 2 == 0 else order.translate(str.maketrans("AB", "BA"))
        result.extend((round_id, position, variant, selected) for position, variant in enumerate(selected))
    return result


def check_reference(reference, provenance, cases):
    require(reference["status"] == "pass" and reference["schema"] == 1,
            "reference must be a passing schema-1 capture")
    require(reference["config"] == CONFIG, "stream/padding configuration changed")
    require(reference["provenance"]["model_sha256"] == provenance["model_sha256"],
            "model identity changed")
    for language, fixture in provenance["fixtures"].items():
        previous = reference["provenance"]["fixtures"][language]
        require(all(previous[key] == fixture[key] for key in ("sha256", "samples")),
                f"{language}: audio identity changed")

    for name, (pcm, completion) in cases.items():
        require(name in reference["cases"], f"{name}: missing reference case")
        expected = reference["cases"][name]
        require(expected["pcm_samples"] == int(pcm.size) and
                expected["pcm_sha256"] == hashlib.sha256(pcm.tobytes()).hexdigest() and
                expected["completion"] == completion, f"{name}: PCM/case recipe changed")


def first_caption(result):
    elapsed = result["timings"]["start_ms"]
    for index, feed in enumerate(result["feeds"]):
        elapsed += feed["ms"]
        if feed["preview"]:
            return {"phase": "feed", "feed_index": index, "compute_ms": elapsed,
                    "audio_samples": feed["input_offset"] + feed["input_samples"]}
    final = result["final"]
    if final is not None and final["text"]:
        return {"phase": "finish", "feed_index": None,
                "compute_ms": elapsed + result["timings"]["finish_or_reset_ms"],
                "audio_samples": result["pcm_samples"]}
    return None


def summarize(rows):
    summary = shared.summarize(rows)
    for name, values in summary.items():
        selected = [row for row in rows if row["case"] == name]
        values["finish_p95_ms"] = float(np.percentile([
            row["result"]["timings"]["finish_or_reset_ms"] for row in selected], 95))
        captions = [row["first_nonempty"] for row in selected if row["first_nonempty"] is not None]
        values["first_caption_compute_p50_ms"] = (float(np.median([
            row["compute_ms"] for row in captions])) if captions else None)
        values["first_caption_audio_samples"] = [row["audio_samples"] for row in captions]
    return summary


def ratios(blocks):
    variants = {variant: summarize([row for block in blocks if block["variant"] == variant
                                    for row in block["measured"]]) for variant in "AB"}
    return {"variants": variants, "candidate_over_baseline": {
        case: {metric: (variants["B"][case][metric] / variants["A"][case][metric]
                        if variants["A"][case][metric] else None) for metric in METRICS}
        for case in variants["A"]}}


def retention(summaries, cases):
    primary = {"zh-full", "en-full"}
    checks = []
    for scope, summary in summaries:
        for name, ratio in summary["candidate_over_baseline"].items():
            checks.append({"scope": scope, "case": name,
                           "stop_target": ratio["finish_p50_ms"] <= 0.90 if name in primary else None,
                           "p95_and_first_caption_guard": all(
                               ratio[key] is None or ratio[key] <= 1.05 for key in (
                                   "finish_p95_ms", "feed_p95_ms", "first_caption_compute_p50_ms"))})
    complete = primary.issubset(cases) and len(summaries) >= 3
    return {"complete_two_round_primary_measurement": complete, "checks": checks,
            "timing_gate_pass": complete and all(row["stop_target"] is not False and
                                                  row["p95_and_first_caption_guard"] for row in checks),
            "scope": "raw native stream; production VAD/segmented smoke remains required"}


def stop_child(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


def run_child(command, env, log_path, timeout, factory=subprocess.Popen):

    with log_path.open("x", encoding="utf-8") as log:
        process = factory(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            result = {"pid": process.pid, "timed_out": False}
            try:
                result["returncode"] = process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                result.update(returncode=None, timed_out=True)
        finally:
            stop_child(process)
    return result


def worker(spec_path):
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    output_path = Path(spec["output"])
    require(not output_path.exists(), "worker output exists; refusing overwrite")
    output = {"schema": 1, "status": "running", "created_utc": now(), "pid": os.getpid(),
              "variant": spec["variant"], "config": CONFIG, "provenance": {},
              "warmup": [], "measured": []}
    save(output_path, output)
    native, handles, preloaded = None, [], []
    try:
        require(os.name == "nt", "real benchmark requires Windows CUDA DLLs")
        for path, expected in spec["hashes"].items():
            require(sha256(path) == expected, f"input changed before worker: {path}")
        cases, fixtures = regression.make_cases(argparse.Namespace(
            zh_wav=Path(spec["zh_wav"]), en_wav=Path(spec["en_wav"]), fixture="all", case=spec["cases"]))
        output["provenance"] = spec["provenance"] | {"fixtures": fixtures}
        reference = (json.loads(Path(spec["reference"]).read_text(encoding="utf-8"))
                     if spec["reference"] else None)
        if reference is not None:
            check_reference(reference, output["provenance"], cases)
        sys.path[:0] = [spec["source_dir"], str(regression.RESOURCES)]
        native_module, stream_module = (importlib.import_module(name) for name in ("r2t2_native", "r2t2_stream"))
        for module, key in ((native_module, "native_wrapper"), (stream_module, "stream_wrapper")):
            require(Path(module.__file__).resolve() == Path(spec["provenance"][key]),
                    f"wrong imported source: {module.__file__}")
        require(native_module.NativeRuntime.FINISH_CONTEXT_SAMPLES == regression.FINISH_SAMPLES,
                "original 320 ms context changed")
        for directory in spec["dependencies"]:
            handles.append(os.add_dll_directory(directory))
        output["gpu_before_load"] = shared.gpu_snapshot()
        for path in spec["cuda_dependencies"]:
            preloaded.append(ctypes.WinDLL(path))
        before = time.perf_counter()
        native = native_module.NativeRuntime(spec["model"], spec["library"], backend="cuda", rolling=True,
                                            chunk_ms=160, dll_directories=spec["dependencies"])
        output["model_load_ms"] = (time.perf_counter() - before) * 1000
        session = stream_module.R2T2StreamSession(native)
        session_id = 0

        def observe(name, repeat, target):
            nonlocal session_id
            session_id += 1
            print(f"OBSERVE_BEGIN {session_id} {name} {repeat}", flush=True)
            before = time.perf_counter()
            result = regression.run_case(native, session, session_id, name, *cases[name])
            row = {"case": name, "repeat": repeat, "wall_ms": (time.perf_counter() - before) * 1000,
                   "result": result, "captured_count": result["pcm_samples"],
                   "native_padded_count": result["final_native_samples"],
                   "first_nonempty": first_caption(result), "quality": "checking"}
            target.append(row)
            save(output_path, output)
            if reference is not None:
                regression.compare(reference, {"cases": {name: result}, "tail_pairs": {}})
            row["quality"] = "exact_reference_match" if reference else "baseline_capture"
            print(f"OBSERVE_END {session_id}", flush=True)
            save(output_path, output)

        for name in cases:
            observe(name, None, output["warmup"])
        output["gpu_before_measurement"] = shared.gpu_snapshot()
        for repeat in range(spec["repeats"]):
            for name in cases:
                observe(name, repeat, output["measured"])
        output["summary"] = summarize(output["measured"])
        output["gpu_after_measurement"] = shared.gpu_snapshot()
        output["status"] = "pass"
    except BaseException as error:
        output.update(status="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        try:
            if native is not None:
                native.close()
        except BaseException as error:
            output.update(status="failed", cleanup_error=f"{type(error).__name__}: {error}")
            raise
        finally:
            for handle in handles:
                handle.close()
            output["completed_utc"] = now()
            save(output_path, output)


def controller(args):
    for key in ("baseline_source_dir", "candidate_source_dir", "baseline_library", "candidate_library",
                "model", "dependencies", "output_dir"):
        require(getattr(args, key), f"--{key.replace('_', '-')} is required")
    require(args.rounds > 0 and args.repeats >= 3 and args.worker_timeout > 0,
            "positive rounds/timeout and at least 3 measured sweeps required")
    sources = {key: path.resolve() for key, path in zip("AB", (args.baseline_source_dir, args.candidate_source_dir))}
    libraries = {key: path.resolve() for key, path in zip("AB", (args.baseline_library, args.candidate_library))}
    cases, fixtures = regression.make_cases(argparse.Namespace(
        zh_wav=args.zh_wav, en_wav=args.en_wav, fixture="all", case=args.case or list(DEFAULT_CASES)))
    dependencies = [path.resolve() for path in args.dependencies]
    require(all(path.is_dir() for path in dependencies), "dependency directory missing")
    cuda_paths = [next((directory / name for directory in dependencies if (directory / name).is_file()), None)
                  for name in CUDA_DLLS]
    require(all(cuda_paths), "explicit dependencies must provide cublasLt/cublas/cudart CUDA 12 DLLs")
    common = [args.model.resolve(), args.zh_wav.resolve(), args.en_wav.resolve(),
              Path(__file__).resolve(), Path(regression.__file__).resolve(), Path(shared.__file__).resolve(), *cuda_paths]
    if args.reference:
        common.append(args.reference.resolve())
    variant_paths = {}
    for variant in "AB":
        stream = sources[variant] / "r2t2_stream.py"

        if not stream.is_file():
            stream = regression.RESOURCES / "r2t2_stream.py"
        variant_paths[variant] = [sources[variant] / "r2t2_native.py", stream.resolve(), libraries[variant]]
    for path in {*common, *(path for paths in variant_paths.values() for path in paths)}:
        require(path.is_file(), f"input missing: {path}")
    hashes = {str(path): sha256(path) for path in {*common, *(path for paths in variant_paths.values() for path in paths)}}
    output_dir = args.output_dir.resolve()
    require(not output_dir.exists(), "output directory exists; refusing overwrite")
    output_dir.mkdir(parents=True)
    output_path = output_dir / "benchmark.json"
    output = {"schema": 1, "status": "running", "created_utc": now(), "config": CONFIG,
              "cases": list(cases), "repeats_per_block": args.repeats, "blocks": [], "round_summaries": [],
              "hashes": hashes, "timing_scope": "Stop=complete session.finish; first caption=start+feed compute sum",
              "wrapper_fallback": "missing frozen r2t2_stream.py resolves current resources; actual path/hash recorded"}
    save(output_path, output)
    reference_path = args.reference.resolve() if args.reference else None
    reference_sha = hashes[str(reference_path)] if reference_path else None
    completed = []

    def block(variant, stem, repeats):
        native_wrapper, stream_wrapper, library = variant_paths[variant]
        spec_path, worker_path, log_path = [output_dir / f"{stem}{suffix}" for suffix in (".spec.json", ".json", ".log")]
        provenance = {"model": str(args.model.resolve()), "model_sha256": hashes[str(args.model.resolve())],
                      "library": str(library), "library_sha256": hashes[str(library)],
                      "native_wrapper": str(native_wrapper), "native_wrapper_sha256": hashes[str(native_wrapper)],
                      "stream_wrapper": str(stream_wrapper), "stream_wrapper_sha256": hashes[str(stream_wrapper)],
                      "script_sha256": hashes[str(Path(regression.__file__).resolve())], "fixtures": fixtures,
                      "benchmark_sha256": hashes[str(Path(__file__).resolve())]}
        paths = [*common, *variant_paths[variant]]
        spec = {"variant": variant, "source_dir": str(sources[variant]), "library": str(library),
                "model": str(args.model.resolve()), "dependencies": list(map(str, dependencies)),
                "cuda_dependencies": list(map(str, cuda_paths)), "zh_wav": str(args.zh_wav.resolve()),
                "en_wav": str(args.en_wav.resolve()), "cases": list(cases), "repeats": repeats,
                "reference": str(reference_path) if reference_path else None, "output": str(worker_path),
                "provenance": provenance, "hashes": {str(path): hashes[str(path)] for path in paths}}
        if reference_path is not None:
            spec["hashes"][str(reference_path)] = reference_sha
        save(spec_path, spec)
        command = [sys.executable, "-B", "-u", str(Path(__file__).resolve()), "--worker-spec", str(spec_path)]
        row = {"variant": variant, "worker_json": str(worker_path), "log": str(log_path), "command": command,
               "status": "running", "gpu_before_process": shared.gpu_snapshot()}
        output["blocks"].append(row)
        save(output_path, output)
        env = os.environ.copy()
        env.update(PATH=os.pathsep.join(map(str, dependencies)) + os.pathsep + env.get("PATH", ""),
                   PYTHONIOENCODING="utf-8", PYTHONDONTWRITEBYTECODE="1")
        print(f"BLOCK {stem} source={sources[variant]} library={library}", flush=True)
        row.update(run_child(command, env, log_path, args.worker_timeout))
        result = json.loads(worker_path.read_text(encoding="utf-8")) if worker_path.is_file() else None
        row["status"] = "timeout" if row["timed_out"] else result["status"] if result else "worker_output_missing"
        counts = re.findall(r"confucius4_r2t2\.session\.stream\.chunks\s+(\d+)", log_path.read_text(encoding="utf-8", errors="replace"))
        row["native_decoder_counts"] = [int(value) for value in counts] if counts else None
        row["native_decoder_count_status"] = "native_debug_log" if counts else "logging unavailable through existing C ABI"
        row["gpu_after_process"] = shared.gpu_snapshot()
        save(output_path, output)
        require(row["returncode"] == 0 and result is not None and result["status"] == "pass",
                f"{stem}: worker/quality failure; log={log_path}; {result.get('error', '') if result else row['status']}")
        return result

    try:
        if reference_path is None:
            capture = block("A", "reference-capture", 1)
            reference_path = output_dir / "reference.json"
            captured_cases = {row["case"]: row["result"] for row in capture["measured"]}
            save(reference_path, {"schema": 1, "status": "pass", "config": CONFIG,
                                  "provenance": capture["provenance"], "cases": captured_cases,
                                  "tail_pairs": regression.tail_pairs(captured_cases)})
            reference_sha = sha256(reference_path)
            output["hashes"][str(reference_path)] = reference_sha
        output["reference"] = str(reference_path)
        for round_id, position, variant, order in schedule(args.rounds, args.order):
            result = block(variant, f"round-{round_id:02d}-{position}-{variant}", args.repeats)
            result["round"] = round_id
            completed.append(result)
            if position == 3:
                output["round_summaries"].append({"round": round_id, "order": order, **ratios(completed[-4:])})
                save(output_path, output)
        output["aggregate"] = ratios(completed)
        summaries = [(f"round-{row['round']}", row) for row in output["round_summaries"]] + [("aggregate", output["aggregate"])]
        output["retention"] = retention(summaries, cases)
        output["status"] = "pass"
    except BaseException as error:
        output.update(status="failed", error=f"{type(error).__name__}: {error}")
        raise
    finally:
        output["completed_utc"] = now()
        save(output_path, output)
    print(f"PASS quality blocks={len(completed)} timing_gate={output['retention']['timing_gate_pass']} output={output_path}")


def self_test():
    response = {"session_id": 1, "text": "好", "tentative_text": "呀", "language": "Chinese",
                "sample_count": 37, "final": False, "success": True}
    case = {"pcm_samples": 37, "pcm_sha256": "pcm", "completion": "finish",
            "feeds": [{"input_offset": 0, "input_samples": 37, "response": response,
                       "preview": "好呀", "native_samples": 37, "ms": 2.0}],
            "final": response | {"text": "好呀", "tentative_text": "", "final": True},
            "final_preview": "", "final_native_samples": 5157,
            "timings": {"start_ms": 1.0, "feed_ms": [2.0], "finish_or_reset_ms": 4.0}}
    reference = {"cases": {"test": case}, "tail_pairs": {}}
    regression.compare(reference, copy.deepcopy(reference))
    mutations = (
        lambda row: row["final"].update(text="changed"),
        lambda row: row["final"].update(language="English"),
        lambda row: row["final"].update(sample_count=36),
        lambda row: row["feeds"][0]["response"].update(tentative_text="changed"),
        lambda row: row.update(final_native_samples=4997),
    )
    for mutate in mutations:
        changed = copy.deepcopy(reference)
        mutate(changed["cases"]["test"])
        try:
            regression.compare(reference, changed)
        except AssertionError:
            pass
        else:
            raise AssertionError("quality drift escaped comparison")
    require(first_caption(case) == {"phase": "feed", "feed_index": 0, "compute_ms": 3.0,
                                    "audio_samples": 37}, "first caption metric wrong")
    require("".join(row[2] for row in schedule(2, "ABBA")) == "ABBABAAB", "ABBA/BAAB schedule wrong")
    require("".join(row[2] for row in schedule(2, "BAAB")) == "BAABABBA", "BAAB/ABBA schedule wrong")
    pcm = np.ones(37, dtype=np.float32)
    identity_reference = reference | {"schema": 1, "status": "pass", "config": CONFIG,
                                      "provenance": {"model_sha256": "model", "fixtures": {
                                          "zh": {"sha256": "wave", "samples": 37}},
                                          "native_wrapper_sha256": "baseline-wrapper"}}
    identity_reference = copy.deepcopy(identity_reference)
    identity_reference["cases"]["test"]["pcm_sha256"] = hashlib.sha256(pcm.tobytes()).hexdigest()
    provenance = identity_reference["provenance"] | {"native_wrapper_sha256": "candidate-wrapper"}
    check_reference(identity_reference, provenance, {"test": (pcm, "finish")})
    for changed in (provenance | {"model_sha256": "different-model"},
                    provenance | {"fixtures": {"zh": {"sha256": "different-wave", "samples": 37}}}):
        try:
            check_reference(identity_reference, changed, {"test": (pcm, "finish")})
        except AssertionError:
            pass
        else:
            raise AssertionError("model/audio identity drift accepted")
    with tempfile.TemporaryDirectory() as directory:
        process = Mock(pid=123, **{"poll.return_value": None})
        process.wait.side_effect = [subprocess.TimeoutExpired("test", 1), subprocess.TimeoutExpired("test", 10), 0]
        result = run_child(["fake-worker"], {}, Path(directory) / "worker.log", 1,
                           factory=Mock(return_value=process))
        require(result["timed_out"] and process.terminate.call_count == 1 and process.kill.call_count == 1,
                "timed-out worker not terminated/killed")
        process = Mock(pid=456, **{"poll.return_value": 0, "wait.return_value": 0})
        run_child(["fake-worker"], {}, Path(directory) / "normal.log", 1, factory=Mock(return_value=process))
        require(not process.terminate.called and not process.kill.called, "completed worker terminated")
        try:
            run_child(["fake-worker"], {}, Path(directory) / "normal.log", 1, factory=Mock())
        except FileExistsError:
            pass
        else:
            raise AssertionError("worker log overwrite allowed")
    print("PASS self-test: transcript/language/sample/live-trace/padding drift; strict model/audio identity with wrapper changes; first-caption metric; ABBA/BAAB; bounded worker cleanup; overwrite refusal")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("baseline-source-dir", "candidate-source-dir", "baseline-library", "candidate-library", "model", "reference", "output-dir"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--dependencies", type=Path, action="append", help="explicit CUDA DLL directory; repeatable")
    parser.add_argument("--zh-wav", type=Path, default=regression.DEFAULT_ZH)
    parser.add_argument("--en-wav", type=Path, default=regression.DEFAULT_EN)
    parser.add_argument("--case", action="append", help="existing regression recipe; default ZH/EN full + ZH rolling 36s")
    parser.add_argument("--rounds", type=int, default=2)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--order", choices=("ABBA", "BAAB"), default="ABBA")
    parser.add_argument("--worker-timeout", type=float, default=900, help="seconds per independent model worker")
    parser.add_argument("--worker-spec", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    elif args.worker_spec:
        worker(args.worker_spec)
    else:
        controller(args)


if __name__ == "__main__":
    main()
