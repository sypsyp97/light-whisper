"""Compile and exercise the actual Windows R2T2 frontend, without models/CUDA.

Example: python scripts/test_r2t2_frontend_cache.py --source build/r2t2/source
The only interposition counts real FFT frames; DSP/audio/frontend code is real.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment


SOURCES = (
    "src/community_models/confucius4_r2t2/frontend_whisper.cpp",
    "src/framework/audio/dsp.cpp",
    "src/framework/audio/conversion.cpp",
    "src/framework/audio/resampling.cpp",
    "src/framework/audio/waveform_ops.cpp",
    "src/framework/audio/wav_reader.cpp",
    "src/framework/debug/trace.cpp",
)
FFT = "src/framework/audio/fft.cpp"
HEADERS = (
    "include/engine/community_models/confucius4_r2t2/frontend_whisper.h",
    "include/engine/community_models/confucius4_r2t2/assets.h",
    "include/engine/community_models/confucius4_r2t2/types.h",
    "include/engine/framework/audio/dsp.h",
    "include/engine/framework/audio/fft.h",
)
TEST = ROOT / "scripts/native_tests/r2t2_frontend_cache_test.cpp"
EMPTY_ASSETS = ROOT / "scripts/native_tests/r2t2_empty_assets.cpp"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path,
                        default=ROOT / "tmp/asr-engine-20261017/r2t2-cache-tests")
    parser.add_argument("--mode", choices=("all", "equivalence", "reuse"), default="all")
    parser.add_argument("--no-build", action="store_true", help="Run the previously compiled executable")
    args = parser.parse_args()
    source = args.source.absolute()
    build = args.build_dir.absolute()
    build.mkdir(parents=True, exist_ok=True)
    for path in (*[source / name for name in (*SOURCES, FFT)], TEST, EMPTY_ASSETS):
        if not path.is_file():
            parser.error(f"required source missing: {path}")

    executable = build / "r2t2_frontend_cache_test.exe"
    fingerprint = {str(path): sha256(path) for path in (
        *[source / name for name in (*SOURCES, FFT, *HEADERS)],
        TEST, EMPTY_ASSETS, Path(__file__).absolute())}
    compiled_manifest = build / "compiled-source.json"
    env, _ = build_environment()

    env = {key.upper(): value for key, value in env.items()}
    compiler = shutil.which("cl.exe", path=env["PATH"])
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable after vcvars")
    commands = []

    def run(command):
        commands.append(list(map(str, command)))
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace")
        with (build / "build.log").open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(list(map(str, command))) + "\n")
            log.write(result.stdout + result.stderr)
        if result.returncode:
            print(result.stdout + result.stderr, end="")
            raise RuntimeError(f"native compilation/link failed ({result.returncode}); see {build / 'build.log'}")

    if not args.no_build:
        (build / "build.log").write_text("", encoding="utf-8")
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                  "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS", f"/I{source / 'include'}",
                  f"/I{source / 'external/ggml/include'}", f"/Fo{build}/"]
        run([*common, *[str(source / name) for name in SOURCES], str(TEST), str(EMPTY_ASSETS)])

        run([*common, "/Dreal_fft_forward=r2t2_test_real_fft_forward", str(source / FFT)])
        objects = [build / (Path(name).stem + ".obj") for name in (*SOURCES, FFT)]
        run([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}",
             *map(str, objects), str(build / (TEST.stem + ".obj")),
             str(build / (EMPTY_ASSETS.stem + ".obj"))])
        compiled_manifest.write_text(json.dumps({
            "inputs": fingerprint, "executable_sha256": sha256(executable),
        }, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(compiled_manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executable_sha256"] != sha256(executable):
            parser.error("compiled inputs changed; rerun without --no-build")

    result = subprocess.run([str(executable), args.mode], cwd=build, env=env,
                            capture_output=True, text=True, errors="replace")
    output = result.stdout + result.stderr
    (build / f"{args.mode}.log").write_text(output, encoding="utf-8")
    evidence = {
        "source": str(source), "mode": args.mode, "returncode": result.returncode,
        "commands": commands, "test_sha256": sha256(TEST),
        "harness_sha256": sha256(Path(__file__)),
        "empty_assets_sha256": sha256(EMPTY_ASSETS),
        "native_sha256": {name: sha256(source / name) for name in (*SOURCES, FFT)},
        "native_header_sha256": {name: sha256(source / name) for name in HEADERS},
        "executable_sha256": sha256(executable),
    }
    (build / f"{args.mode}.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(output, end="")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
