"""Build and verify the native Windows x64 engine.tar.xz used by Tauri."""
import argparse
import hashlib
import importlib.metadata
import json
import lzma
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile

from build_r2t2_runtime import BUILD_ID, PATCH, SOURCE_REVISION, SOURCE_URL
from build_reuse_runtime import source_fingerprint as reuse_source_fingerprint
from native_runtime_build import ROOT, build_binary, source_fingerprint, verify_image

RESOURCES_DIR = ROOT / "src-tauri/resources"
OUTPUT_ARCHIVE = RESOURCES_DIR / "engine.tar.xz"
DISTRIBUTION = "engine-distribution.json"
ORT_PACKAGE = "microsoft.ml.onnxruntime.1.24.1.nupkg"
ORT_PACKAGE_SHA = "5fca4421974b450711520f17a27e527fec60a78c2b6f9db3b05c159d9b5a346d"
BASE_ASSETS = ("fireredvad_vad.onnx", "fireredvad_cmvn.json", "FireRedVAD-LICENSE.txt",
               "R2T2-MODEL-LICENSE.txt", "R2T2-CODE-LICENSE.txt", "RE-USE-LICENSE.txt",
               "Real-time-RE-USE-LICENSE.txt")
CRT = {"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"}
CUDA = {"cublas64_12.dll", "cublasLt64_12.dll", "cudart64_12.dll"}
NATIVE = {"audiocpp.dll", "reuse.dll", "audio.cpp-LICENSE.txt", "ggml-LICENSE.txt", "sentencepiece-LICENSE.txt"}
REQUIRED_BACKENDS = ("cpu", "cuda")
OPTIONAL_BACKENDS = ("vulkan",)


def metadata(path):
    with path.open("rb") as stream:
        return {"size": path.stat().st_size, "sha256": hashlib.file_digest(stream, "sha256").hexdigest()}


def static_sources():
    sources = {f"_internal/{name}": RESOURCES_DIR / name for name in BASE_ASSETS}
    for name in ("KissFFT-LICENSE.txt", "ONNXRuntime-LICENSE.txt"):
        sources[f"_internal/licenses/{name}"] = ROOT / "docs/licenses" / name
    for name in ("LICENSE-pocketfft.md", "LICENSE-numpy.txt"):
        sources[f"_internal/licenses/{name}"] = ROOT / "rust-runtime/crates/reuse/native" / name
    return sources


def static_pins():
    # Windows x64 binaries and notices from the pinned NuGet package and uv wheels.
    return {
        "onnxruntime.dll": "8a1aad8d59d02a5337d4e3f5bbd1158c3f7bf84fe3b3f0052f957dd3e75a91cb",
        "_internal/licenses/ONNXRuntime-ThirdPartyNotices.txt": "fb0af774b4d7cffc5b9d046f2aaeade2f37df2f80abf8033c95dfffcc77a8866",
        "kaldi-native-fbank-core.dll": "26445253459a25d06b1d07865f10127fb90705615f3cdea238790ee974e5ed91",
        "_internal/licenses/kaldi-native-fbank-LICENSE.txt": "3ddf9be5c28fe27dad143a5dc76eea25222ad1dd68934a047064e56ed2fa40c5",
        "_internal/_soundfile_data/libsndfile_x64.dll": "22518c16f9d13eda5ae5adf999c4740d7d16cc2d6b178b36dd4ac2f759a38559",
        "_internal/_soundfile_data/COPYING": "ad01ea5cd2755f6048383c8d54c88459cd6fcb17757c5c8892f8c5ea060f6140",
    }


def prepare_ort():
    if "LW_ORT_SDK" in os.environ:
        return Path(os.environ["LW_ORT_SDK"]).resolve(strict=True)
    cache = ROOT / "build/native-sdk"
    sdk = cache / "onnxruntime-1.24.1"
    if not sdk.is_dir():
        cache.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=cache) as temporary:
            stage = Path(temporary)
            package = stage / ORT_PACKAGE
            url = f"https://api.nuget.org/v3-flatcontainer/microsoft.ml.onnxruntime/1.24.1/{ORT_PACKAGE}"
            with urllib.request.urlopen(url, timeout=90) as response, package.open("wb") as out:
                shutil.copyfileobj(response, out)
            if metadata(package)["sha256"] != ORT_PACKAGE_SHA:
                raise ValueError("ONNX Runtime NuGet checksum mismatch")
            with zipfile.ZipFile(package) as archive:
                for name in archive.namelist():
                    if (name.startswith("build/native/include/") or name in
                        {"runtimes/win-x64/native/onnxruntime.lib", "runtimes/win-x64/native/onnxruntime.dll",
                         "LICENSE", "ThirdPartyNotices.txt"}):
                        archive.extract(name, stage / "sdk")
            try:
                (stage / "sdk").rename(sdk)
            except FileExistsError:
                if not sdk.is_dir():
                    raise
    return sdk


def verify_sdk(root, headers, library, expected):
    paths = [p for p in (root / headers).rglob("*") if p.is_file()] + [root / library]
    digest = hashlib.sha256()
    for path in sorted(paths, key=lambda p: p.relative_to(root).as_posix()):
        digest.update(path.relative_to(root).as_posix().encode() + b"\0" + bytes.fromhex(metadata(path)["sha256"]))
    if digest.hexdigest() != expected:
        raise ValueError(f"Native SDK headers/import library differ from release inputs: {root}")


def sdk_sources():
    ort = prepare_ort()
    wheel = importlib.metadata.distribution("kaldi-native-fbank")
    if wheel.version != "1.22.3":
        raise ValueError("Building requires the locked kaldi-native-fbank 1.22.3 Windows wheel")
    kaldi = Path(os.environ.get("LW_KALDI_SDK", wheel.locate_file("kaldi_native_fbank"))).resolve(strict=True)
    sound = Path(importlib.metadata.distribution("soundfile").locate_file("_soundfile_data"))
    verify_sdk(ort, "build/native/include", "runtimes/win-x64/native/onnxruntime.lib",
               "2cab0438770e82b8919fa7f62b5b24acddfba71931ecab42b9001e3e728dc852")
    verify_sdk(kaldi, "include", "lib/kaldi-native-fbank-core.lib",
               "ea7468d01a3613456df772059dee47153141d2d362dcebd443e862769de1fd57")
    os.environ.update(LW_ORT_SDK=str(ort), LW_KALDI_SDK=str(kaldi))
    return {
        "onnxruntime.dll": ort / "runtimes/win-x64/native/onnxruntime.dll",
        "_internal/licenses/ONNXRuntime-ThirdPartyNotices.txt": ort / "ThirdPartyNotices.txt",
        "kaldi-native-fbank-core.dll": kaldi / "lib/kaldi-native-fbank-core.dll",
        "_internal/licenses/kaldi-native-fbank-LICENSE.txt": Path(wheel.locate_file("kaldi_native_fbank-1.22.3.dist-info/LICENSE")),
        "_internal/_soundfile_data/libsndfile_x64.dll": sound / "libsndfile_x64.dll",
        "_internal/_soundfile_data/COPYING": sound / "COPYING",
    }


def native_root():
    return Path(os.environ.get("LW_NATIVE_RESOURCES", RESOURCES_DIR)) / "r2t2-native"


def native_layout(backend, manifest):
    provenance = {"backend": backend, "source": SOURCE_URL, "build_id": BUILD_ID, "revision": SOURCE_REVISION,
                  "patch_sha256": metadata(PATCH)["sha256"],
                  "reuse_source_sha256": None if backend == "vulkan" else reuse_source_fingerprint()}
    if any(manifest.get(key) != value for key, value in provenance.items()):
        raise ValueError(f"Stale or invalid {backend} native runtime provenance")
    files = manifest["files"]
    shared = manifest.get("shared_files", {})
    names = (NATIVE - {"reuse.dll"} if backend == "vulkan" else NATIVE) | CRT | (
        CUDA | {"CUDA-LICENSE.txt"} if backend == "cuda" else set())
    if files.keys() & shared.keys() or files.keys() | shared.keys() != names or shared.keys() - (CRT | CUDA):
        raise ValueError(f"Unexpected {backend} native runtime file table")
    for name, meta in (files | shared).items():
        if (set(meta) != {"size", "sha256"} or type(meta["size"]) is not int or meta["size"] <= 0
                or len(meta["sha256"]) != 64 or any(c not in "0123456789abcdef" for c in meta["sha256"])):
            raise ValueError(f"Invalid native metadata for {name}")
    return files, shared


def copy_native(engine_dir):
    shared_files = {}
    root = native_root()
    optional = [backend for backend in OPTIONAL_BACKENDS
                if (root / backend / "runtime-manifest.json").is_file()]
    for backend in (*REQUIRED_BACKENDS, *optional):
        source = native_root() / backend
        manifest = json.loads((source / "runtime-manifest.json").read_bytes())
        files, shared = native_layout(backend, manifest)
        if shared or {p.name for p in source.iterdir()} != set(files) | {"runtime-manifest.json"}:
            raise ValueError(f"Unexpected source package contents in {source}")
        package = engine_dir / "_internal/r2t2-native" / backend
        package.mkdir(parents=True)
        local, common = {}, {}
        for name, expected in files.items():
            previous = shared_files.get(name)
            use_root = name in (CRT | CUDA) and (previous is None or previous == expected)
            target = (engine_dir if use_root else package) / name
            if not use_root or previous is None:
                shutil.copyfile(source / name, target)
                actual = metadata(target)
            else:
                actual = metadata(source / name)
            if actual != expected:
                raise ValueError(f"Native dependency checksum mismatch: {source / name}")
            (common if use_root else local)[name] = expected
            if use_root:
                shared_files[name] = expected
        (package / "runtime-manifest.json").write_text(json.dumps({**manifest, "files":local, "shared_files":common},
                                                                 indent=2) + "\n", encoding="utf-8")


def stage_engine(engine_dir):
    engine_dir.mkdir(parents=True, exist_ok=True)
    sources = static_sources() | sdk_sources()
    pins = static_pins()
    for name, source in sources.items():
        target = engine_dir / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        if name in pins and metadata(target)["sha256"] != pins[name]:
            raise ValueError(f"Unexpected vendor file: {source}")
    build_binary("engine", engine_dir)
    copy_native(engine_dir)
    files = {p.relative_to(engine_dir).as_posix(): metadata(p) for p in sorted(engine_dir.rglob("*")) if p.is_file()}
    (engine_dir / DISTRIBUTION).write_text(json.dumps({"format":1, "source_sha256":source_fingerprint("engine"),
                                                       "files":files}, indent=2) + "\n", encoding="utf-8")


def verify_archive_sources(archive):
    fingerprint = source_fingerprint("engine")
    actual, documents = {}, {}
    seen = set()
    with lzma.open(archive, "rb") as payload, tarfile.open(fileobj=payload, mode="r|") as tar:
        for item in tar:
            name = item.name
            if (not item.isfile() or "\\" in name or ":" in name or any(p in ("", ".", "..") for p in name.split("/"))
                    or name.casefold() in seen):
                raise ValueError(f"Invalid or duplicate archive entry: {name}")
            seen.add(name.casefold())
            name = name.casefold()
            with tar.extractfile(item) as src:
                if name == DISTRIBUTION or name.endswith("/runtime-manifest.json"):
                    if item.size > 128 * 1024:
                        raise ValueError(f"Invalid manifest size: {name}")
                    data = src.read()
                    documents[name] = json.loads(data)
                    actual[name] = {"size":len(data), "sha256":hashlib.sha256(data).hexdigest()}
                elif name == "engine.exe":
                    if item.size > 256 * 1024 * 1024:
                        raise ValueError("Unexpected engine.exe size")
                    actual[name] = verify_image(src.read(), "engine", fingerprint)
                else:
                    actual[name] = {"size":item.size, "sha256":hashlib.file_digest(src, "sha256").hexdigest()}
        # TAR's end marker precedes the XZ footer. Read through it so a torn
        # final compressed block cannot pass an otherwise complete manifest.
        while tail := payload.read(1024 * 1024):
            if any(tail):
                raise ValueError("Unexpected data after the end of the engine TAR")
    manifest = documents[DISTRIBUTION]
    del actual[DISTRIBUTION]
    declared = manifest["files"]
    if (set(manifest) != {"format", "source_sha256", "files"} or type(manifest["format"]) is not int
            or manifest["format"] != 1 or manifest["source_sha256"] != fingerprint
            or len(declared) != len(actual) or {k.casefold(): v for k, v in declared.items()} != actual):
        raise ValueError("Engine archive payload hashes or release sources disagree with distribution manifest")
    sources, pins = static_sources(), static_pins()
    required = {"engine.exe"} | {k.casefold() for k in sources.keys() | pins.keys()}
    for name, path in sources.items():
        if actual.get(name.casefold()) != metadata(path):
            raise ValueError(f"Missing or stale bundled resource: {name}")
    for name, sha in pins.items():
        if actual.get(name.casefold(), {}).get("sha256") != sha:
            raise ValueError(f"Missing or corrupt vendor asset: {name}")
    for backend in (*REQUIRED_BACKENDS, *OPTIONAL_BACKENDS):
        prefix = f"_internal/r2t2-native/{backend}/"
        manifest_path = prefix + "runtime-manifest.json"
        if backend in OPTIONAL_BACKENDS and manifest_path not in documents:
            continue
        if manifest_path not in documents:
            raise ValueError(f"Missing bundled {backend} runtime manifest")
        native = documents[manifest_path]
        files, shared = native_layout(backend, native)
        reference = native_root() / backend / "runtime-manifest.json"
        if reference.exists():
            original = json.loads(reference.read_bytes())
            merged = {k:v for k,v in native.items() if k != "shared_files"}
            if original != {**merged, "files":files | shared}:
                raise ValueError(f"Packaged {backend} manifest differs from the selected build")
        required.add(manifest_path)
        expected = {prefix + name: meta for name, meta in files.items()} | shared
        for name, meta in expected.items():
            if actual.get(name.casefold()) != meta:
                raise ValueError(f"Native dependency missing or corrupt: {name}")
        required.update(k.casefold() for k in expected)
    if set(actual) != required or not {k.casefold() for k in CUDA} <= set(actual):
        raise ValueError("Engine archive contains missing, extra or misplaced runtime files")


def create_tar_xz(engine_dir, output):
    files = sorted((p for p in engine_dir.rglob("*") if p.is_file()),
                   key=lambda p: (-p.stat().st_size, p.relative_to(engine_dir).as_posix()))
    seven_zip = os.environ.get("SEVEN_ZIP") or shutil.which("7z")
    if not seven_zip and Path(r"C:\Program Files\7-Zip\7z.exe").is_file():
        seven_zip = r"C:\Program Files\7-Zip\7z.exe"
    tar_path = output.with_suffix(".tar") if seven_zip else output
    kwargs = {} if seven_zip else {"preset": 6}
    with tarfile.open(tar_path, "w" if seven_zip else "w:xz", **kwargs) as archive:
        for path in files:
            archive.add(path, arcname=path.relative_to(engine_dir).as_posix(), recursive=False)
    if seven_zip:
        subprocess.run([seven_zip, "a", "-txz", "-mx=9", "-mmt=2", "-md=256m", "-mf=BCJ",
                        str(output.resolve()), str(tar_path.resolve())], check=True)
    return output.stat().st_size / (1024 * 1024)


def main():
    fingerprint = source_fingerprint("engine")
    OUTPUT_ARCHIVE.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".engine-build-", dir=OUTPUT_ARCHIVE.parent) as temporary:
        stage = Path(temporary)
        engine_dir = stage / "engine"
        engine_dir.mkdir()
        stage_engine(engine_dir)
        archive = stage / "engine.tar.xz"
        create_tar_xz(engine_dir, archive)
        verify_archive_sources(archive)
        if source_fingerprint("engine") != fingerprint:
            raise ValueError("Engine inputs changed during the build")
        os.replace(archive, OUTPUT_ARCHIVE)
    print(f"Built {OUTPUT_ARCHIVE} ({OUTPUT_ARCHIVE.stat().st_size} bytes)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-archive", type=Path)
    args = parser.parse_args()
    try:
        if args.verify_archive is None:
            main()
        else:
            verify_archive_sources(args.verify_archive)
    except Exception as error:
        parser.exit(1, f"Engine build or verification failed: {error}\n")
