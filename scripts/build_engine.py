#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
自动化 PyInstaller 构建脚本。

流程：
1. PyInstaller --onedir 构建 engine.exe
2. 删除可安全裁剪的可选 CUDA DLL
3. 压缩为 engine.zip（适配 NSIS 2GB 限制）
4. 输出到 src-tauri/resources/engine.zip

必须在项目 .venv 环境中运行。
"""

import os
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
RESOURCES_DIR = PROJECT_ROOT / "src-tauri" / "resources"
DIST_DIR = RESOURCES_DIR / "python-dist"
ENTRY_SCRIPT = RESOURCES_DIR / "engine.py"
OUTPUT_ZIP = RESOURCES_DIR / "engine.zip"

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


def create_zip(engine_dir: Path, output: Path) -> float:
    """将 engine 目录压缩为 zip，返回 zip 大小 MB"""
    print(f"正在压缩到 {output.name} ...")
    files = [f for f in engine_dir.rglob("*") if f.is_file()]
    total = len(files)

    with zipfile.ZipFile(output, "w", zipfile.ZIP_LZMA, allowZip64=True) as zf:
        for i, filepath in enumerate(files, 1):
            arcname = filepath.relative_to(engine_dir)
            zf.write(filepath, arcname)
            if i % 500 == 0 or i == total:
                print(f"  压缩进度: {i}/{total} ({i * 100 // total}%)")

    return output.stat().st_size / (1024 * 1024)


def main():
    if not ENTRY_SCRIPT.exists():
        print(f"错误: 入口脚本不存在: {ENTRY_SCRIPT}", file=sys.stderr)
        sys.exit(1)

    # 清理旧构建
    if DIST_DIR.exists():
        print(f"清理旧构建: {DIST_DIR}")
        shutil.rmtree(DIST_DIR)
    if OUTPUT_ZIP.exists():
        OUTPUT_ZIP.unlink()

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
    print("步骤 2/3: 瘦身（删除可安全裁剪的 CUDA DLL）")
    print("=" * 60)

    saved = strip_cuda_dlls(engine_dir)
    validate_torch_cuda_deps(engine_dir)
    stripped_size = get_size_mb(engine_dir)
    print(f"节省: {saved:.0f} MB, 瘦身后: {stripped_size:.0f} MB")

    # 压缩
    print("=" * 60)
    print("步骤 3/3: 压缩为 engine.zip")
    print("=" * 60)

    zip_size = create_zip(engine_dir, OUTPUT_ZIP)

    # 清理未压缩目录
    shutil.rmtree(DIST_DIR)

    print("=" * 60)
    print("构建完成！")
    print(f"  原始大小:   {raw_size:.0f} MB")
    print(f"  瘦身后:     {stripped_size:.0f} MB")
    print(f"  压缩包:     {zip_size:.0f} MB → {OUTPUT_ZIP.name}")
    print(f"  输出路径:   {OUTPUT_ZIP}")
    print("=" * 60)

    if zip_size > 1800:
        print(f"警告: 压缩包 {zip_size:.0f} MB 接近 NSIS 2GB 限制！", file=sys.stderr)


if __name__ == "__main__":
    main()
