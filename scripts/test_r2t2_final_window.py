"""Compile and exercise the native rolling decode's actual pre-extract statements.

Pass --source session.cpp to use a prepared audio.cpp checkout. By default
the owning method is read from the tracked native build patch. The standalone
fixture observes the exact PCM that would be passed to frontend_.extract.
"""
import argparse
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent


def acoustic_input(source):
    start = source.index("R2T2ASRSession::decode_rolling_stream_chunk(bool final_flush) {")
    trim = source.index("trim_rolling_window();", start)
    copy = source.index("accum.samples = audio_accum_;", trim)
    extract = source.index("const auto features = frontend_.extract(accum);", copy)
    constants = re.findall(r"^constexpr [^;\n]+ kFinishContextMs = [^;]+;", source, re.MULTILINE)
    return "\n".join(constants) + "\n" + source[copy:extract]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    if args.source:
        source = args.source.read_text(encoding="utf-8")
    else:
        patch = HERE / "patches/audio-cpp-r2t2-windows-streaming.patch"
        source = "\n".join(line[1:] for line in patch.read_text(encoding="utf-8").splitlines()
                           if line.startswith(("+", " ")))

    with tempfile.TemporaryDirectory() as temp:
        directory = Path(temp)
        (directory / "acoustic_input.inc").write_text(acoustic_input(source), encoding="utf-8")
        fixture = HERE / "native_tests/r2t2_final_window_test.cpp"
        exe = directory / "test.exe"
        compiler = shutil.which("g++") or shutil.which("clang++")
        if compiler:
            subprocess.run([compiler, "-std=c++17", "-I", directory, fixture, "-o", exe], check=True)
        else:
            from build_r2t2_runtime import build_environment

            env, _ = build_environment()
            subprocess.run(["cl", "/nologo", "/EHsc", "/std:c++17", f"/I{directory}", fixture,
                            f"/Fe:{exe}"], check=True, cwd=directory, env=env)
        return subprocess.run([exe]).returncode


if __name__ == "__main__":
    raise SystemExit(main())
