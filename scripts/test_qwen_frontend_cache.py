"""Qwen exact-feature and actual FFT/sparse-mel cache-work regression.

Reuses the R2T2 oracle and mechanical interposition, with an explicit Qwen
compile definition. The pinned baseline is the current uncached Qwen owner and
current DSP, independent of the older R2T2/asr9 baseline.

Example: python scripts/test_qwen_frontend_cache.py --source build/r2t2/source
    --baseline-source build/r2t2/shipping-source --build-dir tmp/qwen-frontend
"""
import argparse
import hashlib
from pathlib import Path
import shutil

import test_r2t2_logmel_cache as harness
from build_r2t2_runtime import ROOT, build_environment


# Canonical LF hashes of the shipping-source owners before Qwen cache reuse.
FROZEN_SHA256 = {
    "src/models/qwen3_asr/frontend_whisper.cpp": "11096fde6f6134916e6afa575c30c35b395bc9d88de9c0311ad8d9449a94995e",
    "include/engine/models/qwen3_asr/frontend_whisper.h": "daabb532f3cb6911909a1189dc27ada2681693c48834de2accff1f7451d9f86e",
    "src/framework/audio/dsp.cpp": "43cdd799d9a240c302c98f75e534e274fff0393a7e2c4e9a2de301a66153be5e",
    "include/engine/framework/audio/dsp.h": "ff131b0a806ef90b46c49cb81be29d538bd7e7623083710ea755b17e60baa374",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--baseline-source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/qwen-frontend")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source, baseline_source, build_root = (
        path.resolve() for path in (args.source, args.baseline_source, args.build_dir))
    for name, digest in FROZEN_SHA256.items():
        canonical = (baseline_source / name).read_bytes().replace(b"\r\n", b"\n")
        if hashlib.sha256(canonical).hexdigest() != digest:
            raise RuntimeError(f"frozen Qwen baseline fingerprint mismatch: {name}")

    harness.FROZEN_SHA256 = FROZEN_SHA256
    harness.FROZEN = tuple(FROZEN_SHA256)
    original_inputs = harness.inputs

    def inputs(*values):
        selected, fingerprint = original_inputs(*values)
        fingerprint[str(Path(__file__).resolve().relative_to(ROOT))] = harness.sha(Path(__file__))
        return selected, fingerprint

    harness.inputs = inputs
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    # CL is MSVC's documented per-process command prefix; no global setting.
    env["CL"] = env.get("CL", "") + " /DLIGHT_WHISPER_QWEN_FRONTEND_TEST"
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        raise RuntimeError("MSVC compiler/linker unavailable after vcvars")
    baseline, baseline_inputs = harness.build(
        source, baseline_source, build_root, "baseline", env, compiler, linker, args.no_build)
    parity, baseline_trace = harness.execute(baseline, "parity", "baseline-parity", env)
    red, red_trace = harness.execute(baseline, "all", "baseline-negative-control", env)
    if parity["returncode"] or red["returncode"] != 2 or not red["work_red"]:
        raise RuntimeError("Qwen baseline must pass numerics and fail actual-work requirements")
    if baseline_trace.read_bytes() != red_trace.read_bytes():
        raise RuntimeError("Qwen baseline negative control changed numerical trace")
    evidence = {"frozen_manifest": FROZEN_SHA256, "baseline_inputs": baseline_inputs,
                "compile_definition": "LIGHT_WHISPER_QWEN_FRONTEND_TEST",
                "baseline_parity": parity, "baseline_negative_control": red}
    if args.baseline_only:
        harness.save(build_root / "baseline-red.json", evidence)
        print("RED established: Qwen numerical control passed; actual FFT/sparse-work contract rejected baseline")
        return 0
    candidate, candidate_inputs = harness.build(
        source, baseline_source, build_root, "candidate", env, compiler, linker, args.no_build)
    actual, candidate_trace = harness.execute(candidate, "all", "candidate", env)
    evidence.update(candidate_inputs=candidate_inputs, candidate=actual,
                    exact_full_trace_equal=baseline_trace.read_bytes() == candidate_trace.read_bytes())
    harness.save(build_root / "results.json", evidence)
    if actual["returncode"] or not evidence["exact_full_trace_equal"]:
        raise RuntimeError("Qwen candidate failed numerical/work requirements or entire frozen trace differs; see results.json")
    print(f"PASS exact entire frozen Qwen trace ({actual['trace_bytes']} bytes); actual sparse mel cells and FFT frames")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
