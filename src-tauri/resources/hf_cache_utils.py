"""HuggingFace 缓存检测共享工具模块

供 funasr_server.py 和 download_models.py 共同使用，
避免重复实现缓存路径和模型就绪检查逻辑。
"""

import os

ASR_REPO_ID = "FunAudioLLM/SenseVoiceSmall"
VAD_REPO_ID = "funasr/fsmn-vad"
MODEL_REPOS = [ASR_REPO_ID, VAD_REPO_ID]

WHISPER_REPO_ID = "deepdml/faster-whisper-large-v3-turbo-ct2"
WHISPER_MODEL_REPOS = [WHISPER_REPO_ID]

_WEIGHT_EXTS = (".pt", ".bin", ".safetensors", ".onnx")
_MIN_WEIGHT_SIZE = 1_000_000  # 与 Rust 端阈值一致


def get_hf_cache_root():
    """返回 HuggingFace 默认缓存根目录"""
    hf_home = os.environ.get("HF_HOME")
    if hf_home:
        return os.path.join(hf_home, "hub")
    xdg_cache = os.environ.get("XDG_CACHE_HOME")
    if xdg_cache:
        return os.path.join(xdg_cache, "huggingface", "hub")
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
        for root, _dirs, files in os.walk(snapshot_path):
            for f in files:
                if f.endswith(_WEIGHT_EXTS):
                    filepath = os.path.join(root, f)
                    try:
                        if os.path.getsize(filepath) >= _MIN_WEIGHT_SIZE:
                            return True
                    except OSError:
                        continue

    return False
