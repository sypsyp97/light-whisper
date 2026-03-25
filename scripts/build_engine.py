#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
自动化 PyInstaller 构建脚本。

流程：
1. PyInstaller --onedir 构建 engine.exe
2. 删除可安全裁剪的可选 CUDA DLL 和开发期库文件
3. 压缩为 engine.tar.xz（适配 NSIS 2GB 限制）
4. 输出到 src-tauri/resources/engine.tar.xz

必须在项目 .venv 环境中运行。
"""

import os
import shutil
import subprocess
import sys
import tarfile
import time
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
RESOURCES_DIR = PROJECT_ROOT / "src-tauri" / "resources"
DIST_DIR = RESOURCES_DIR / "python-dist"
ENTRY_SCRIPT = RESOURCES_DIR / "engine.py"
OUTPUT_ARCHIVE = RESOURCES_DIR / "engine.tar.xz"
WINDOWS_MANIFEST = PROJECT_ROOT / "src-tauri" / "windows-app-manifest.xml"

# 同级 Python 脚本，打包到 _internal/
ADD_DATA_FILES = [
    "funasr_server.py",
    "whisper_server.py",
    "download_models.py",
    "server_common.py",
    "hf_cache_utils.py",
]

HIDDEN_IMPORTS = [
    "funasr",
    "funasr.utils.postprocess_utils",
    "faster_whisper",
    "ctranslate2",
    "requests",
    "certifi",
    "torch",
    "torchaudio",
    "transformers",
    "librosa",
    "numpy",
    "scipy",
    "huggingface_hub",
    "huggingface_hub.utils",
    "tqdm",
    "soundfile",
    "sklearn.utils._cython_blas",
]

# 需要完整收集的包（子模块 + 数据文件）
# funasr 用 pkgutil.walk_packages 动态注册所有模型类，必须收集全部子模块
COLLECT_ALL = [
    "funasr",
    "faster_whisper",
]

EXCLUDE_MODULES = [
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
]

# 可安全裁剪的可选 CUDA DLL（glob 模式，匹配 torch/lib/ 下的文件）
# 注意：torch_cuda.dll 在 Windows 上会直接依赖 cusolver/cusparse/cufft；
# 这些 DLL 即使在 CPU 回退场景下也必须保留，否则 import torch 会在无 NVIDIA 机器上崩溃。
STRIP_CUDA_PATTERNS = [
    "cudnn_engines_precompiled*.dll",   # ~562M cuDNN 预编译融合引擎
    "curand*.dll",                       # ~61M  随机数生成（仅训练）
]

# Windows 运行时不需要 .lib / .pdb；这些仅用于链接或调试，
# 但在 PyTorch wheel 中体积很大（例如 dnnl.lib ~675 MB）。
STRIP_DEV_ARTIFACT_PATTERNS = [
    "*.lib",
    "*.pdb",
]


def find_7z_executable() -> str | None:
    """返回可用的 7z 可执行文件路径。"""
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
    """删除目录；在 Windows 文件句柄尚未释放时做有限重试。"""
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


def get_size_mb(path: Path) -> float:
    total = sum(f.stat().st_size for f in path.rglob("*") if f.is_file())
    return total / (1024 * 1024)


def strip_cuda_dlls(engine_dir: Path) -> float:
    """删除可安全裁剪的 CUDA DLL，返回节省的 MB 数"""
    torch_lib = engine_dir / "_internal" / "torch" / "lib"
    if not torch_lib.is_dir():
        return 0.0

    saved = 0.0
    for pattern in STRIP_CUDA_PATTERNS:
        for match in torch_lib.glob(pattern):
            size = match.stat().st_size / (1024 * 1024)
            print(f"  删除: {match.name} ({size:.0f} MB)")
            match.unlink()
            saved += size

    return saved


def strip_dev_artifacts(engine_dir: Path) -> float:
    """删除运行时不需要的链接/调试产物，返回节省的 MB 数。"""
    internal_dir = engine_dir / "_internal"
    if not internal_dir.is_dir():
        return 0.0

    saved = 0.0
    for pattern in STRIP_DEV_ARTIFACT_PATTERNS:
        for match in internal_dir.rglob(pattern):
            if not match.is_file():
                continue
            size = match.stat().st_size / (1024 * 1024)
            print(f"  删除: {match.relative_to(engine_dir)} ({size:.0f} MB)")
            match.unlink()
            saved += size

    return saved


def validate_torch_cuda_deps(engine_dir: Path) -> None:
    """校验 torch_cuda.dll 的直接 CUDA 依赖仍然存在。"""
    try:
        import pefile
    except ImportError:
        print("警告: 未安装 pefile，跳过 torch CUDA 依赖校验", file=sys.stderr)
        return

    torch_lib = engine_dir / "_internal" / "torch" / "lib"
    torch_cuda = torch_lib / "torch_cuda.dll"
    if not torch_cuda.is_file():
        return

    pe = pefile.PE(str(torch_cuda), fast_load=True)
    pe.parse_data_directories(
        directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]]
    )

    required = []
    for entry in getattr(pe, "DIRECTORY_ENTRY_IMPORT", []):
        dll_name = entry.dll.decode("utf-8", "ignore")
        lowered = dll_name.lower()
        if lowered.startswith(("cu", "nv", "cudnn")):
            required.append(dll_name)

    missing = [name for name in required if not (torch_lib / name).is_file()]
    if missing:
        raise RuntimeError(
            "torch_cuda.dll 依赖缺失，当前裁剪配置会导致引擎启动失败: "
            + ", ".join(missing)
        )


def create_tar_xz_with_python(engine_dir: Path, output: Path) -> float:
    """使用 Python 标准库压缩为 tar.xz，返回压缩包大小 MB。"""
    print(f"正在压缩到 {output.name} ...")
    files = [f for f in engine_dir.rglob("*") if f.is_file()]
    total = len(files)

    with tarfile.open(output, mode="w:xz", preset=9) as tf:
        for i, filepath in enumerate(files, 1):
            arcname = filepath.relative_to(engine_dir)
            tf.add(filepath, arcname=str(arcname), recursive=False)
            if i % 500 == 0 or i == total:
                print(f"  压缩进度: {i}/{total} ({i * 100 // total}%)")

    return output.stat().st_size / (1024 * 1024)


def create_tar_xz_with_7z(engine_dir: Path, output: Path, seven_zip: str) -> float:
    """使用 7-Zip 构建 tar.xz，返回压缩包大小 MB。

    分两步：先打 tar，再用 7z 多线程压缩为 xz，避免 Windows 管道写入问题。
    """
    output = output.resolve()
    tar_path = output.with_suffix("")  # engine.tar
    files = [f for f in engine_dir.rglob("*") if f.is_file()]
    total = len(files)
    if not files:
        raise RuntimeError("engine 目录为空，无法创建归档")

    print(f"正在用 7-Zip 构建 {output.name} ...")

    # 阶段 1: 打 tar
    print(f"  阶段 1/2: 打包 tar ({total} 文件)")
    with tarfile.open(tar_path, mode="w") as tf:
        for i, filepath in enumerate(files, 1):
            arcname = filepath.relative_to(engine_dir)
            tf.add(filepath, arcname=str(arcname), recursive=False)
            if i % 500 == 0 or i == total:
                print(f"  打包进度: {i}/{total} ({i * 100 // total}%)")

    # 阶段 2: 用 7z 压缩为 xz
    tar_mb = tar_path.stat().st_size / (1024 * 1024)
    print(f"  阶段 2/2: 7-Zip 多线程压缩 xz ({tar_mb:.0f} MB)")
    cmd = [seven_zip, "a", "-txz", "-mx=9", "-mmt=on", str(output), str(tar_path)]
    try:
        subprocess.run(cmd, check=True, cwd=output.parent)
    finally:
        tar_path.unlink(missing_ok=True)

    return output.stat().st_size / (1024 * 1024)


def create_tar_xz(engine_dir: Path, output: Path) -> float:
    """将 engine 目录压缩为 tar.xz，返回压缩包大小 MB。"""
    seven_zip = find_7z_executable()
    if seven_zip:
        print(f"检测到 7-Zip: {seven_zip}")
        return create_tar_xz_with_7z(engine_dir, output, seven_zip)

    print("警告: 未找到 7-Zip，回退到 Python 单线程 tar.xz 压缩", file=sys.stderr)
    return create_tar_xz_with_python(engine_dir, output)


def main():
    if not ENTRY_SCRIPT.exists():
        print(f"错误: 入口脚本不存在: {ENTRY_SCRIPT}", file=sys.stderr)
        sys.exit(1)

    # 清理旧构建
    if DIST_DIR.exists():
        print(f"清理旧构建: {DIST_DIR}")
        remove_tree(DIST_DIR, warn_only=False)
    if OUTPUT_ARCHIVE.exists():
        OUTPUT_ARCHIVE.unlink()

    work_dir = PROJECT_ROOT / "build" / "pyinstaller"
    spec_dir = PROJECT_ROOT / "build"

    # 构建 PyInstaller 命令
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onedir",
        "--name", "engine",
        "--distpath", str(DIST_DIR),
        "--workpath", str(work_dir),
        "--specpath", str(spec_dir),
    ]

    if WINDOWS_MANIFEST.exists():
        cmd.extend(["--manifest", str(WINDOWS_MANIFEST)])

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

    # 瘦身：删除可安全裁剪的 CUDA DLL
    print("=" * 60)
    print("步骤 2/3: 瘦身（删除可安全裁剪的 CUDA DLL 和开发期库文件）")
    print("=" * 60)

    saved = 0.0
    saved += strip_cuda_dlls(engine_dir)
    saved += strip_dev_artifacts(engine_dir)
    validate_torch_cuda_deps(engine_dir)
    stripped_size = get_size_mb(engine_dir)
    print(f"节省: {saved:.0f} MB, 瘦身后: {stripped_size:.0f} MB")

    # 压缩
    print("=" * 60)
    print("步骤 3/3: 压缩为 engine.tar.xz")
    print("=" * 60)

    archive_size = create_tar_xz(engine_dir, OUTPUT_ARCHIVE)

    # 清理未压缩目录
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
    main()
