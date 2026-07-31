"""HuggingFace 缓存检测共享工具模块

供各本地 ASR 服务与 download_models.py 共同使用，
避免重复实现缓存路径和模型就绪检查逻辑。
"""

import os
import json
import hashlib

QWEN3_ASR_MODELS = {
    "qwen3-asr-0.6b": {
        "repo_id": "handy-computer/Qwen3-ASR-0.6B-gguf",
        "filename": "Qwen3-ASR-0.6B-Q8_0.gguf",
        "revision": "e4e16599b900eb0cb36e524514756bb92eb092b7",
        "size": 850_423_456,
        "sha256": "f081b2d5e23bd669d92cc331d722a8a0681943b8e6f34b48996fd5c319b5acd8",
    },
    "qwen3-asr-1.7b": {
        "repo_id": "handy-computer/Qwen3-ASR-1.7B-gguf",
        "filename": "Qwen3-ASR-1.7B-Q8_0.gguf",
        "revision": "92282af1610a2db19d66f2bef1e260f5deca782d",
        "size": 2_185_030_624,
        "sha256": "9a0d81792dfea2d5f278b8a63deb3ea6e02139ce42c2301f32ea19c4f77526b7",
    },
}

_WEIGHT_EXTS = (".pt", ".bin", ".safetensors", ".onnx", ".gguf")
_MIN_WEIGHT_SIZE = 1_000_000  # 与 Rust 端阈值一致
COMPLETE_MANIFEST_NAME = ".light_whisper_complete.json"


def get_hf_cache_root():
    """返回 HuggingFace 缓存根目录

    优先级：HF_HUB_CACHE（由 Rust 设置的自定义路径）> HF_HOME/hub > 默认
    """
    hf_hub_cache = os.environ.get("HF_HUB_CACHE")
    if hf_hub_cache:
        return hf_hub_cache
    hf_home = os.environ.get("HF_HOME")
    if hf_home:
        return os.path.join(hf_home, "hub")
    return os.path.join(os.path.expanduser("~"), ".cache", "huggingface", "hub")


def is_hf_repo_ready(repo_id):
    """检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。

    仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshots 存在但无权重文件），
    导致后续加载卡死。这里额外验证 snapshots 中存在 >1MB 的模型权重文件。
    """
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    if not os.path.isdir(repo_dir):
        return False

    snapshots_dir = os.path.join(repo_dir, "snapshots")
    if not os.path.isdir(snapshots_dir):
        return False

    for snapshot_name in os.listdir(snapshots_dir):
        snapshot_path = os.path.join(snapshots_dir, snapshot_name)
        if not os.path.isdir(snapshot_path):
            continue
        if _snapshot_matches_completion_manifest(snapshot_path) or _snapshot_matches_legacy_weight_check(snapshot_path):
            return True

    return False


def find_hf_snapshot_file(repo_id, filename):
    """Resolve one exact cached file without scanning unrelated quantizations."""
    cache_root = get_hf_cache_root()
    repo_dir = os.path.join(cache_root, "models--" + repo_id.replace("/", "--"))
    snapshots_dir = os.path.join(repo_dir, "snapshots")
    if not os.path.isdir(snapshots_dir):
        return None

    snapshot_names = []
    try:
        with open(os.path.join(repo_dir, "refs", "main"), "r", encoding="utf-8") as f:
            snapshot_names.append(f.read().strip())
    except OSError:
        pass
    snapshot_names.extend(
        name for name in os.listdir(snapshots_dir) if name not in snapshot_names
    )

    normalized = filename.replace("/", os.sep)
    for snapshot_name in snapshot_names:
        snapshot_path = os.path.join(snapshots_dir, snapshot_name)
        candidate = os.path.join(snapshot_path, normalized)
        try:
            actual_size = os.path.getsize(candidate)
        except OSError:
            continue
        if actual_size < _MIN_WEIGHT_SIZE:
            continue

        manifest_path = os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)
        try:
            with open(manifest_path, "r", encoding="utf-8") as f:
                manifest = json.load(f)
            manifest_item = next(
                (item for item in manifest.get("files", []) if item.get("path") == filename),
                None,
            )
            if manifest_item is None or manifest_item.get("size") != actual_size:
                continue
        except (OSError, json.JSONDecodeError):
            # Legacy Hugging Face caches predate the completion manifest.
            pass
        return candidate

    return None


def cleanup_incomplete_files(repo_id):
    """删除旧 huggingface_hub blob 临时文件，保留 snapshots 下可续传的 partial。"""
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    blobs_dir = os.path.join(repo_dir, "blobs")
    if not os.path.isdir(blobs_dir):
        return 0

    removed = 0
    for root, _dirs, files in os.walk(blobs_dir):
        for filename in files:
            if not filename.endswith(".incomplete"):
                continue
            path = os.path.join(root, filename)
            try:
                os.remove(path)
                removed += 1
            except OSError:
                pass
    return removed


def _snapshot_matches_completion_manifest(snapshot_path):
    manifest_path = os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)
    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except (OSError, json.JSONDecodeError):
        return False

    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        return False

    has_weight = False
    for item in files:
        if not isinstance(item, dict):
            return False
        rel_path = item.get("path")
        expected_size = item.get("size")
        expected_sha256 = item.get("sha256")
        if not isinstance(rel_path, str) or not isinstance(expected_size, int):
            return False
        if expected_sha256 is not None and not isinstance(expected_sha256, str):
            return False
        if os.path.isabs(rel_path) or ".." in rel_path.replace("\\", "/").split("/"):
            return False
        path = os.path.join(snapshot_path, rel_path.replace("/", os.sep))
        try:
            actual_size = os.path.getsize(path)
        except OSError:
            return False
        if actual_size != expected_size:
            return False
        if expected_sha256 and _sha256_file(path).lower() != expected_sha256.lower():
            return False
        if rel_path.endswith(_WEIGHT_EXTS) and actual_size >= _MIN_WEIGHT_SIZE:
            has_weight = True

    return has_weight


def _sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _snapshot_matches_legacy_weight_check(snapshot_path):
    has_weight = False
    for root, _dirs, files in os.walk(snapshot_path):
        for f in files:
            if f.endswith(".incomplete"):
                return False
            if f.endswith(_WEIGHT_EXTS):
                filepath = os.path.join(root, f)
                try:
                    if os.path.getsize(filepath) >= _MIN_WEIGHT_SIZE:
                        has_weight = True
                except OSError:
                    return False
    return has_weight
