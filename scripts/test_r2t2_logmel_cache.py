"""Frozen real-frontend numerical trace and actual sparse-mel-work regression.

Example: python scripts/test_r2t2_logmel_cache.py --source build/r2t2/source
    --baseline-source path/to/frozen-baseline --build-dir tmp/r2t2-logmel-cache-tests
The baseline directory contains the four pinned source/header paths below.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment



FROZEN_SHA256 = {
    "src/community_models/confucius4_r2t2/frontend_whisper.cpp": "07a0cb7424bc2bbde306439a970e279ff3db8cdd4cf2dbc0ee10537b0f0e02d4",
    "include/engine/community_models/confucius4_r2t2/frontend_whisper.h": "aa1f5bd25827aa6c8f6640618245c13bd7af5b71ab75dc2dc2e8581685758729",
    "src/framework/audio/dsp.cpp": "5ccdfbc818d707535cca4bb2330df74a93e4e63277c03199a93ef121493c64aa",
    "include/engine/framework/audio/dsp.h": "4d85ffacdba5ae30adc530e2535f1dd585758908bbc32e0a580bab89d18d2022",
}
FROZEN = tuple(FROZEN_SHA256)
SUPPORT = (
    "src/framework/audio/conversion.cpp",
    "src/framework/audio/resampling.cpp",
    "src/framework/audio/waveform_ops.cpp",
    "src/framework/audio/wav_reader.cpp",
    "src/framework/debug/trace.cpp",
)
FFT = "src/framework/audio/fft.cpp"
TEST = ROOT / "scripts/native_tests/r2t2_logmel_cache_test.cpp"
EMPTY = ROOT / "scripts/native_tests/r2t2_empty_assets.cpp"
TARGET = "MelFilterbank().compute_custom_sparse_from_magnitude("
DECLARATION = """
namespace engine::audio {
AudioTensor r2t2_test_counted_sparse(
    const std::vector<float> &, int64_t, int64_t, int64_t, int64_t,
    const SparseMelFilterbank &);
}
"""


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def inputs(source, baseline, frozen):
    selected = {name: (baseline if frozen and name in FROZEN else source) / name
                for name in (*FROZEN, *SUPPORT, FFT)}

    for path in (source / "include").rglob("*.h"):
        name = path.relative_to(source).as_posix()
        selected[name] = baseline / name if frozen and name in FROZEN else path
    fingerprint = {name: sha(path) for name, path in sorted(selected.items())}
    for path in (TEST, EMPTY, Path(__file__), ROOT / "scripts/build_r2t2_runtime.py"):
        fingerprint[str(path.relative_to(ROOT))] = sha(path)
    return selected, fingerprint


def build(source, baseline, build_root, label, env, compiler, linker, no_build):
    frozen = label == "baseline"
    selected, fingerprint = inputs(source, baseline, frozen)
    directory = build_root / label
    directory.mkdir(parents=True, exist_ok=True)
    executable = directory / "logmel_cache_test.exe"
    manifest_path = directory / "compiled-source.json"
    if no_build:
        previous = json.loads(manifest_path.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executable_sha256"] != sha(executable):
            raise RuntimeError(f"{label}: compiled inputs changed; rerun without --no-build")
        for name, digest in previous["generated"].items():
            if sha(directory / name) != digest:
                raise RuntimeError(f"{label}: generated overlay changed: {name}")
        return executable, fingerprint

    overlay = directory / "overlay"
    if frozen:
        for name in FROZEN:
            destination = overlay / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(baseline / name, destination)
    dsp_text = selected["src/framework/audio/dsp.cpp"].read_text(encoding="utf-8")
    if dsp_text.count(TARGET) != 1:
        raise RuntimeError(f"{label}: expected exactly one real sparse invocation, got {dsp_text.count(TARGET)}")
    include = '#include "engine/framework/audio/dsp.h"'
    if dsp_text.count(include) != 1:
        raise RuntimeError(f"{label}: unexpected DSP include")
    counted = dsp_text.replace(TARGET, "r2t2_test_counted_sparse(", 1).replace(include, include + "\n" + DECLARATION, 1)

    restored = counted.replace(include + "\n" + DECLARATION, include, 1)
    restored = restored.replace("r2t2_test_counted_sparse(", TARGET, 1)
    if restored != dsp_text:
        raise RuntimeError(f"{label}: overlay changed more than declaration and sole call expression")
    generated_dsp = overlay / "counted_dsp.cpp"
    generated_dsp.parent.mkdir(parents=True, exist_ok=True)
    generated_dsp.write_text(counted, encoding="utf-8")
    generated = {path.relative_to(directory).as_posix(): sha(path) for path in overlay.rglob("*") if path.is_file()}
    commands = []
    log_path = directory / "build.log"
    log_path.write_text("", encoding="utf-8")

    def run(command):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=directory, env=env, capture_output=True, text=True, errors="replace")
        with log_path.open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        if result.returncode:
            raise RuntimeError(f"{label}: compile/link exit {result.returncode}; see {log_path}\n" + result.stdout + result.stderr)

    common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
              "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS", f"/I{overlay / 'include'}",
              f"/I{source / 'include'}", f"/I{source / 'external/ggml/include'}", f"/Fo{directory}/"]
    frontend = overlay / FROZEN[0] if frozen else source / FROZEN[0]
    actual_sources = [frontend, generated_dsp, *[source / name for name in SUPPORT], TEST, EMPTY]
    run([*common, *actual_sources])
    run([*common, "/Dreal_fft_forward=r2t2_test_real_fft_forward", source / FFT])
    objects = [directory / (path.stem + ".obj") for path in (*actual_sources, source / FFT)]
    run([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", *objects])
    _, after = inputs(source, baseline, frozen)
    if after != fingerprint:
        raise RuntimeError(f"{label}: inputs changed during compilation")
    save(manifest_path, {"inputs": fingerprint, "generated": generated,
                         "executable_sha256": sha(executable), "commands": commands,
                         "sparse_call_replacements": 1})
    return executable, fingerprint


def execute(executable, mode, label, env):
    trace = executable.parent / f"{label}.trace.bin"
    result = subprocess.run([str(executable), mode, str(trace)], cwd=executable.parent,
                            env=env, capture_output=True, text=True, errors="replace")
    output = result.stdout + result.stderr
    (executable.parent / f"{label}.log").write_text(output, encoding="utf-8")
    evidence = {"command": [str(executable), mode, str(trace)], "returncode": result.returncode,
                "trace_sha256": sha(trace) if trace.exists() else None,
                "trace_bytes": trace.stat().st_size if trace.exists() else None,
                "summary": [line for line in output.splitlines() if line.startswith(("NUMERICAL_PASS", "PASS ", "WORK_FAIL", "FAIL "))],
                "work_red": [line for line in output.splitlines() if line.startswith("WORK_RED ")]}
    save(executable.parent / f"{label}.json", evidence)
    print(f"{label}: exit={result.returncode}; " + "; ".join(evidence["summary"]))
    if evidence["work_red"]:
        print(evidence["work_red"][0])
    return evidence, trace


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--baseline-source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/r2t2-logmel-cache-tests")
    parser.add_argument("--baseline-only", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source = args.source.resolve()
    baseline_source = args.baseline_source.resolve()
    build_root = args.build_dir.resolve()
    external_manifest = baseline_source / "manifest.json"
    if external_manifest.exists():
        claims = json.loads(external_manifest.read_text(encoding="utf-8"))
        if set(claims) != set(FROZEN):
            raise RuntimeError("external frozen manifest must contain exactly the four baseline paths")
        for name, digest in claims.items():
            if sha(baseline_source / name) != digest:
                raise RuntimeError(f"external frozen manifest raw fingerprint mismatch: {name}")
    for name, digest in FROZEN_SHA256.items():
        canonical = (baseline_source / name).read_bytes().replace(b"\r\n", b"\n")
        if hashlib.sha256(canonical).hexdigest() != digest:
            raise RuntimeError(f"frozen baseline fingerprint mismatch: {name}")
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        raise RuntimeError("MSVC compiler/linker unavailable after vcvars")
    baseline, baseline_inputs = build(source, baseline_source, build_root, "baseline", env, compiler, linker, args.no_build)
    parity, baseline_trace = execute(baseline, "parity", "baseline-parity", env)
    red, red_trace = execute(baseline, "all", "baseline-negative-control", env)
    if parity["returncode"] or red["returncode"] != 2 or not red["work_red"]:
        raise RuntimeError("baseline must pass numerics and semantically fail actual-work requirements")
    if baseline_trace.read_bytes() != red_trace.read_bytes():
        raise RuntimeError("baseline negative control changed full numerical trace")
    evidence = {"frozen_manifest": FROZEN_SHA256, "baseline_inputs": baseline_inputs,
                "baseline_parity": parity, "baseline_negative_control": red}
    if not args.baseline_only:
        candidate, candidate_inputs = build(source, baseline_source, build_root, "candidate", env, compiler, linker, args.no_build)
        actual, candidate_trace = execute(candidate, "all", "candidate", env)
        evidence.update(candidate_inputs=candidate_inputs, candidate=actual,
                        exact_full_trace_equal=baseline_trace.read_bytes() == candidate_trace.read_bytes())
        save(build_root / "results.json", evidence)
        if actual["returncode"] or not evidence["exact_full_trace_equal"]:
            raise RuntimeError("candidate failed numerical/work requirements or entire frozen trace differs; see results.json")
        print(f"PASS exact entire frozen trace ({actual['trace_bytes']} bytes); actual sparse mel cells and FFT frames")
    else:
        save(build_root / "baseline-red.json", evidence)
        print("RED established: baseline numerical control passed; actual sparse-work contract rejected baseline")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
