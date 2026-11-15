"""Build/run the actual CPU frontend stage without CUDA, models, or FFT mocks.

Build baseline/candidate separately with --build-only before measuring either.
Then --no-build runs one fresh process: two warmups and --iterations measured
36-second replays. Alternate preserved executables externally for paired ABBA.
"""
import argparse
import json
from pathlib import Path
import shutil
import statistics
import subprocess

from build_r2t2_runtime import ROOT, build_environment
from test_r2t2_frontend_cache import SOURCES, FFT, HEADERS, EMPTY_ASSETS, sha256


BENCH = ROOT / "scripts/native_tests/r2t2_frontend_bench.cpp"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--wav", type=Path, default=ROOT / "tmp/asr-caption-20261017/fixtures/asr_zh.wav")
    parser.add_argument("--iterations", type=int, default=6)
    parser.add_argument("--output", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--build-only", action="store_true")
    mode.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if args.iterations < 1:
        parser.error("--iterations must be positive")
    source, build = args.source.absolute(), args.build_dir.absolute()
    build.mkdir(parents=True, exist_ok=True)
    executable = build / "r2t2_frontend_bench.exe"
    manifest_path = build / "build-manifest.json"
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}

    if not args.no_build:
        compiler = shutil.which("cl.exe", path=env["PATH"])
        linker = shutil.which("link.exe", path=env["PATH"])
        if not compiler or not linker:
            parser.error("Visual Studio cl.exe/link.exe unavailable")
        inputs = [*[source / name for name in (*SOURCES, FFT, *HEADERS)], BENCH, EMPTY_ASSETS,
                  Path(__file__).absolute(), ROOT / "scripts/test_r2t2_frontend_cache.py"]
        manifest = {"source_at_build": str(source), "inputs_sha256": {str(p): sha256(p) for p in inputs}}
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                  "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS", f"/I{source / 'include'}",
                  f"/I{source / 'external/ggml/include'}", f"/Fo{build}/"]
        compile_command = [*common, *[str(source / name) for name in (*SOURCES, FFT)],
                           str(BENCH), str(EMPTY_ASSETS)]
        objects = [build / (Path(name).stem + ".obj") for name in (*SOURCES, FFT)]
        link_command = [linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", *map(str, objects),
                        str(build / (BENCH.stem + ".obj")), str(build / (EMPTY_ASSETS.stem + ".obj"))]
        manifest["commands"] = [compile_command, link_command]
        with (build / "build.log").open("w", encoding="utf-8") as log:
            for command in manifest["commands"]:
                log.write(subprocess.list2cmdline(command) + "\n")
                log.flush()
                subprocess.run(command, env=env, cwd=build, stdout=log, stderr=subprocess.STDOUT, check=True)
        manifest["executable_sha256"] = sha256(executable)
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        print(f"BUILD READY {executable} sha256={manifest['executable_sha256']}", flush=True)
    else:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest["executable_sha256"] != sha256(executable):
            parser.error("preserved benchmark executable changed")


        for path in (BENCH, EMPTY_ASSETS, Path(__file__).absolute(), ROOT / "scripts/test_r2t2_frontend_cache.py"):
            if manifest["inputs_sha256"][str(path)] != sha256(path):
                parser.error(f"benchmark input changed since build: {path}")
    if args.build_only:
        return 0

    result = subprocess.run([str(executable), str(args.wav.absolute()), str(args.iterations)],
                            env=env, cwd=build, capture_output=True, text=True, check=True)
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    measured = [row for row in rows if not row["warmup"]]
    if len(rows) != args.iterations + 2 or len(measured) != args.iterations:
        raise RuntimeError("benchmark iteration accounting mismatch")
    checksums = {row["checksum"] for row in rows}
    if len(checksums) != 1 or any(len(row["extracts"]) != 225 for row in rows):
        raise RuntimeError("fresh-frontend checksum/extract count changed across iterations")
    output = {"build": manifest, "fixture": {"path": str(args.wav.absolute()), "sha256": sha256(args.wav)},
              "warmups": 2, "measured_iterations": args.iterations, "rows": rows,
              "median_total_ns": statistics.median(row["total_ns"] for row in measured)}
    target = args.output or build / "benchmark.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    print(f"BENCH iterations={len(measured)} median_total_ns={output['median_total_ns']} output={target}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
