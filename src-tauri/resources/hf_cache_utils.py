"""HuggingFace 缓存检测共享工具模块

供 funasr_server.py 和 download_models.py 共同使用，
避免重复实现缓存路径和模型就绪检查逻辑。
"""

import os
import json
import hashlib

ASR_REPO_ID = "FunAudioLLM/SenseVoiceSmall"
VAD_REPO_ID = "funasr/fsmn-vad"
MODEL_REPOS = [ASR_REPO_ID, VAD_REPO_ID]

WHISPER_REPO_ID = "deepdml/faster-whisper-large-v3-turbo-ct2"
WHISPER_MODEL_REPOS = [WHISPER_REPO_ID]

_WEIGHT_EXTS = (".pt", ".bin", ".safetensors", ".onnx")
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


def cleanup_incomplete_files(repo_id):
    """删除某个 repo 缓存目录下残留的 .incomplete 文件，返回删除数量。"""
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    if not os.path.isdir(repo_dir):
        return 0

    removed = 0
    for root, _dirs, files in os.walk(repo_dir):
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
