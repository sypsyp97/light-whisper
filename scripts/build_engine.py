#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
自动化 PyInstaller 构建脚本。

流程：
1. PyInstaller --onedir 构建 engine.exe
2. 删除开发期库文件
3. 压缩为 engine.tar.xz（适配 NSIS 2GB 限制）
4. 输出到 src-tauri/resources/engine.tar.xz

必须在项目 .venv 环境中运行。
"""

import filecmp
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import time
from pathlib import Path
from build_r2t2_runtime import BUILD_ID as R2T2_BUILD_ID, SOURCE_REVISION as R2T2_REVISION, PATCH as R2T2_PATCH
from build_reuse_runtime import source_fingerprint as reuse_source_fingerprint

PROJECT_ROOT = Path(__file__).resolve().parent.parent
RESOURCES_DIR = PROJECT_ROOT / "src-tauri" / "resources"
DIST_DIR = RESOURCES_DIR / "python-dist"
ENTRY_SCRIPT = RESOURCES_DIR / "engine.py"
OUTPUT_ARCHIVE = RESOURCES_DIR / "engine.tar.xz"
WINDOWS_MANIFEST = PROJECT_ROOT / "src-tauri" / "windows-app-manifest.xml"


ADD_DATA_FILES = [
    "engine.py",
    "qwen3_asr_server.py",
    "qwen3_native.py",
    "r2t2_asr_server.py",
    "r2t2_native.py",
    "r2t2_stream.py",
    "r2t2_segmented.py",
    "R2T2-MODEL-LICENSE.txt",
    "R2T2-CODE-LICENSE.txt",
    "firered_vad.py",
    "fireredvad_vad.onnx",
    "fireredvad_cmvn.json",
    "FireRedVAD-LICENSE.txt",
    "download_models.py",
    "server_common.py",
    "asr_stream_server.py",
    "gpu_idle.py",
    "reuse_enhancement.py",
    "RE-USE-LICENSE.txt",
    "Real-time-RE-USE-LICENSE.txt",
    "hf_cache_utils.py",
]

HIDDEN_IMPORTS = [
    "gpu_idle",
    "reuse_enhancement",
    "ctypes",
    "requests",
    "certifi",
    "numpy",
    "huggingface_hub",
    "huggingface_hub.utils",
    "kaldi_native_fbank",
    "_kaldi_native_fbank",
    "tqdm",
    "soundfile",
    "onnxruntime",
]


COLLECT_ALL = [
    "kaldi_native_fbank",
]

EXCLUDE_MODULES = [


    "transcribe_cpp",
    "transcribe_cpp_native",
    "transcribe_cpp_native_cu12",
    "faster_whisper",
    "ctranslate2",
    "av",
    "tokenizers",
    "librosa",
    "scipy",
    "numba",
    "llvmlite",
    "matplotlib",
    "tkinter",
    "PyQt5",
    "PyQt6",
    "PySide2",
    "PySide6",
    "IPython",
    "notebook",
    "pytest",
    "sphinx",
    "docutils",
    "tensorboard",
    "triton",

    "transformers",
    "modelscope",
    "datasets",
    "pyarrow",
    "pandas",
    "onnx",

    "wandb",
    "lightning",
    "pytorch_lightning",
    "torchmetrics",
    "sentry_sdk",
    "umap",
    "sklearn",
    "aliyunsdkcore",
    "oss2",
]



STRIP_DEV_ARTIFACT_PATTERNS = [
    "*.lib",
    "*.pdb",
]


def find_7z_executable() -> str | None:

    candidates = [
        os.environ.get("SEVEN_ZIP"),
        shutil.which("7z"),
        r"C:\Program Files\7-Zip\7z.exe",
        r"C:\Program Files (x86)\7-Zip\7z.exe",
    ]

    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return str(candidate)

    return None


def remove_tree(path: Path, *, warn_only: bool, retries: int = 5, delay: float = 1.0) -> None:

    if not path.exists():
        return

    last_error: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            shutil.rmtree(path)
            return
        except PermissionError as exc:
            last_error = exc
            if attempt == retries:
                break
            print(
                f"警告: 删除 {path} 失败（文件占用），{delay:.0f} 秒后重试 "
                f"{attempt}/{retries} ...",
                file=sys.stderr,
            )
            time.sleep(delay)

    message = f"清理目录失败: {path}\n{last_error}"
    if warn_only:
        print(f"警告: {message}", file=sys.stderr)
        return
    raise RuntimeError(message) from last_error


def remove_file(path: Path, *, warn_only: bool, retries: int = 5, delay: float = 1.0) -> None:

    if not path.exists():
        return

    last_error: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            path.unlink(missing_ok=True)
            return
        except PermissionError as exc:
            last_error = exc
            if attempt == retries:
                break
            print(
                f"警告: 删除 {path} 失败（文件占用），{delay:.0f} 秒后重试 "
                f"{attempt}/{retries} ...",
                file=sys.stderr,
            )
            time.sleep(delay)

    message = f"清理文件失败: {path}\n{last_error}"
    if warn_only:
        print(f"警告: {message}", file=sys.stderr)
        return
    raise RuntimeError(message) from last_error


def get_size_mb(path: Path) -> float:
    total = sum(f.stat().st_size for f in path.rglob("*") if f.is_file())
    return total / (1024 * 1024)


def strip_dev_artifacts(engine_dir: Path) -> float:

    internal_dir = engine_dir / "_internal"
    saved = 0.0
    for pattern in STRIP_DEV_ARTIFACT_PATTERNS:
        for match in internal_dir.rglob(pattern):
            if not match.is_file():
                continue
            size = match.stat().st_size / (1024 * 1024)
            print(f"  删除: {match.relative_to(engine_dir)} ({size:.0f} MB)")
            match.unlink()
            saved += size

    for match in engine_dir.rglob("*"):
        if not match.is_file() or match.name.casefold() not in {"ggml-vulkan.dll", "vulkan-1.dll"}:
            continue
        size = match.stat().st_size / (1024 * 1024)
        print(f"  删除: {match.relative_to(engine_dir)} ({size:.0f} MB)")
        match.unlink()
        saved += size

    return saved


def share_native_runtime_dlls(engine_dir: Path) -> float:





    internal = engine_dir / "_internal"
    shared_names = {
        "cublas64_12.dll", "cublasLt64_12.dll", "cudart64_12.dll",
        "msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll",
    }
    saved = 0
    local = internal / "kaldi_native_fbank" / "lib" / "kaldi-native-fbank-core.dll"
    common = internal / "kaldi-native-fbank-core.dll"
    if local.is_file() and common.is_file() and filecmp.cmp(local, common, shallow=False):
        saved += local.stat().st_size
        local.unlink()
    for manifest_path in (internal / "r2t2-native").glob("*/runtime-manifest.json"):
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        shared = {}
        for name in shared_names & manifest["files"].keys():
            local = manifest_path.parent / name
            common = internal / name
            if not common.is_file() or not local.is_file():
                continue
            if common.stat().st_size != local.stat().st_size:
                continue
            with local.open("rb") as stream:
                local_hash = hashlib.file_digest(stream, "sha256").hexdigest()
            with common.open("rb") as stream:
                common_hash = hashlib.file_digest(stream, "sha256").hexdigest()
            if common_hash != local_hash:
                continue
            metadata = manifest["files"][name]
            if metadata["sha256"] != local_hash or metadata["size"] != local.stat().st_size:
                raise RuntimeError(f"Native dependency checksum mismatch: {local}")
            shared[name] = metadata
        for name in shared:
            local = manifest_path.parent / name
            saved += local.stat().st_size
            local.unlink()
            del manifest["files"][name]
        if shared:
            manifest.setdefault("shared_files", {}).update(shared)
            manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return saved / (1024 * 1024)


def create_tar_xz_with_python(engine_dir: Path, output: Path) -> float:

    print(f"正在压缩到 {output.name} ...")
    files = [f for f in engine_dir.rglob("*") if f.is_file()]
    files.sort(key=lambda f: (-f.stat().st_size, f.relative_to(engine_dir).as_posix()))
    total = len(files)

    with tarfile.open(output, mode="w:xz", preset=9) as tf:
        for i, filepath in enumerate(files, 1):
            arcname = filepath.relative_to(engine_dir)
            tf.add(filepath, arcname=str(arcname), recursive=False)
            if i % 500 == 0 or i == total:
                print(f"  压缩进度: {i}/{total} ({i * 100 // total}%)")

    return output.stat().st_size / (1024 * 1024)


def create_tar_xz_with_7z(engine_dir: Path, output: Path, seven_zip: str) -> float:




    output = output.resolve()
    tar_path = output.with_suffix("")
    files = [f for f in engine_dir.rglob("*") if f.is_file()]
    files.sort(key=lambda f: (-f.stat().st_size, f.relative_to(engine_dir).as_posix()))
    total = len(files)
    if not files:
        raise RuntimeError("engine 目录为空，无法创建归档")

    print(f"正在用 7-Zip 构建 {output.name} ...")


    print(f"  阶段 1/2: 打包 tar ({total} 文件)")
    with tarfile.open(tar_path, mode="w") as tf:
        for i, filepath in enumerate(files, 1):
            arcname = filepath.relative_to(engine_dir)
            tf.add(filepath, arcname=str(arcname), recursive=False)
            if i % 500 == 0 or i == total:
                print(f"  打包进度: {i}/{total} ({i * 100 // total}%)")


    tar_mb = tar_path.stat().st_size / (1024 * 1024)
    print(f"  阶段 2/2: 7-Zip 多线程压缩 xz ({tar_mb:.0f} MB)")

    cmd = [seven_zip, "a", "-txz", "-mx=9", "-mmt=2", "-ms=on", "-md=256m",
           "-mf=BCJ", str(output), str(tar_path)]
    try:
        subprocess.run(cmd, check=True, cwd=output.parent)
    finally:



        remove_file(tar_path, warn_only=True)

    return output.stat().st_size / (1024 * 1024)


def create_tar_xz(engine_dir: Path, output: Path) -> float:

    seven_zip = find_7z_executable()
    if seven_zip:
        print(f"检测到 7-Zip: {seven_zip}")
        return create_tar_xz_with_7z(engine_dir, output, seven_zip)

    print("警告: 未找到 7-Zip，回退到 Python 单线程 tar.xz 压缩", file=sys.stderr)
    return create_tar_xz_with_python(engine_dir, output)


def validate_r2t2_runtime():

    root = RESOURCES_DIR / "r2t2-native"
    for backend in ("cpu", "cuda"):
        directory = root / backend
        manifest_path = directory / "runtime-manifest.json"
        if not manifest_path.is_file():
            raise RuntimeError(
                f"R2T2 {backend} runtime missing; run scripts/build_r2t2_runtime.py --backend {backend}"
            )
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        patch_sha = hashlib.sha256(R2T2_PATCH.read_bytes()).hexdigest()
        if (manifest.get("build_id") != R2T2_BUILD_ID
                or manifest.get("revision") != R2T2_REVISION
                or manifest.get("patch_sha256") != patch_sha
                or manifest.get("reuse_source_sha256") != reuse_source_fingerprint()):
            raise RuntimeError(f"Stale R2T2 {backend} runtime; rebuild the native package")
        files = manifest.get("files", {})
        if manifest.get("backend") != backend or not {"audiocpp.dll", "reuse.dll"} <= files.keys():
            raise RuntimeError(f"Invalid R2T2 runtime manifest: {manifest_path}")
        actual_names = {item.name for item in directory.iterdir()}
        if actual_names != set(files) | {"runtime-manifest.json"}:
            raise RuntimeError(f"Unexpected native runtime contents: {directory}")
        for name, expected in files.items():
            if Path(name).name != name:
                raise RuntimeError(f"Invalid native runtime filename: {name}")
            path = directory / name
            if not path.is_file() or path.stat().st_size != expected["size"]:
                raise RuntimeError(f"Missing or incomplete native runtime file: {path}")
            with path.open("rb") as source:
                actual = hashlib.file_digest(source, "sha256").hexdigest()
            if actual != expected["sha256"]:
                raise RuntimeError(f"Native runtime checksum mismatch: {path}")
    return root


def verify_archive_sources(archive: Path) -> None:

    expected = {
        f"_internal/{name}": (RESOURCES_DIR / name).read_bytes().replace(b"\r\n", b"\n")
        for name in ADD_DATA_FILES if name.endswith(".py")
    }
    seen = set()
    native_paths = {
        f"_internal/r2t2-native/{backend}/{filename}"
        for backend in ("cpu", "cuda")
        for filename in ("runtime-manifest.json", "audiocpp.dll", "reuse.dll")
    }
    native_prefixes = tuple(f"_internal/r2t2-native/{backend}/" for backend in ("cpu", "cuda"))
    native = {}
    native_names = set()
    retired_packages = {"transcribe_cpp", "transcribe_cpp_native", "transcribe_cpp_native_cu12"}
    with tarfile.open(archive, mode="r|xz") as packaged:
        for member in packaged:
            name = member.name.removeprefix("./")
            normalized = member.name.replace("\\", "/").casefold().lstrip("./")
            for component in normalized.split("/"):
                distribution = component.removesuffix(".dist-info").rsplit("-", 1)[0].replace("-", "_")
                if (component in retired_packages
                        or component.endswith(".dist-info") and distribution in retired_packages):
                    raise RuntimeError(f"Retired transcribe.cpp package in bundled archive: {name}; rebuild the engine")
            if (normalized == "_internal/r2t2-native/vulkan"
                    or normalized.startswith("_internal/r2t2-native/vulkan/")
                    or normalized.rsplit("/", 1)[-1] in {"ggml-vulkan.dll", "vulkan-1.dll"}):
                raise RuntimeError(f"Retired Vulkan runtime in bundled archive: {name}; rebuild the engine")
            native_name = name.casefold()
            if (native_name in native_paths or native_name.startswith(native_prefixes)
                    or native_name.startswith("_internal/") and name.count("/") == 1
                    and name not in expected):
                if native_name in native_names:
                    raise RuntimeError(f"Invalid bundled native file: {name}; rebuild the engine")
                native_names.add(native_name)
                if member.isdir():
                    continue
                if not member.isfile():
                    raise RuntimeError(f"Invalid bundled native file: {name}; rebuild the engine")
                with packaged.extractfile(member) as stream:
                    if native_name.endswith("runtime-manifest.json"):

                        if member.size > 64 * 1024:
                            raise RuntimeError(f"Invalid bundled native manifest: {name}")
                        try:
                            native[native_name] = json.load(stream)
                        except (ValueError, UnicodeError) as error:
                            raise RuntimeError(f"Invalid bundled native manifest: {name}") from error
                    else:
                        native[native_name] = {"size": member.size,
                                               "sha256": hashlib.file_digest(stream, "sha256").hexdigest()}
                continue
            if name not in expected:
                continue
            source = expected[name]

            if name in seen or not member.isfile() or member.size > len(source) * 2:
                raise RuntimeError(f"Invalid bundled source: {name}; rebuild the engine")
            seen.add(name)
            with packaged.extractfile(member) as stream:
                if stream.read().replace(b"\r\n", b"\n") != source:
                    raise RuntimeError(f"Stale bundled source: {name}; rebuild the engine")
    missing = expected.keys() - seen
    if missing:
        raise RuntimeError(f"Missing bundled sources: {', '.join(sorted(missing))}; rebuild the engine")
    missing_native = native_paths - native.keys()
    if missing_native:
        raise RuntimeError(f"Missing bundled native files: {', '.join(sorted(missing_native))}; rebuild the engine")
    provenance = {"build_id": R2T2_BUILD_ID, "revision": R2T2_REVISION,
                  "patch_sha256": hashlib.sha256(R2T2_PATCH.read_bytes()).hexdigest(),
                  "reuse_source_sha256": reuse_source_fingerprint()}
    for backend in ("cpu", "cuda"):
        prefix = f"_internal/r2t2-native/{backend}"
        manifest = native[f"{prefix}/runtime-manifest.json"]
        if (not isinstance(manifest, dict)
                or manifest.get("backend") != backend
                or any(manifest.get(key) != value for key, value in provenance.items())):
            raise RuntimeError(f"Stale bundled R2T2 {backend} runtime; rebuild the engine")
        files = manifest.get("files")
        if not isinstance(files, dict) or not {"audiocpp.dll", "reuse.dll"} <= files.keys():
            raise RuntimeError(f"Invalid bundled R2T2 {backend} file table; rebuild the engine")
        for directory, entries in ((prefix, files), ("_internal", manifest.get("shared_files", {}))):
            if not isinstance(entries, dict):
                raise RuntimeError(f"Invalid bundled R2T2 {backend} file table; rebuild the engine")
            for filename, metadata in entries.items():
                if (not filename or filename in {".", ".."}
                        or any(character in filename for character in "/\\:\0")
                        or not isinstance(metadata, dict)
                        or type(metadata.get("size")) is not int or metadata["size"] < 0
                        or not isinstance(metadata.get("sha256"), str)
                        or not re.fullmatch(r"[0-9a-f]{64}", metadata["sha256"])):
                    raise RuntimeError(f"Invalid bundled native metadata: {filename!r}; rebuild the engine")
                name = f"{directory}/{filename}"
                actual = native.get(name.casefold())
                if actual is None or any(actual.get(key) != metadata[key] for key in ("size", "sha256")):
                    raise RuntimeError(f"Bundled native dependency missing or checksum/size mismatch: {name}; rebuild the engine")


def main():
    if not ENTRY_SCRIPT.exists():
        print(f"错误: 入口脚本不存在: {ENTRY_SCRIPT}", file=sys.stderr)
        sys.exit(1)

    native_runtime = validate_r2t2_runtime()


    if DIST_DIR.exists():
        print(f"清理旧构建: {DIST_DIR}")
        remove_tree(DIST_DIR, warn_only=False)

    work_dir = PROJECT_ROOT / "build" / "pyinstaller"
    spec_dir = PROJECT_ROOT / "build"


    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onedir",
        "--name", "engine",
        "--distpath", str(DIST_DIR),
        "--workpath", str(work_dir),
        "--specpath", str(spec_dir),
        "--additional-hooks-dir", str(PROJECT_ROOT / "scripts" / "pyinstaller_hooks"),
    ]

    if WINDOWS_MANIFEST.exists():
        cmd.extend(["--manifest", str(WINDOWS_MANIFEST)])

    for backend in ("cpu", "cuda"):
        cmd.extend(["--add-data", f"{native_runtime / backend}{os.pathsep}r2t2-native/{backend}"])

    for filename in ADD_DATA_FILES:
        src = RESOURCES_DIR / filename
        if not src.exists():
            print(f"警告: 数据文件不存在，跳过: {src}", file=sys.stderr)
            continue
        cmd.extend(["--add-data", f"{src}{os.pathsep}."])

    for mod in HIDDEN_IMPORTS:
        cmd.extend(["--hidden-import", mod])

    for pkg in COLLECT_ALL:
        cmd.extend(["--collect-all", pkg])

    for mod in EXCLUDE_MODULES:
        cmd.extend(["--exclude-module", mod])

    cmd.append(str(ENTRY_SCRIPT))

    print("=" * 60)
    print("步骤 1/3: PyInstaller 构建")
    print(f"入口: {ENTRY_SCRIPT}")
    print("=" * 60)

    result = subprocess.run(cmd)
    if result.returncode != 0:
        print("PyInstaller 构建失败！", file=sys.stderr)
        sys.exit(result.returncode)

    engine_dir = DIST_DIR / "engine"
    if not engine_dir.exists():
        print("错误: 构建输出目录不存在", file=sys.stderr)
        sys.exit(1)

    raw_size = get_size_mb(engine_dir)
    print(f"\nPyInstaller 输出: {raw_size:.0f} MB")


    print("=" * 60)
    print("步骤 2/3: 瘦身（删除开发期库文件）")
    print("=" * 60)

    saved = 0.0
    saved += strip_dev_artifacts(engine_dir)
    saved += share_native_runtime_dlls(engine_dir)
    stripped_size = get_size_mb(engine_dir)
    print(f"节省: {saved:.0f} MB, 瘦身后: {stripped_size:.0f} MB")


    print("=" * 60)
    print("步骤 3/3: 压缩为 engine.tar.xz")
    print("=" * 60)

    staging_archive = OUTPUT_ARCHIVE.with_name(
        f".{OUTPUT_ARCHIVE.name}.{os.getpid()}.{time.time_ns()}.tmp"
    )
    try:
        archive_size = create_tar_xz(engine_dir, staging_archive)
        if not staging_archive.is_file() or staging_archive.stat().st_size == 0:
            raise RuntimeError("引擎归档构建结果为空")
        os.replace(staging_archive, OUTPUT_ARCHIVE)
    finally:
        remove_file(staging_archive, warn_only=True)


    remove_tree(DIST_DIR, warn_only=True)

    print("=" * 60)
    print("构建完成！")
    print(f"  原始大小:   {raw_size:.0f} MB")
    print(f"  瘦身后:     {stripped_size:.0f} MB")
    print(f"  压缩包:     {archive_size:.0f} MB → {OUTPUT_ARCHIVE.name}")
    print(f"  输出路径:   {OUTPUT_ARCHIVE}")
    print("=" * 60)

    if archive_size > 1800:
        print(f"警告: 压缩包 {archive_size:.0f} MB 接近 NSIS 2GB 限制！", file=sys.stderr)


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verify-archive", type=Path)
    args = parser.parse_args()
    if args.verify_archive is not None:
        try:
            verify_archive_sources(args.verify_archive)
        except (OSError, RuntimeError, tarfile.TarError, EOFError) as error:
            parser.exit(1, f"{error}\nRun: uv run --locked python scripts/build_engine.py\n")
    else:
        main()
