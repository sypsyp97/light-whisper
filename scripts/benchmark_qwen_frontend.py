"""Paired actual Qwen CPU frontend measurements, without models or CUDA.

Build both immutable source snapshots with --build-only, then use --no-build.
Even --pairs alternate AB/BA, giving ABBA process order; every fresh process
excludes two warmup replays. Each replay measures public PCM resident reuse,
growing previews, 12-second sliding previews, and final/rewrite invalidation.
Long PCM is explicitly a synthetic 36-second concatenation of the public WAV.
Every output has a SHA256 of complete feature bytes, metadata and mask, checked
against the uncached baseline. No latency threshold is a correctness condition.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import shutil
import statistics
import subprocess

from build_r2t2_runtime import ROOT, build_environment
import test_r2t2_logmel_cache as harness
from test_qwen_frontend_cache import FROZEN_SHA256


BENCH = ROOT / "scripts/native_tests/qwen_frontend_bench.cpp"
SCENARIOS = {"identical_resident": 20, "growing_preview": 60,
             "sliding_preview_12s": 120, "final_and_rewrite": 2}
WARMUPS = 2


def signature(row):
    return [(value["scenario"], value["label"], value["captured_samples"], value["input_samples"],
             value["frames"], value["encoder_tokens"], value["pcm_sha256"], value["feature_sha256"])
            for value in row["extracts"]]


def scenario_totals(row):
    totals = {name: {"ns": 0, "fft_frames": 0, "mel_frames": 0, "mel_cells": 0} for name in SCENARIOS}
    for value in row["extracts"]:
        for key in totals[value["scenario"]]:
            totals[value["scenario"]][key] += value[key]
    return totals


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-source", required=True, type=Path)
    parser.add_argument("--candidate-source", required=True, type=Path)
    parser.add_argument("--wav", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/qwen-frontend-bench")
    parser.add_argument("--pairs", type=int, default=4)
    parser.add_argument("--iterations", type=int, default=1, help="measured replays per process, after two warmups")
    parser.add_argument("--output", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--build-only", action="store_true")
    mode.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if args.pairs < 2 or args.pairs % 2 or args.iterations < 1:
        parser.error("--pairs must be positive/even and at least 2; --iterations must be positive")
    baseline_source, candidate_source, wav, build = (
        path.resolve() for path in (args.baseline_source, args.candidate_source, args.wav, args.build_dir))
    if not wav.is_file():
        parser.error(f"public WAV missing: {wav}")
    for name, expected in FROZEN_SHA256.items():
        canonical = (baseline_source / name).read_bytes().replace(b"\r\n", b"\n")
        if hashlib.sha256(canonical).hexdigest() != expected:
            raise RuntimeError(f"uncached Qwen baseline changed: {name}")

    # Reuse the accepted mechanical overlays, compiler setup, input fingerprints,
    # real FFT renaming, sparse-call interposition and no-build integrity checks.
    harness.TEST = BENCH
    harness.FROZEN_SHA256 = FROZEN_SHA256
    harness.FROZEN = tuple(FROZEN_SHA256)
    original_inputs = harness.inputs

    def inputs(*values):
        selected, fingerprint = original_inputs(*values)
        for path in (Path(__file__).resolve(), ROOT / "scripts/test_qwen_frontend_cache.py"):
            fingerprint[str(path.relative_to(ROOT))] = harness.sha(path)
        return selected, fingerprint

    harness.inputs = inputs
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    env.update(OMP_NUM_THREADS="1", MKL_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1")
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        raise RuntimeError("MSVC compiler/linker unavailable after vcvars")
    executables, manifests = {}, {}
    for label, source in (("baseline", baseline_source), ("candidate", candidate_source)):
        executable, _ = harness.build(source, baseline_source, build, label, env, compiler, linker, args.no_build)
        executables[label] = executable
        manifests[label] = json.loads((executable.parent / "compiled-source.json").read_text(encoding="utf-8"))
    if args.build_only:
        print(f"BUILD READY baseline={executables['baseline']} candidate={executables['candidate']}")
        return 0

    output = args.output.resolve() if args.output else build / "benchmark.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    evidence = {
        "schema": "qwen-frontend-paired-benchmark-v1", "builds": manifests,
        "sources": {"baseline": str(baseline_source), "candidate": str(candidate_source)},
        "fixture": {"path": str(wav), "sha256": harness.sha(wav),
                    "derivation": "public PCM repeated/truncated to synthetic 36 seconds; rewrite rotates by 200 ms"},
        "config": {"sample_rate": 16000, "n_fft": 400, "hop_length": 160, "mel_bins": 128, "fft_threads": 1},
        "schedule": {"startup_samples": 1920, "preview_step_samples": 3200,
                     "window_samples": 192000, "native_min_padded_samples": 8000,
                     "extracts_per_replay": SCENARIOS, "resident_prime_excluded": True,
                     "sliding_and_final_prime_excluded": True},
        "measurement": "const native frontend.extract wall time, including real work counters; hashes/PCM construction/IO excluded",
        "warmups_per_process": WARMUPS, "measured_iterations_per_process": args.iterations,
        "pairs": args.pairs, "runs": [], "all_feature_hashes_match_uncached_baseline": True,
    }
    reference = None
    samples = defaultdict(list)
    paired = defaultdict(list)
    for pair in range(args.pairs):
        order = ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline")
        pair_values = {}
        for label in order:
            executable = executables[label]
            command = [str(executable), str(wav), str(WARMUPS), str(args.iterations)]
            result = subprocess.run(command, cwd=executable.parent, env=env, capture_output=True,
                                    text=True, errors="replace")
            log = build / f"pair-{pair}-{label}.log"
            log.write_text(result.stdout + result.stderr, encoding="utf-8")
            if result.returncode:
                raise RuntimeError(f"{label} benchmark exit {result.returncode}; see {log}")
            rows = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
            measured = [row for row in rows if not row["warmup"]]
            if len(rows) != WARMUPS + args.iterations or len(measured) != args.iterations:
                raise RuntimeError(f"{label}: warmup/measurement accounting changed")
            for row in rows:
                if dict(Counter(value["scenario"] for value in row["extracts"])) != SCENARIOS:
                    raise RuntimeError(f"{label}: replay schedule changed")
                actual = signature(row)
                if reference is None:
                    reference = actual
                if actual != reference:
                    evidence["all_feature_hashes_match_uncached_baseline"] = False
                    evidence["mismatch"] = {"pair": pair, "label": label, "iteration": row["iteration"]}
                    harness.save(output, evidence)
                    raise RuntimeError(f"{label}: per-output SHA256 or input signature differs from uncached baseline; see {output}")
            evidence["runs"].append({"pair": pair, "label": label, "command": command,
                                     "returncode": result.returncode, "rows": rows})
            values = [scenario_totals(row) for row in measured]
            pair_values[label] = values
            for scenario in SCENARIOS:
                samples[label, scenario].extend(value[scenario] for value in values)
            harness.save(output, evidence)
            print(f"PAIR {pair + 1}/{args.pairs} {label} exact_features=PASS", flush=True)
        for scenario in SCENARIOS:
            before = statistics.median(value[scenario]["ns"] for value in pair_values["baseline"])
            after = statistics.median(value[scenario]["ns"] for value in pair_values["candidate"])
            if before <= 0 or after <= 0:
                raise RuntimeError("nonpositive native timing observation")
            paired[scenario].append({"pair": pair, "baseline_ns": before, "candidate_ns": after,
                                     "speedup": before / after, "delta_percent": (after - before) / before * 100})
    evidence["summary"] = {}
    for scenario in SCENARIOS:
        summary = {"paired": paired[scenario],
                   "median_paired_speedup": statistics.median(value["speedup"] for value in paired[scenario]),
                   "median_paired_delta_percent": statistics.median(value["delta_percent"] for value in paired[scenario])}
        for label in ("baseline", "candidate"):
            summary[label] = {key: statistics.median(value[key] for value in samples[label, scenario])
                              for key in ("ns", "fft_frames", "mel_frames", "mel_cells")}
        evidence["summary"][scenario] = summary
        print(f"{scenario}: median paired speedup={summary['median_paired_speedup']:.3f}x "
              f"delta={summary['median_paired_delta_percent']:+.2f}%")
    harness.save(output, evidence)
    print(f"BENCH exact per-output SHA256 PASS; paired measurements saved to {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
