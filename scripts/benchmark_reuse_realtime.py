"""Benchmark the real Real-time RE-USE DLL with deadline-paced 20/40 ms capture.

One first stream precedes --iterations steady streams on each loaded instance.
Loading, capture time, synchronous runtime.run calls, complete feed/flush work,
and observed output latency are recorded separately. No warmup or TF32 override
is applied. A subsequent unpaced real-DLL guard checks enhance versus irregular
feed/flush partitions and a repeat after resetting a partially consumed stream.
--capture-ms changes paced input chunks; native/STFT frames stay at 320 samples.
--threads sets the native and process-local BLAS/OMP thread count.
Lifecycle resource snapshots stay outside every measurement timer. Optional
--cudart queries device-wide memory after model loading; no leak verdict is made.
"""

import argparse
from contextlib import ExitStack
import ctypes as ct
from datetime import datetime, timezone
import gc
import hashlib
import importlib
import json
import math
import os
from pathlib import Path
import platform
import sys
import time

from benchmark_reuse import sha256, summary, timed

ROOT = Path(__file__).resolve().parents[1]
RATE, FFT, HOP, THREADS = 16_000, 640, 320, 2
GUARD_CHUNKS = (1, 319, 321, 641, 17, 960)
ENV_KEYS = ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS",
            "NUMEXPR_NUM_THREADS", "VECLIB_MAXIMUM_THREADS", "CUDA_VISIBLE_DEVICES",
            "NVIDIA_TF32_OVERRIDE", "REUSE_NATIVE_DLL_DIRS")


def pcm_sha256(audio):
    return hashlib.sha256(audio.tobytes()).hexdigest()


def file_record(path):
    path = Path(path).resolve()
    return {"path": str(path), "sha256": sha256(path)}


def output_checks(chunks, audio, input_sha256):
    import numpy as np

    output = np.concatenate(chunks)
    checks = {
        "sample_count_correct": len(output) == len(audio),
        "finite": bool(np.isfinite(output).all()),
        "float32_mono": all(value.dtype == np.float32 and value.ndim == 1 for value in chunks),
        "owned_chunks": all(value.flags.owndata for value in chunks),
        "independent_of_input": all(not np.shares_memory(value, audio) for value in chunks),
        "distinct_chunk_storage": len({value.ctypes.data for value in chunks if len(value)})
                                  == sum(bool(len(value)) for value in chunks),
        "input_unchanged": pcm_sha256(audio) == input_sha256,
    }
    return output, {"passed": all(checks.values()), **checks,
                    "samples": len(output), "expected_samples": len(audio),
                    "pcm_sha256": pcm_sha256(output)}


def compare(left, right):
    import numpy as np

    same_shape = left.shape == right.shape
    finite = bool(np.isfinite(left).all() and np.isfinite(right).all())
    return {"equal": bool(np.array_equal(left, right)),
            "left_pcm_sha256": pcm_sha256(left), "right_pcm_sha256": pcm_sha256(right),
            "max_abs_difference": float(np.max(np.abs(left.astype(np.float64) - right)))
                                  if same_shape and finite and len(left) else None}


def observed(enhancer, action):
    """Keep real buffers alive; inspect ownership after the measured action."""
    import numpy as np

    original_run = enhancer.runtime.run
    calls, buffers = [], []

    def run(magnitude, phase):
        outputs, elapsed = timed(lambda: original_run(magnitude, phase))
        calls.append({"milliseconds": elapsed, "input_frames": len(magnitude)})
        buffers.append((outputs, magnitude, phase))
        return outputs

    enhancer.runtime.run = run
    try:
        result = action(calls)
    finally:
        enhancer.runtime.run = original_run
    arrays = [value for outputs, _, _ in buffers for value in outputs]
    valid = all(len(outputs) == 3 and all(
        isinstance(value, np.ndarray) and value.shape == (1, 321)
        and value.dtype == np.float32 and value.flags.owndata and np.isfinite(value).all()
        and not np.shares_memory(value, magnitude) and not np.shares_memory(value, phase)
        for value in outputs) for outputs, magnitude, phase in buffers)
    distinct = valid and len({value.ctypes.data for value in arrays}) == len(arrays)
    frames = [row["input_frames"] for row in calls]
    priming = frames == ([enhancer.runtime.lookahead + 1] + [1] * (len(frames) - 1) if frames else [])
    times = [row["milliseconds"] for row in calls]
    return result, {"calls": len(calls), "input_frame_pattern": frames,
                    "run": summary(times) if times else None,
                    "total_ms": sum(times), "call_budget_ms": HOP / RATE * 1000,
                    "calls_over_budget": sum(value > HOP / RATE * 1000 for value in times),
                    "outputs_owned_finite_float32_3x1x321": bool(valid),
                    "outputs_have_distinct_storage": bool(distinct),
                    "priming_then_single_frame": priming,
                    "passed": bool(valid and distinct and priming)}


def stream(enhancer, audio, *, paced, pattern):
    def replay(calls):
        chunks, feed_ms, lateness_ms, native_counts = [], [], [], []
        input_sizes, output_sizes = [], []
        captured, processing_ms, first = 0, 0.0, None
        started = time.perf_counter_ns()

        def remember_first(output, stage):
            nonlocal first
            if first is None and len(output):
                first = {"stage": stage, "captured_samples": captured,
                         "capture_ms": captured / RATE * 1000,
                         "processing_accumulated_ms": processing_ms,
                         "runtime_run_accumulated_ms": sum(row["milliseconds"] for row in calls),
                         "runtime_calls": len(calls),
                         "observed_wall_ms": (time.perf_counter_ns() - started) / 1_000_000}

        index = 0
        while captured < len(audio):
            chunk = audio[captured:captured + pattern[index % len(pattern)]]
            captured += len(chunk)
            deadline = started + round(captured / RATE * 1_000_000_000)
            if paced:
                # Absolute capture deadlines preserve processing backlog.
                while (remaining := deadline - time.perf_counter_ns()) > 0:
                    time.sleep(remaining / 1_000_000_000)
            feed_started = time.perf_counter_ns()
            before = len(calls)
            output, elapsed = timed(lambda: enhancer.feed(chunk))
            processing_ms += elapsed
            remember_first(output, "feed")
            chunks.append(output)
            feed_ms.append(elapsed)
            if paced:
                lateness_ms.append(max(0, feed_started - deadline) / 1_000_000)
            input_sizes.append(len(chunk))
            output_sizes.append(len(output))
            native_counts.append(len(calls) - before)
            index += 1
        before = len(calls)
        output, flush_ms = timed(enhancer.flush)
        processing_ms += flush_ms
        remember_first(output, "flush")
        chunks.append(output)
        return chunks, {"paced": paced, "input_chunk_samples": input_sizes,
                        "feed_output_samples": output_sizes, "feed_runtime_calls": native_counts,
                        "feed_processing": summary(feed_ms),
                        "capture_deadline_lateness": summary(lateness_ms) if paced else None,
                        "flush_ms": flush_ms, "flush_samples": len(output),
                        "flush_runtime_calls": len(calls) - before,
                        "first_nonempty_output": first, "processing_ms": processing_ms,
                        "processing_real_time_factor": processing_ms / (len(audio) / RATE * 1000),
                        "wall_ms": (time.perf_counter_ns() - started) / 1_000_000}

    input_sha256 = pcm_sha256(audio)
    (chunks, result), native = observed(enhancer, replay)
    output, checks = output_checks(chunks, audio, input_sha256)
    result.update(native=native, output=checks)
    result["passed"] = (checks["passed"] and native["passed"] and native["calls"] > 0
                        and result["first_nonempty_output"] is not None)
    return output, result


def measure(audio, enhancer_class, options, iterations, first_instance, resource_probe, cudart=None,
            *, capture_hop=HOP):
    duration = len(audio) / RATE
    result = {"samples": len(audio), "duration_seconds": duration,
              "input_pcm_sha256": pcm_sha256(audio), "resource_snapshots": []}
    cuda, sampler_directories = None, ExitStack()
    cuda_error = ("CPU backend; CUDA metrics unavailable" if options["backend"] == "cpu" else
                  "CUDA query deferred until after model constructor to preserve first-load timing")

    def point(phase, iteration=None):
        row = {"phase": phase, "process": resource_probe.process_memory(),
               "cuda_global": resource_probe.cuda_memory(cuda, cuda_error)}
        if iteration is not None:
            row["steady_iteration"] = iteration
        result["resource_snapshots"].append(row)

    point("before_constructor")
    enhancer, load_ms = timed(lambda: enhancer_class(**options))
    result["load"] = {"milliseconds": load_ms, "first_instance_in_process": first_instance,
                      "os_file_cache_cold": "not controlled", "backend": enhancer.runtime.backend,
                      "exit_layers": enhancer.runtime.exit_layers, "lookahead": enhancer.runtime.lookahead}
    try:
        if options["backend"] == "cuda":
            cuda_error = "CUDA metrics not requested; supply --cudart with an explicit runtime library"
            if cudart is not None:
                try:
                    if os.name == "nt":
                        sampler_directories.enter_context(os.add_dll_directory(str(cudart.resolve().parent)))
                    cuda = (ct.WinDLL if os.name == "nt" else ct.CDLL)(str(cudart.resolve()))
                    cuda.cudaDeviceSynchronize.restype, cuda.cudaDeviceSynchronize.argtypes = ct.c_int, []
                    cuda.cudaMemGetInfo.restype = ct.c_int
                    cuda.cudaMemGetInfo.argtypes = [ct.POINTER(ct.c_size_t), ct.POINTER(ct.c_size_t)]
                    cuda_error = None
                except Exception as error:
                    cuda, cuda_error = None, f"{type(error).__name__}: {error}"
        point("after_load")
        first_output, result["first_stream"] = stream(enhancer, audio, paced=True, pattern=(capture_hop,))
        point("after_first_stream")
        steady = []
        for iteration in range(iterations):
            output, row = stream(enhancer, audio, paced=True, pattern=(capture_hop,))
            row["equal_to_first_stream"] = compare(output, first_output)
            row["passed"] = row["passed"] and row["equal_to_first_stream"]["equal"]
            steady.append(row)
            point("after_steady_stream", iteration)
        result["steady_streams"] = steady
        result["steady_processing"] = summary([row["processing_ms"] for row in steady], duration)
        result["steady_flush"] = summary([row["flush_ms"] for row in steady])
        first_walls = [row["first_nonempty_output"]["observed_wall_ms"] for row in steady
                       if row["first_nonempty_output"] is not None]
        result["steady_first_output_wall"] = summary(first_walls) if first_walls else None

        # This guard runs after paced measurements, preserving the first call.
        (full, full_ms), full_native = observed(enhancer, lambda _: timed(lambda: enhancer.enhance(audio)))
        _, full_checks = output_checks([full], audio, result["input_pcm_sha256"])
        segmented, segmented_row = stream(enhancer, audio, paced=False, pattern=GUARD_CHUNKS)
        prefix = audio[:min(len(audio), max(FFT, (options["lookahead"] + 2) * HOP))]
        _, dirty_native = observed(enhancer, lambda _: enhancer.feed(prefix))
        _, reset_ms = timed(enhancer.reset)
        repeated, repeated_row = stream(enhancer, audio, paced=False, pattern=GUARD_CHUNKS)
        comparisons = {"enhance_vs_segmented": compare(full, segmented),
                       "segmented_vs_reset_repeat": compare(segmented, repeated),
                       "enhance_vs_paced_first": compare(full, first_output)}
        guard = {"full_enhance_ms": full_ms, "full_enhance_output": full_checks,
                 "full_enhance_native": full_native, "segmented": segmented_row,
                 "dirty_prefix_samples": len(prefix), "dirty_prefix_native": dirty_native,
                 "reset_ms": reset_ms, "reset_with_active_native_state": dirty_native["calls"] > 0,
                 "reset_repeat": repeated_row, "comparisons": comparisons}
        guard["passed"] = (full_checks["passed"] and full_native["passed"] and full_native["calls"] > 0
                           and segmented_row["passed"] and repeated_row["passed"] and dirty_native["passed"]
                           and all(row["equal"] for row in comparisons.values()))
        result["partition_and_reset_guard"] = guard
        result["passed"] = (enhancer.runtime.backend == options["backend"]
                            and enhancer.runtime.exit_layers == options["exit_layers"]
                            and enhancer.runtime.lookahead == options["lookahead"] and guard["passed"]
                            and result["first_stream"]["passed"] and all(row["passed"] for row in steady))
        point("after_partition_guard")
    finally:
        try:
            _, result["close_ms"] = timed(enhancer.close)
        finally:
            enhancer = None
            result["host_gc_collected_objects"] = gc.collect()
            try:
                point("after_close_gc")
            finally:
                # Keep the sampler loaded through the post-close CUDA query.
                cuda = None
                sampler_directories.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--resources", type=Path, default=ROOT / "src-tauri/resources",
                        help="Directory containing the assembled reuse_enhancement.py and server_common.py")
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--library", type=Path, required=True)
    parser.add_argument("--backend", choices=("cpu", "cuda"), required=True)
    parser.add_argument("--exit-layers", type=int, choices=range(3, 13), default=8)
    parser.add_argument("--lookahead", type=int, choices=range(3), default=1)
    parser.add_argument("--capture-ms", type=int, choices=(20, 40), default=20,
                        help="Paced capture chunk duration; native/STFT hop and budget remain 20 ms")
    parser.add_argument("--threads", type=int, default=THREADS,
                        help="Positive native and process-local BLAS/OMP thread count (default: 2)")
    parser.add_argument("--iterations", type=int, default=5, help="Steady streams after one first stream; at least 1")
    parser.add_argument("--audio", type=Path, required=True)
    parser.add_argument("--duration", type=float, nargs="+", help="Positive prefix seconds; default: complete real clip")
    parser.add_argument("--native-source", type=Path, help="Optional exact C++ source used to build --library, for hashing")
    parser.add_argument("--cudart", type=Path,
                        help="Optional explicit CUDA runtime library; device-wide memory queries start after model load")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error("--iterations must be at least 1")
    if args.threads < 1:
        parser.error("--threads must be at least 1")
    if args.duration and any(not math.isfinite(value) or value <= 0 for value in args.duration):
        parser.error("--duration must contain finite, positive seconds")
    paths = [("--model", args.model), ("--library", args.library), ("--audio", args.audio)]
    if args.native_source:
        paths.append(("--native-source", args.native_source))
    if args.cudart:
        paths.append(("--cudart", args.cudart))
    for name, path in paths:
        if not path.is_file():
            parser.error(f"{name} must name an existing file: {path}")
    resources = args.resources.resolve()
    for name in ("reuse_enhancement.py", "server_common.py"):
        if not (resources / name).is_file():
            parser.error(f"INTERFACE_PENDING: --resources lacks {name}: {resources}")
    incoming_env = {key: os.environ.get(key) for key in ENV_KEYS}
    # Bound process-local CPU concurrency before NumPy/native libraries load.
    for name in ("OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "MKL_NUM_THREADS"):
        os.environ[name] = str(args.threads)
    sys.path.insert(0, str(resources))
    import numpy as np
    import benchmark_asr_resources as resource_probe

    enhancement = importlib.import_module("reuse_enhancement")
    server_common = importlib.import_module("server_common")
    enhancer_class = getattr(enhancement, "RealtimeReuseEnhancer", None)
    if enhancer_class is None:
        parser.error("INTERFACE_PENDING: --resources has no RealtimeReuseEnhancer")
    audio, source_rate = server_common.BaseASRServer._read_mono_audio(args.audio.resolve())
    audio = np.ascontiguousarray(server_common.BaseASRServer._resample(audio, source_rate), dtype=np.float32)
    if not len(audio) or audio.ndim != 1 or not np.isfinite(audio).all():
        parser.error("--audio must contain finite, nonempty mono PCM after resampling")
    counts = [len(audio)] if args.duration is None else [round(value * RATE) for value in args.duration]
    if any(count < 1 or count > len(audio) for count in counts):
        parser.error("Every --duration must select a nonempty prefix within the real input clip")
    sources = {"benchmark": file_record(__file__),
               "benchmark_helpers": file_record(sys.modules["benchmark_reuse"].__file__),
               "resource_helpers": file_record(resource_probe.__file__),
               "resource_fixture_helpers": file_record(sys.modules["test_r2t2_native_cache"].__file__),
               "reuse_enhancement": file_record(enhancement.__file__),
               "server_common": file_record(server_common.__file__)}
    if args.native_source:
        sources["native_build_source_supplied"] = file_record(args.native_source)
    capture_hop = RATE * args.capture_ms // 1000
    result = {"recorded_at_utc": datetime.now(timezone.utc).isoformat(),
              "command": sys.argv, "python": sys.version, "numpy": np.__version__, "platform": platform.platform(),
              "process": {"pid": os.getpid(), "executable": sys.executable,
                          "cpu_count": os.cpu_count(), "native_threads_requested": args.threads,
                          "incoming_environment": incoming_env,
                          "effective_environment": {key: os.environ.get(key) for key in ENV_KEYS}},
              "parameters": {"backend": args.backend, "exit_layers": args.exit_layers,
                             "lookahead": args.lookahead, "steady_iterations": args.iterations,
                             "threads": args.threads, "capture_ms": args.capture_ms,
                             "rate": RATE, "fft": FFT, "capture_hop": capture_hop, "stft_hop": HOP},
              "timing": {"clock": "perf_counter_ns (monotonic)",
                         "pacing": "absolute deadlines from cumulative captured samples; backlog retained",
                         "runtime_run_scope": "Python validation/marshalling plus synchronous native ggml compute",
                         "processing_scope": "feed plus flush, including native-call observation; output checks excluded",
                         "load_scope": "new enhancer constructor; no warmup; OS file cache not controlled",
                         "resource_scope": "resource queries, sampler loading, GC and hashes excluded from all timers",
                         "speed_target": "none; 20 ms native-call budget is descriptive"},
              "resource_sampling": {
                  "cudart": file_record(args.cudart) if args.cudart else None,
                  "cuda_scope": "device-wide current-context OS-reported total minus free; not per-model or per-process VRAM",
                  "cuda_before_constructor": "not queried; avoids warming CUDA before timed model construction",
                  "after_close_gc_scope": "host gc.collect after enhancer.close; input/output/report objects retained",
                  "sampler_scope": "sampler DLL and CUDA context may remain resident through and after model close; directory handles closed after final query",
                  "interpretation": "observed counts/bytes only; no stable-memory or leak-free assertion"},
              "model": file_record(args.model), "runtime": file_record(args.library), "sources": sources,
              "audio": {**file_record(args.audio), "source_rate": source_rate,
                        "resampled_samples": len(audio), "resampled_duration_seconds": len(audio) / RATE,
                        "resampled_pcm_sha256": pcm_sha256(audio)}, "cases": []}
    options = {"model_path": args.model.resolve(), "library_path": args.library.resolve(),
               "backend": args.backend, "threads": args.threads,
               "exit_layers": args.exit_layers, "lookahead": args.lookahead}
    for count in counts:
        case = measure(audio[:count], enhancer_class, options, args.iterations, not result["cases"],
                       resource_probe, args.cudart, capture_hop=capture_hop)
        result["cases"].append(case)
        result["passed"] = all(row["passed"] for row in result["cases"])
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2, allow_nan=False) + "\n", encoding="utf-8")
        print(f"{args.backend} {case['duration_seconds']:.3f}s: "
              f"steady processing p50={case['steady_processing']['p50_ms']:.3f}ms; "
              f"checks={'PASS' if case['passed'] else 'FAIL'}", flush=True)
    print(f"Measurements: {args.output.resolve()}")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
