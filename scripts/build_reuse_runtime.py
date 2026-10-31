"""Link the FP32 RE-USE adapter against an existing pinned GGML build.

No downloads, GGML rebuilds, model conversion, or shipped-resource changes.
"""
import argparse
import hashlib
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import PATCH, ROOT, SOURCE_REVISION, build_environment


def source_fingerprint():
    digest = hashlib.sha256()
    for path in (ROOT / "scripts/native/reuse_native.cpp", Path(__file__)):
        digest.update(path.read_bytes().replace(b"\r\n", b"\n"))
    return digest.hexdigest()


def build(backend, source, native_build_dir, output):
    source, native_build_dir, output = map(Path.absolute, (source, native_build_dir, output))
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if revision != SOURCE_REVISION:
        raise RuntimeError(f"Expected audio.cpp {SOURCE_REVISION}, found {revision}")
    modified = subprocess.check_output(
        ["git", "-C", str(source), "diff", "--binary", "--unified=4", "HEAD"], text=True).strip()
    if modified != PATCH.read_text(encoding="utf-8").strip():
        raise RuntimeError("Native source must contain exactly the reviewed R2T2 patch")
    cache = {}
    for line in (native_build_dir / "CMakeCache.txt").read_text(encoding="utf-8").splitlines():
        if "=" in line and not line.startswith(("#", "//")):
            key, value = line.split("=", 1)
            cache[key.split(":", 1)[0]] = value
    if Path(cache["CMAKE_HOME_DIRECTORY"]).resolve() != source.resolve():
        raise RuntimeError("GGML build and requested source differ")
    cuda_build = cache.get("ENGINE_ENABLE_CUDA") == "ON"
    if cuda_build != (backend == "cuda"):
        raise RuntimeError("Requested backend differs from the existing GGML build")
    libraries = [native_build_dir / f"ggml/src/{name}.lib" for name in ("ggml", "ggml-base", "ggml-cpu")]
    cuda_root = None
    if backend == "cuda":
        cuda_root = Path(cache["CUDAToolkit_ROOT"])
        libraries += [native_build_dir / "ggml/src/ggml-cuda/ggml-cuda.lib"]
        libraries += [cuda_root / f"lib/x64/{name}.lib" for name in ("cublas", "cublasLt", "cuda", "cudart")]
    for library in libraries:
        if not library.is_file():
            raise RuntimeError(f"Required existing native library missing: {library}")
    env, _ = build_environment()
    output.mkdir(parents=True, exist_ok=True)
    compiler = shutil.which("cl.exe", path=env["PATH"])
    if not compiler:
        raise RuntimeError("Visual Studio C++ compiler is unavailable")
    command = [compiler, "/nologo", "/O2", "/std:c++17", "/MD", "/EHsc", "/LD", "/utf-8",
               "/D_CRT_SECURE_NO_WARNINGS", f"/I{source / 'external/ggml/include'}", f"/I{source / 'include'}"]
    if backend == "cuda":
        command += ["/DREUSE_USE_CUDA"]
    command += [str(ROOT / "scripts/native/reuse_native.cpp"), str(source / "src/framework/io/safetensors.cpp"),
                "/link", f"/OUT:{output / 'reuse.dll'}", *map(str, libraries)]
    if cuda_root:
        command += [f"/LIBPATH:{cuda_root / 'lib/x64'}"]
    command += ["advapi32.lib", "/INCREMENTAL:NO"]
    subprocess.run(command, cwd=output, env=env, check=True)
    if cuda_root:
        for pattern in ("cublas64_*.dll", "cublasLt64_*.dll", "cudart64_*.dll"):
            matches = list((cuda_root / "bin").glob(pattern))
            if len(matches) != 1:
                raise RuntimeError(f"Expected one CUDA dependency matching {pattern}")
            shutil.copy2(matches[0], output / matches[0].name)
    print(f"RE-USE {backend}: {output / 'reuse.dll'}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=("cpu", "cuda"), required=True)
    default_source = ROOT / "build/r2t2/shipping-source"
    parser.add_argument("--source", type=Path, default=default_source if default_source.is_dir() else ROOT / "build/r2t2/source")
    parser.add_argument("--native-build-dir", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    build(args.backend, args.source, args.native_build_dir or ROOT / f"build/r2t2/{args.backend}",
          args.output or ROOT / f"tmp/reuse-investigation/native-{args.backend}")


if __name__ == "__main__":
    main()
