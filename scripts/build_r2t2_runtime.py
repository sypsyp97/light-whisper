"""Build the shared Windows Qwen3-ASR/R2T2 library without changing global tools.

Requires Visual Studio C++ build tools, CMake/Ninja and (for CUDA) a compatible
CUDA SDK. Models are downloaded separately; no model weights enter the bundle.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE_URL = "https://github.com/0xShug0/audio.cpp.git"
SOURCE_REVISION = "6c70f32d0d90a29a9863e556bd9712ce622f868b"
BUILD_ID = "light-whisper-asr-13"
PATCH = ROOT / "scripts/patches/audio-cpp-r2t2-windows-streaming.patch"


def run(args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def digest(path):
    with open(path, "rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def build_environment():
    if os.name != "nt":
        raise RuntimeError("This packaging script builds the Windows native runtime")
    vswhere = Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = run(
        [vswhere, "-latest", "-products", "*", "-requires",
         "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"],
        capture_output=True, text=True,
    ).stdout.strip()
    vcvars = Path(installation) / "VC/Auxiliary/Build/vcvars64.bat"
    if not installation or not vcvars.is_file():
        raise RuntimeError("Visual Studio x64 C++ tools are required")
    # The command is a fixed system-installed batch path, not user shell input.
    output = subprocess.run(
        f'cmd.exe /d /s /c ""{vcvars}" >nul && set"', check=True,
        capture_output=True, text=True, errors="replace",
    ).stdout
    # Windows names are case-insensitive, unlike this plain Python dictionary.
    env = {key.upper(): value for key, value in os.environ.items()}
    for line in output.splitlines():
        key, separator, value = line.partition("=")
        if separator and key:
            env[key.upper()] = value
    if "VCTOOLSREDISTDIR" in env:
        env["VCToolsRedistDir"] = env.pop("VCTOOLSREDISTDIR")
    ninja = Path(installation) / "Common7/IDE/CommonExtensions/Microsoft/CMake/Ninja/ninja.exe"
    return env, ninja


def prepare_source(source):
    if not source.exists():
        source.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "--no-checkout", "--filter=blob:none",
             "--config", "core.autocrlf=false", SOURCE_URL, source])
        run(["git", "-C", source, "checkout", "--detach", SOURCE_REVISION])
    actual = run(["git", "-C", source, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    if actual != SOURCE_REVISION:
        raise RuntimeError(f"Expected audio.cpp {SOURCE_REVISION}, found {actual}; refusing to reset source")
    diff_command = ["git", "-C", source, "diff", "--binary", "--unified=4", "HEAD"]
    reviewed = PATCH.read_text(encoding="utf-8").strip()
    modified = run(diff_command, capture_output=True, text=True).stdout.strip()
    if modified == reviewed:
        return
    if modified:
        raise RuntimeError("Native source contains changes other than the reviewed R2T2 patch")
    # The reviewed patch is LF-only, while this pinned upstream has mixed CRLF
    # files. Restore added-line endings without rewriting untouched source lines.
    sections = []
    for section in re.split(rb"(?=^diff --git )", (reviewed + "\n").encode("utf-8"), flags=re.M):
        if not section:
            continue
        name = re.search(rb"^--- a/(.+)$", section, re.M).group(1).decode("utf-8")
        if b"\r\n" in (source / name).read_bytes():
            section = b"".join(
                line[:-1] + b"\r\n" if line.startswith(b"+") and not line.startswith(b"+++ ")
                and line.endswith(b"\n") else line for line in section.splitlines(keepends=True)
            )
        sections.append(section)
    with tempfile.TemporaryDirectory(prefix="r2t2-source-patch-") as temporary:
        patch = Path(temporary) / "reviewed.patch"
        patch.write_bytes(b"".join(sections))
        apply = ["git", "-c", "core.autocrlf=false", "-C", source, "apply",
                 "--ignore-space-change", "--whitespace=nowarn"]
        run([*apply, "--check", patch])
        run([*apply, patch])
    if run(diff_command, capture_output=True, text=True).stdout.strip() != reviewed:
        raise RuntimeError("Native source contains changes other than the reviewed R2T2 patch")


def _assemble_runtime(source, build, output, backend, cuda_root, env, architectures):
    library = build / "bin/audiocpp.dll"
    if not library.is_file():
        raise RuntimeError(f"Native DLL missing: {library}")
    output.mkdir(parents=True, exist_ok=True)
    shutil.copy2(library, output / library.name)
    shutil.copy2(source / "LICENSE", output / "audio.cpp-LICENSE.txt")
    shutil.copy2(source / "external/ggml/LICENSE", output / "ggml-LICENSE.txt")
    shutil.copy2(source / "external/sentencepiece/LICENSE", output / "sentencepiece-LICENSE.txt")
    redist = Path(env["VCToolsRedistDir"]) / "x64"
    for name in ("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"):
        candidates = list(redist.glob(f"*/{name}"))
        if not candidates:
            raise RuntimeError(f"Visual C++ redistributable missing: {name}")
        shutil.copy2(candidates[0], output / name)
    if backend == "cuda":
        for pattern in ("cublas64_*.dll", "cublasLt64_*.dll", "cudart64_*.dll"):
            matches = list((cuda_root / "bin").glob(pattern))
            if len(matches) != 1:
                raise RuntimeError(f"Expected one CUDA library matching {pattern}")
            shutil.copy2(matches[0], output / matches[0].name)
        license_path = cuda_root / "LICENSE"
        if not license_path.is_file():
            raise RuntimeError("CUDA redistribution license is missing")
        shutil.copy2(license_path, output / "CUDA-LICENSE.txt")
    files = {item.name: {"size": item.stat().st_size, "sha256": digest(item)}
             for item in sorted(output.iterdir()) if item.is_file() and item.name != "runtime-manifest.json"}
    manifest = {"build_id": BUILD_ID, "source": SOURCE_URL, "revision": SOURCE_REVISION,
                "patch_sha256": digest(PATCH), "backend": backend,
                "cuda_architectures": architectures if backend == "cuda" else None, "files": files}
    (output / "runtime-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def copy_runtime(source, build, output, backend, cuda_root, env, architectures):
    output = output.absolute()
    parent = output.parent
    if output.resolve().parent != parent.resolve() or output.is_symlink():
        raise RuntimeError("Native output must be a direct, non-linked child directory")
    if output.exists() and (not output.is_dir() or (
            any(output.iterdir()) and not (output / "runtime-manifest.json").is_file())):
        raise RuntimeError("Refusing to replace an unmanaged native output directory")
    parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".r2t2-runtime-", dir=parent))
    preserve_backup = False
    try:
        assembled = staging / "new"
        _assemble_runtime(source, build, assembled, backend, cuda_root, env, architectures)
        previous = staging / "previous"
        if output.exists():
            output.rename(previous)
        try:
            assembled.rename(output)
        except Exception:
            if previous.exists():
                try:
                    previous.rename(output)
                except Exception as error:
                    preserve_backup = True
                    raise RuntimeError(f"Previous native package retained at {previous}") from error
            raise
    finally:
        # Only remove the unique staging directory created by this invocation.
        if not preserve_backup and staging.resolve().parent == parent.resolve():
            shutil.rmtree(staging)
    print(f"R2T2 {backend} runtime: {output}")


def resolved_cuda_architectures(configure_output):
    prefix = "-- Using CMAKE_CUDA_ARCHITECTURES="
    resolved = set()
    for line in configure_output.splitlines():
        if not line.startswith(prefix):
            continue
        architectures, separator, _native = line[len(prefix):].partition(" CMAKE_CUDA_ARCHITECTURES_NATIVE=")
        if not separator or not all(re.fullmatch(r"[0-9]+[af]?(?:-(?:real|virtual))?", architecture)
                                    for architecture in architectures.split(";")):
            raise RuntimeError(f"Invalid resolved CUDA architectures: {line}")
        resolved.add(architectures)
    if len(resolved) != 1:
        raise RuntimeError("CMake must report one consistent, concrete CUDA architecture list")
    return resolved.pop()


def portable_cuda_architectures(architectures):
    targets = architectures.split(";")
    # Keep upstream PTX and future targets; restore native code shipped by asr11.
    for architecture in ("80", "90"):
        if (f"{architecture}-virtual" in targets
                and architecture not in targets and f"{architecture}-real" not in targets):
            targets.append(f"{architecture}-real")
    return ";".join(targets)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=("cpu", "cuda"), required=True)
    parser.add_argument("--source", type=Path, default=ROOT / "build/r2t2/source")
    parser.add_argument("--build-dir", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cuda-root", type=Path)
    parser.add_argument("--cuda-architectures")
    parser.add_argument("--parallel", type=int, default=6)
    args = parser.parse_args()
    # Keep the caller's spelling: resolving MSIX aliases changes compiler paths
    # and makes CMake discard its configured cache as if the compiler changed.
    source = args.source.absolute()
    build = (args.build_dir or ROOT / f"build/r2t2/{args.backend}").absolute()
    output = (args.output or ROOT / f"src-tauri/resources/r2t2-native/{args.backend}").absolute()
    cuda_root = args.cuda_root or (Path(os.environ["CUDA_PATH"]) if os.environ.get("CUDA_PATH") else None)
    if args.backend == "cuda" and (cuda_root is None or not (cuda_root / "bin/nvcc.exe").is_file()):
        parser.error("CUDA build needs --cuda-root or CUDA_PATH (validated with CUDA 12.9)")
    prepare_source(source)
    env, ninja = build_environment()
    architectures = None
    command = ["cmake", "-S", source, "-B", build, "-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release",
               f"-DCMAKE_MAKE_PROGRAM={ninja}", "-DAUDIOCPP_MODEL_SET=custom",
               "-DAUDIOCPP_MODELS=confucius4_r2t2,qwen3_asr", "-DAUDIOCPP_BUILD_C_API=ON",
               "-DAUDIOCPP_DEPLOYMENT_BUILD=ON", "-DENGINE_ENABLE_NATIVE_CPU=OFF",
               "-DENGINE_ENABLE_VULKAN=OFF",
               "-DENGINE_ENABLE_LLAMAFILE=OFF", f"-DAUDIOCPP_VERSION={BUILD_ID}"]
    if args.backend == "cuda":
        cuda_root = cuda_root.absolute()
        env = {key: value for key, value in env.items() if key.upper() != "CUDAARCHS"}
        env["CUDA_PATH"] = str(cuda_root)
        env["PATH"] = str(cuda_root / "bin") + os.pathsep + env["PATH"]
        command += ["-DENGINE_ENABLE_CUDA=ON", f"-DCUDAToolkit_ROOT={cuda_root}",
                    f"-DCMAKE_CUDA_COMPILER={cuda_root / 'bin/nvcc.exe'}",
                    "-U", "CMAKE_CUDA_ARCHITECTURES"]
        if args.cuda_architectures is not None:
            command += [f"-DCMAKE_CUDA_ARCHITECTURES={args.cuda_architectures}"]
        try:
            configured = run(command, env=env, stdout=subprocess.PIPE, text=True, errors="replace")
            print(configured.stdout, end="", flush=True)
            architectures = resolved_cuda_architectures(configured.stdout)
            if args.cuda_architectures is None:
                planned = portable_cuda_architectures(architectures)
                if planned != architectures:
                    configured = run([*command, f"-DCMAKE_CUDA_ARCHITECTURES={planned}"],
                                     env=env, stdout=subprocess.PIPE, text=True, errors="replace")
                    print(configured.stdout, end="", flush=True)
                    architectures = resolved_cuda_architectures(configured.stdout)
                    if architectures != planned:
                        raise RuntimeError("CMake changed the planned portable CUDA architecture list")
        except subprocess.CalledProcessError as error:
            print(error.stdout or "", end="", flush=True)
            raise
    else:
        command += ["-DENGINE_ENABLE_CUDA=OFF"]
        run(command, env=env)
    run(["cmake", "--build", build, "--target", "audiocpp", "--parallel", str(args.parallel)], env=env)
    copy_runtime(source, build, output, args.backend, cuda_root, env, architectures)


if __name__ == "__main__":
    main()
