"""Measure the assembled RE-USE runtime on real, public sample audio.

The first invocation is separate from --iterations steady invocations. Without
--duration the entire recording is enhanced, including its overlap tail. Native
core timings exclude STFT/ISTFT and use the same one-second/overlap segmentation.
Optional --streaming replays 40 ms capture chunks with real pacing; its latency
fields distinguish native compute, complete processing, and observed wall time.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
RATE, FFT, STFT_HOP, THREADS = 16_000, 640, 80, 2


def sha256(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def timed(function):
    started = time.perf_counter_ns()
    result = function()
    return result, (time.perf_counter_ns() - started) / 1_000_000


def summary(values, duration_seconds=None):
    import numpy as np

    result = {"n": len(values), "milliseconds": values,
              "p50_ms": float(np.percentile(values, 50)), "p95_ms": float(np.percentile(values, 95))}
    if duration_seconds is not None:
        result["p50_real_time_factor"] = result["p50_ms"] / (duration_seconds * 1000)
    return result


def spectra_for_clip(audio, enhancer_class):
    import numpy as np

    window = np.hanning(FFT + 1)[:-1].astype(np.float32)
    spectra, start = [], 0
    while start < len(audio):
        segment = audio[start:start + enhancer_class.WINDOW]
        padded_count = max(FFT, ((len(segment) + STFT_HOP - 1) // STFT_HOP) * STFT_HOP)
        padded = np.pad(segment, (0, padded_count - len(segment)), constant_values=1e-8)
        padded = np.pad(padded, (FFT // 2, FFT // 2), mode="reflect")
        frames = np.lib.stride_tricks.sliding_window_view(padded, FFT)[::STFT_HOP]
        spectrum = np.fft.rfft(frames * window).astype(np.complex64)
        spectra.append((np.log1p(np.abs(spectrum)).astype(np.float32), np.angle(spectrum).astype(np.float32)))
        if len(segment) < enhancer_class.WINDOW:
            break
        start += enhancer_class.HOP
    return spectra


def repeated(function, iterations, duration_seconds):
    output, first_ms = timed(function)
    times = []
    for _ in range(iterations):
        output, elapsed = timed(function)
        times.append(elapsed)
    return output, {"first_call_ms": first_ms, "steady": summary(times, duration_seconds)}


def streaming_replay(enhancer, audio):
    import numpy as np

    compute_ms, processing_ms, first = 0.0, 0.0, None
    outputs, calls = [], []
    original_run = enhancer.runtime.run

    def observed_run(magnitude, phase):
        nonlocal compute_ms
        output, elapsed = timed(lambda: original_run(magnitude, phase))
        compute_ms += elapsed
        calls.append(elapsed)
        return output

    enhancer.runtime.run = observed_run
    started = time.perf_counter_ns()
    captured = 0

    def remember_first(output, stage):
        nonlocal first
        if first is None and len(output):
            capture_ms = captured / RATE * 1000
            first = {"stage": stage, "captured_samples": captured,
                     "capture_ms": capture_ms, "accumulated_model_compute_ms": compute_ms,
                     "capture_plus_model_ms": capture_ms + compute_ms,
                     "capture_plus_processing_ms": capture_ms + processing_ms,
                     "observed_wall_ms": (time.perf_counter_ns() - started) / 1_000_000}

    try:
        for start in range(0, len(audio), 640):
            chunk = audio[start:start + 640]
            time.sleep(len(chunk) / RATE)
            captured += len(chunk)
            output, elapsed = timed(lambda: enhancer.feed(chunk))
            processing_ms += elapsed
            outputs.append(output)
            remember_first(output, "feed")
        output, flush_ms = timed(enhancer.flush)
        processing_ms += flush_ms
        outputs.append(output)
        remember_first(output, "flush")
    finally:
        enhancer.runtime.run = original_run
    output = np.concatenate(outputs)
    return {"first_nonempty_output": first, "tail_flush_ms": flush_ms,
            "model_compute_ms": compute_ms, "processing_ms": processing_ms,
            "native_call_ms": calls, "native_calls": len(calls),
            "wall_ms": (time.perf_counter_ns() - started) / 1_000_000,
            "output_samples": len(output), "output_finite": bool(np.isfinite(output).all())}


def measure(audio, options, iterations, with_streaming):
    import numpy as np
    from reuse_enhancement import NativeReuseRuntime, ReuseEnhancer

    duration = len(audio) / RATE
    result = {"samples": len(audio), "duration_seconds": duration,
              "input_pcm_sha256": hashlib.sha256(audio.tobytes()).hexdigest()}
    spectra = spectra_for_clip(audio, ReuseEnhancer)
    runtime, load_ms = timed(lambda: NativeReuseRuntime(**options))
    try:
        _, result["native_core"] = repeated(lambda: [runtime.run(mag, phase) for mag, phase in spectra],
                                            iterations, duration)
        result["native_core"].update(load_ms=load_ms, backend=runtime.backend,
                                      calls_per_clip=len(spectra), frames_per_call=[len(mag) for mag, _ in spectra])
    finally:
        runtime.close()
    enhancer, load_ms = timed(lambda: ReuseEnhancer(**options))
    try:
        output, result["waveform"] = repeated(lambda: enhancer.enhance(audio), iterations, duration)
        result["waveform"].update(load_ms=load_ms, backend=enhancer.runtime.backend,
                                   output_samples=len(output), output_finite=bool(np.isfinite(output).all()),
                                   output_pcm_sha256=hashlib.sha256(output.tobytes()).hexdigest())
    finally:
        enhancer.close()
    if with_streaming:
        enhancer, load_ms = timed(lambda: ReuseEnhancer(**options))
        try:
            first = streaming_replay(enhancer, audio)
            steady = [streaming_replay(enhancer, audio) for _ in range(iterations)]
            result["streaming"] = {"load_ms": load_ms, "chunk_samples": 640, "pacing": "sleep each capture chunk",
                                   "first_replay": first, "steady_replays": steady,
                                   "steady_processing": summary([row["processing_ms"] for row in steady], duration),
                                   "steady_tail_flush": summary([row["tail_flush_ms"] for row in steady]),
                                   "steady_first_output_capture_plus_model": summary([
                                       row["first_nonempty_output"]["capture_plus_model_ms"] for row in steady]),
                                   "steady_first_output_capture_plus_processing": summary([
                                       row["first_nonempty_output"]["capture_plus_processing_ms"] for row in steady])}
        finally:
            enhancer.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=("cpu", "cuda"), required=True)
    parser.add_argument("--model", type=Path, default=ROOT / "tmp/reuse-investigation/model.safetensors")
    parser.add_argument("--library", type=Path)
    parser.add_argument("--audio", type=Path, default=ROOT / "tmp/reuse-investigation/mic_test2.wav")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=5)
    parser.add_argument("--duration", type=float, nargs="+", help="Seconds from the real input start; default: entire clip")
    parser.add_argument("--streaming", action="store_true", help="Also run first plus steady paced 40 ms streaming replays")
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error("--iterations must be positive")
    if args.duration and any(not math.isfinite(duration) or duration <= 0 for duration in args.duration):
        parser.error("--duration must contain finite, positive seconds")
    # Process-local controls, set before NumPy or either native backend loads.
    os.environ["OMP_NUM_THREADS"] = str(THREADS)
    if args.backend == "cuda":
        os.environ["NVIDIA_TF32_OVERRIDE"] = "0"
    sys.path.insert(0, str(ROOT / "src-tauri/resources"))
    import numpy as np
    from server_common import BaseASRServer

    library = (args.library or ROOT / f"src-tauri/resources/r2t2-native/{args.backend}/reuse.dll").resolve()
    model, source = args.model.resolve(), args.audio.resolve()
    audio, source_rate = BaseASRServer._read_mono_audio(source)
    audio = np.ascontiguousarray(BaseASRServer._resample(audio, source_rate), dtype=np.float32)
    if not len(audio) or not np.isfinite(audio).all():
        parser.error("--audio must contain finite, nonempty PCM")
    counts = [len(audio)] if args.duration is None else [round(duration * RATE) for duration in args.duration]
    if any(count < 1 or count > len(audio) for count in counts):
        parser.error("Every --duration must select a nonempty prefix within the real input clip")
    result = {"recorded_at_utc": datetime.now(timezone.utc).isoformat(),
              "command": sys.argv, "python": sys.version, "platform": platform.platform(),
              "backend_requested": args.backend, "threads": THREADS, "steady_iterations": args.iterations,
              "first_invocations": 1, "rate": RATE, "fft": FFT, "stft_hop": STFT_HOP,
              "tf32_override": os.environ.get("NVIDIA_TF32_OVERRIDE"),
              "model": {"path": str(model), "sha256": sha256(model)},
              "runtime": {"path": str(library), "sha256": sha256(library)},
              "audio": {"path": str(source), "sha256": sha256(source), "source_rate": source_rate,
                        "resampled_samples": len(audio), "resampled_duration_seconds": len(audio) / RATE},
              "cases": []}
    options = {"model_path": model, "library_path": library, "backend": args.backend, "threads": THREADS}
    for count in counts:
        result["cases"].append(measure(audio[:count], options, args.iterations, args.streaming))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        case = result["cases"][-1]
        print(f"{args.backend} {case['duration_seconds']:.3f}s: "
              f"waveform steady p50={case['waveform']['steady']['p50_ms']:.3f}ms "
              f"native p50={case['native_core']['steady']['p50_ms']:.3f}ms", flush=True)
    print(f"Measurements: {args.output.resolve()}")


if __name__ == "__main__":
    main()
