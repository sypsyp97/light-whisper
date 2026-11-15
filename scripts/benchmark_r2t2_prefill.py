"""Warmed, subprocess-isolated ABBA/BAAB comparison of real R2T2 CUDA DLLs.

A=retained baseline, B=candidate. Each block loads one model, warms Chinese,
English and rolling audio, then measures repeats with that same resident model.
Whole feed/start/finish timings come from test_r2t2_native_cache.run_case; this
does not instrument or infer native prefill time. No GPU work runs with --help.
"""
import argparse
import ctypes
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

import numpy as np

import test_r2t2_native_cache as regression


ROOT = Path(__file__).resolve().parents[1]
WARM_CASES = ("zh-full", "en-full", "zh-rolling-36s")
CONFIG = {"backend": "cuda", "rolling": True, "chunk_samples": regression.CHUNK_SAMPLES,
          "sample_rate": regression.SAMPLE_RATE, "finish_context_samples": regression.FINISH_SAMPLES}


def now():
    return datetime.now(timezone.utc).isoformat()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n", encoding="utf-8")


def gpu_snapshot():
    executable = shutil.which("nvidia-smi")
    result = {"created_utc": now(), "status": "unavailable", "commands": []}
    if executable is None:
        result["error"] = "nvidia-smi not found on this process PATH"
        return result
    queries = (("gpu", "--query-gpu=index,uuid,name,driver_version,pstate,temperature.gpu,power.draw,"
                "clocks.current.sm,clocks.current.memory,utilization.gpu,memory.used,memory.total"),
               ("processes", "--query-compute-apps=pid,process_name,used_memory"))
    for name, query in queries:
        command = [executable, query, "--format=csv,noheader,nounits"]
        try:
            observed = subprocess.run(command, capture_output=True, text=True, errors="replace", timeout=15)
            result["commands"].append({"name": name, "command": command, "returncode": observed.returncode,
                                       "stdout": observed.stdout, "stderr": observed.stderr})
        except (OSError, subprocess.TimeoutExpired) as error:
            result["commands"].append({"name": name, "command": command, "error": str(error)})
    result["status"] = "available" if all(row.get("returncode") == 0 for row in result["commands"]) else "partial"
    return result


def summarize(rows):
    summary = {}
    for name in dict.fromkeys(row["case"] for row in rows):
        selected = [row for row in rows if row["case"] == name]
        feeds = [ms for row in selected for ms in row["result"]["timings"]["feed_ms"]]
        timings = [row["result"]["timings"] for row in selected]
        summary[name] = {
            "runs": len(selected), "feeds": len(feeds),
            "feed_p50_ms": float(np.percentile(feeds, 50)),
            "feed_p95_ms": float(np.percentile(feeds, 95)),
            "feed_mean_ms": float(np.mean(feeds)),
            "feed_max_ms": max(feeds),
            "start_p50_ms": float(np.median([row["start_ms"] for row in timings])),
            "finish_p50_ms": float(np.median([row["finish_or_reset_ms"] for row in timings])),
            "stream_calls_total_p50_ms": float(np.median([
                row["start_ms"] + sum(row["feed_ms"]) + row["finish_or_reset_ms"] for row in timings])),
            "stream_wall_p50_ms": float(np.median([row["wall_ms"] for row in selected])),
        }
    return summary


def ratios(blocks):
    summaries = {variant: summarize([row for block in blocks if block["variant"] == variant
                                     for row in block["measured"]]) for variant in "AB"}
    return {"variants": summaries, "candidate_over_baseline": {
        case: {metric: summaries["B"][case][metric] / summaries["A"][case][metric]
               for metric in ("feed_p50_ms", "feed_p95_ms", "feed_mean_ms", "start_p50_ms",
                              "finish_p50_ms", "stream_calls_total_p50_ms", "stream_wall_p50_ms")}
        for case in summaries["A"]}}


def worker(spec_path):
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    output_path = Path(spec["output"])
    regression.require(not output_path.exists(), "worker output exists; refusing to overwrite")
    output = {"schema": 1, "status": "running", "created_utc": now(), "pid": os.getpid(),
              "variant": spec["variant"], "round": spec["round"], "block": spec["block"],
              "config": CONFIG, "provenance": {}, "warmup": [], "measured": []}
    save(output_path, output)
    native = None
    directory_handles, preloaded = [], []
    try:
        regression.require(os.name == "nt", "this benchmark requires Windows CUDA DLLs")
        reference = json.loads(Path(spec["reference"]).read_text(encoding="utf-8"))
        args = argparse.Namespace(zh_wav=Path(spec["zh_wav"]), en_wav=Path(spec["en_wav"]),
                                  fixture="all", case=None)
        cases, fixtures = regression.make_cases(args)
        output["provenance"] = {"model": spec["model"], "library": spec["library"],
            "dependencies": spec["dependencies"], "fixtures": fixtures,
            **{key: regression.sha256(path) for key, path in {
                "model_sha256": spec["model"], "library_sha256": spec["library"],
                "reference_sha256": spec["reference"], "benchmark_sha256": __file__,
                "native_wrapper_sha256": regression.RESOURCES / "r2t2_native.py",
                "stream_wrapper_sha256": regression.RESOURCES / "r2t2_stream.py",
                "script_sha256": regression.__file__}.items()}}
        for key, expected in spec["expected_sha256"].items():
            regression.require(output["provenance"][key] == expected, f"{key}: input changed before worker")
        regression.check_reference_inputs(reference, {"schema": 1, "config": CONFIG,
                                                     "provenance": output["provenance"]})
        regression.require(len(reference["cases"]) == 13 and sum(
            len(case["feeds"]) for case in reference["cases"].values()) == 508,
            "expected frozen 13-case/508-feed recognition reference")
        for name in dict.fromkeys((*WARM_CASES, *spec["cases"])):
            pcm, completion = cases[name]
            expected = reference["cases"][name]
            regression.require(expected["pcm_samples"] == int(pcm.size) and
                expected["pcm_sha256"] == hashlib.sha256(pcm.tobytes()).hexdigest() and
                expected["completion"] == completion, f"{name}: frozen fixture recipe changed")

        sys.path.insert(0, str(regression.RESOURCES))
        from r2t2_native import NativeRuntime
        from r2t2_stream import R2T2StreamSession
        regression.require(NativeRuntime.FINISH_CONTEXT_SAMPLES == regression.FINISH_SAMPLES,
                           "original finish padding changed")
        for directory in spec["dependencies"]:
            directory_handles.append(os.add_dll_directory(directory))
        output["gpu_before_load"] = gpu_snapshot()
        output["preloaded_dependencies"] = []
        before = time.perf_counter()

        for name in ("cublasLt64_12.dll", "cublas64_12.dll", "cudart64_12.dll"):
            path = next((Path(directory) / name for directory in spec["dependencies"]
                         if (Path(directory) / name).is_file()), None)
            regression.require(path is not None, f"CUDA dependency missing from --dependencies: {name}")
            preloaded.append(ctypes.WinDLL(str(path)))
            output["preloaded_dependencies"].append({"path": str(path), "sha256": regression.sha256(path)})
        output["dependency_preload_ms"] = (time.perf_counter() - before) * 1000
        before = time.perf_counter()
        native = NativeRuntime(spec["model"], spec["library"], backend="cuda", rolling=True,
                               chunk_ms=160, dll_directories=spec["dependencies"])
        output["model_load_ms"] = (time.perf_counter() - before) * 1000
        session = R2T2StreamSession(native)
        session_id = 0

        def observe(name, repeat, target):
            nonlocal session_id
            session_id += 1
            before = time.perf_counter()
            result = regression.run_case(native, session, session_id, name, *cases[name])
            row = {"case": name, "repeat": repeat, "wall_ms": (time.perf_counter() - before) * 1000,
                   "result": result, "quality": "checking"}
            target.append(row)
            save(output_path, output)
            regression.compare(reference, {"cases": {name: result}, "tail_pairs": {}})
            row["quality"] = "exact_reference_match"
            save(output_path, output)

        for name in WARM_CASES:
            observe(name, None, output["warmup"])
        output["gpu_before_measurement"] = gpu_snapshot()
        for repeat in range(spec["repeats"]):
            for name in spec["cases"]:
                observe(name, repeat, output["measured"])
        output["gpu_after_measurement"] = gpu_snapshot()
        output["summary"] = summarize(output["measured"])
        output["status"] = "pass"
    except BaseException as error:
        output["status"] = "failed"
        output["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        try:
            if native is not None:
                native.close()
        except BaseException as error:
            output["status"] = "failed"
            output["cleanup_error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            for handle in directory_handles:
                handle.close()
            output["completed_utc"] = now()
            save(output_path, output)
    print(f"PASS worker {spec['variant']} round={spec['round']} block={spec['block']}", flush=True)


def controller(args):
    for name in ("candidate_library", "model", "dependencies", "output_dir"):
        regression.require(getattr(args, name), f"--{name.replace('_', '-')} is required")
    regression.require(args.rounds > 0 and args.repeats > 0, "--rounds/--repeats must be positive")
    output_dir = args.output_dir.resolve()
    regression.require(not output_dir.exists(), "output directory exists; choose a new directory")
    paths = {"A": args.baseline_library.resolve(), "B": args.candidate_library.resolve()}
    for path in (*paths.values(), args.model, args.reference, args.zh_wav, args.en_wav):
        regression.require(path.is_file(), f"input file missing: {path}")
    dependencies = [path.resolve() for path in args.dependencies]
    regression.require(all(path.is_dir() for path in dependencies), "dependency directory missing")
    cases = list(dict.fromkeys(args.case or WARM_CASES))
    output_dir.mkdir(parents=True)
    output_path = output_dir / "benchmark.json"
    model_sha = regression.sha256(args.model)
    reference_sha = regression.sha256(args.reference)
    library_sha = {key: regression.sha256(path) for key, path in paths.items()}
    output = {"schema": 1, "status": "running", "created_utc": now(), "A": "baseline", "B": "candidate",
              "rounds": args.rounds, "repeats_per_block": args.repeats, "cases": cases,
              "warmup_cases_per_block": list(WARM_CASES), "blocks": [], "round_summaries": [],
              "timing_scope": "actual stream start/feed/finish calls; no native prefill timer",
              "provenance": {"model_sha256": model_sha, "reference_sha256": reference_sha,
                             "library_sha256": library_sha, "benchmark_sha256": regression.sha256(__file__)}}
    save(output_path, output)
    process = None
    completed = []
    try:
        for round_id in range(args.rounds):
            order = args.order if round_id % 2 == 0 else args.order.translate(str.maketrans("AB", "BA"))
            round_blocks = []
            for position, variant in enumerate(order):
                block_id = len(output["blocks"])
                stem = f"block-{block_id:02d}-round-{round_id:02d}-{variant}"
                spec_path, worker_path, log_path = (output_dir / f"{stem}{suffix}"
                                                    for suffix in (".spec.json", ".json", ".log"))
                spec = {"variant": variant, "round": round_id, "block": block_id,
                        "library": str(paths[variant]), "model": str(args.model.resolve()),
                        "reference": str(args.reference.resolve()), "dependencies": list(map(str, dependencies)),
                        "zh_wav": str(args.zh_wav.resolve()), "en_wav": str(args.en_wav.resolve()),
                        "cases": cases, "repeats": args.repeats, "output": str(worker_path),
                        "expected_sha256": {"model_sha256": model_sha, "reference_sha256": reference_sha,
                                            "library_sha256": library_sha[variant],
                                            "benchmark_sha256": output["provenance"]["benchmark_sha256"]}}
                save(spec_path, spec)
                command = [sys.executable, "-u", str(Path(__file__).resolve()), "--worker-spec", str(spec_path)]
                block = {"round": round_id, "order": order, "position": position, "variant": variant,
                         "worker_json": str(worker_path), "log": str(log_path), "command": command,
                         "gpu_before_process": gpu_snapshot(), "status": "running"}
                output["blocks"].append(block)
                save(output_path, output)
                env = os.environ.copy()
                env["PATH"] = os.pathsep.join(map(str, dependencies)) + os.pathsep + env.get("PATH", "")
                env["PYTHONIOENCODING"] = "utf-8"
                print(f"BLOCK {block_id} round={round_id} order={order} variant={variant}", flush=True)
                with log_path.open("w", encoding="utf-8") as log:
                    process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
                    block["pid"] = process.pid
                    save(output_path, output)
                    block["returncode"] = process.wait()
                process = None
                block["gpu_after_process"] = gpu_snapshot()
                result = json.loads(worker_path.read_text(encoding="utf-8")) if worker_path.is_file() else None
                block["status"] = result["status"] if result else "worker_output_missing"
                save(output_path, output)
                regression.require(block["returncode"] == 0 and result is not None and result["status"] == "pass",
                    f"{stem}: subprocess/quality failure; {result.get('error', '') if result else 'no worker JSON'}; log={log_path}")
                completed.append(result)
                round_blocks.append(result)
            output["round_summaries"].append({"round": round_id, "order": order, **ratios(round_blocks)})
            save(output_path, output)
        output["aggregate"] = ratios(completed)
        output["status"] = "pass"
    except BaseException as error:
        output["status"] = "failed"
        output["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        output["completed_utc"] = now()
        save(output_path, output)
    print(f"PASS blocks={len(completed)} output={output_path}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-library", type=Path,
                        default=ROOT / "src-tauri/resources/r2t2-native/cuda/audiocpp.dll")
    parser.add_argument("--candidate-library", type=Path)
    parser.add_argument("--model", type=Path)
    parser.add_argument("--dependencies", type=Path, action="append", help="CUDA DLL directory; repeatable; child PATH only")
    parser.add_argument("--reference", type=Path, default=ROOT / "tmp/asr-engine-20261017/r2t2-stream-tests/baseline.json")
    parser.add_argument("--zh-wav", type=Path, default=regression.DEFAULT_ZH)
    parser.add_argument("--en-wav", type=Path, default=regression.DEFAULT_EN)
    parser.add_argument("--rounds", type=int, default=2, help="Four blocks per round; odd rounds swap ABBA/BAAB")
    parser.add_argument("--repeats", type=int, default=3, help="Measured case sweeps per resident model block")
    parser.add_argument("--order", choices=("ABBA", "BAAB"), default="ABBA", help="First round order")
    parser.add_argument("--case", choices=WARM_CASES, action="append", help="Measured subset; all three always warm up")
    parser.add_argument("--output-dir", type=Path, help="New directory for summary, worker JSONs and logs")
    parser.add_argument("--worker-spec", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker_spec:
        worker(args.worker_spec)
    else:
        controller(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
