#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
FunASR SenseVoiceSmall 模型下载脚本
从 HuggingFace 顺序下载 SenseVoiceSmall + VAD 模型文件
"""

import sys
import json
import threading
import os
from pathlib import Path

# 防止 snapshot_download 在 Windows 上无限期挂起
os.environ.setdefault("HF_HUB_ETAG_TIMEOUT", "30")
os.environ.setdefault("HF_HUB_DOWNLOAD_TIMEOUT", "300")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")

from huggingface_hub import snapshot_download
from tqdm import tqdm


# ---------------------------------------------------------------------------
# 进度输出
# ---------------------------------------------------------------------------

_progress_lock = threading.Lock()
_progress = {}
_completed_count = 0
_total_count = 0


def _emit(model_type, stage, percent, error=None, message=None):
    global _completed_count

    with _progress_lock:
        if stage == "downloading":
            _progress[model_type] = percent
        elif stage == "completed":
            _progress[model_type] = 100
            _completed_count += 1
        elif stage == "error":
            _progress[model_type] = 0
            _completed_count += 1

        overall = sum(_progress.values()) / _total_count if _total_count else 0
        completed_snapshot = _completed_count

    status = {
        "stage": stage,
        "model": model_type,
        "progress": percent,
        "overall_progress": round(overall, 1),
        "completed": completed_snapshot,
        "total": _total_count,
    }
    if error:
        status["error"] = error
    if message:
        status["message"] = message

    print(json.dumps(status, ensure_ascii=False))
    sys.stdout.flush()


class _ProgressTqdm(tqdm):
    """自定义 tqdm 子类，将真实下载进度转发到 JSON 输出。"""

    _model_type = "asr"
    _model_name = ""
    _last_pct = -1

    def display(self, msg=None, pos=None):
        self._report_progress()

    def update(self, n=1):
        super().update(n)
        self._report_progress()

    def _report_progress(self):
        if self.total and self.total > 0:
            pct = int(self.n * 100 / self.total)
        else:
            pct = 0
        # 避免过于频繁地输出（相同百分比不重复发）
        if pct != _ProgressTqdm._last_pct:
            _ProgressTqdm._last_pct = pct
            _emit(
                _ProgressTqdm._model_type,
                "downloading",
                pct,
                message=f"{_ProgressTqdm._model_name} 下载中... {pct}%",
            )


# ---------------------------------------------------------------------------
# 缓存检测
# ---------------------------------------------------------------------------

def _get_hf_cache_root():
    """获取 HuggingFace 缓存根目录"""
    hf_home = os.environ.get("HF_HOME", str(Path.home() / ".cache" / "huggingface"))
    return os.path.join(hf_home, "hub")


def _is_repo_cached(repo_id):
    """检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。

    仅检查目录结构不够——下载中途取消会留下空壳目录，
    这里额外验证 snapshots 中存在 >1MB 的模型权重文件。
    """
    cache_root = _get_hf_cache_root()
    dir_name = f"models--{repo_id.replace('/', '--')}"
    repo_dir = os.path.join(cache_root, dir_name)
    if not os.path.isdir(repo_dir):
        return False

    weight_exts = (".pt", ".bin", ".safetensors", ".onnx")
    min_size = 1 * 1024 * 1024  # 1MB

    snapshots_dir = os.path.join(repo_dir, "snapshots")
    if not os.path.isdir(snapshots_dir):
        return False

    for snapshot_name in os.listdir(snapshots_dir):
        snapshot_path = os.path.join(snapshots_dir, snapshot_name)
        if not os.path.isdir(snapshot_path):
            continue
        for root, _dirs, files in os.walk(snapshot_path):
            for f in files:
                if f.endswith(weight_exts):
                    filepath = os.path.join(root, f)
                    try:
                        if os.path.getsize(filepath) >= min_size:
                            return True
                    except OSError:
                        continue

    return False


# ---------------------------------------------------------------------------
# 下载逻辑
# ---------------------------------------------------------------------------

def download_model(model_config):
    """下载模型，已缓存则直接跳过"""
    model_name = model_config["name"]
    model_type = model_config["type"]
    revision = model_config.get("revision")
    fallback_name = model_config.get("fallback_name")
    fallback_revision = model_config.get("fallback_revision", revision)

    # 已缓存的模型直接跳过，不调用 snapshot_download（避免 Windows 上缓慢的完整性校验）
    if _is_repo_cached(model_name):
        _emit(model_type, "completed", 100,
              message=f"{model_name} 已缓存，跳过下载")
        return {"success": True, "model": model_type}

    _emit(model_type, "downloading", 0, message=f"准备下载 {model_name}")

    # 设置真实进度回调的上下文
    _ProgressTqdm._model_type = model_type
    _ProgressTqdm._model_name = model_name
    _ProgressTqdm._last_pct = -1

    result_box = {}
    done_event = threading.Event()

    def _snapshot(repo_id, repo_revision):
        if repo_revision:
            snapshot_download(repo_id=repo_id, revision=repo_revision,
                              tqdm_class=_ProgressTqdm)
        else:
            snapshot_download(repo_id=repo_id, tqdm_class=_ProgressTqdm)

    def _try_download(repo_id, repo_revision):
        try:
            _snapshot(repo_id, repo_revision)
            return True, repo_id, None
        except Exception as e:
            if repo_revision:
                try:
                    _snapshot(repo_id, None)
                    return True, repo_id, None
                except Exception as e2:
                    return False, repo_id, str(e2)
            return False, repo_id, str(e)

    def _worker():
        try:
            ok, name, err = _try_download(model_name, revision)
            if ok:
                result_box["ok"] = True
                result_box["name"] = name
            elif fallback_name:
                ok, name, err = _try_download(fallback_name, fallback_revision)
                if ok:
                    result_box["ok"] = True
                    result_box["name"] = name
                else:
                    result_box["ok"] = False
                    result_box["error"] = err
            else:
                result_box["ok"] = False
                result_box["error"] = err
        finally:
            done_event.set()

    t = threading.Thread(target=_worker, daemon=True)
    t.start()

    # 每 2 秒发一次心跳保活（最长等待 30 分钟）
    # 真实进度由 _ProgressTqdm 回调输出，心跳仅防止前端判定超时
    tick = 0
    max_ticks = 900  # 900 * 2s = 1800s = 30 min
    while not done_event.wait(timeout=2.0):
        tick += 1
        if tick > max_ticks:
            _emit(model_type, "error", 0, "下载超时（超过30分钟）",
                  message=f"{model_name} 下载超时")
            return {"success": False, "model": model_type, "error": "下载超时（超过30分钟）"}
        # 心跳：用当前已记录的进度值重新发送，保持 UI 活跃
        with _progress_lock:
            current_pct = _progress.get(model_type, 0)
        if current_pct == 0 and tick <= 15:
            # 前 30 秒无进度时提示正在连接
            _emit(model_type, "downloading", 0,
                  message=f"正在连接 HuggingFace 下载 {model_name}...")
        else:
            _emit(model_type, "downloading", current_pct,
                  message=f"{model_name} 下载中...")

    if result_box.get("ok"):
        finished_name = result_box.get("name", model_name)
        _emit(model_type, "completed", 100,
              message=f"{finished_name} 下载完成")
        return {"success": True, "model": model_type}
    else:
        err = result_box.get("error", "unknown error")
        _emit(model_type, "error", 0, err,
              message=f"{model_name} 下载失败")
        return {"success": False, "model": model_type, "error": err}


def main():
    global _total_count

    # 模型配置（HuggingFace repo ID）
    models = [
        {"name": "FunAudioLLM/SenseVoiceSmall", "type": "asr"},
        {"name": "funasr/fsmn-vad", "type": "vad"},
    ]

    _total_count = len(models)
    for m in models:
        _progress[m["type"]] = 0

    results = {}
    for model_config in models:
        result = download_model(model_config)
        results[model_config["type"]] = result
        if not result.get("success"):
            break

    failed = [mt for mt, r in results.items() if not r["success"]]

    if failed:
        final_result = {
            "success": False,
            "error": f"以下模型下载失败: {', '.join(failed)}",
            "failed_models": failed,
            "results": results,
        }
    else:
        final_result = {
            "success": True,
            "message": "所有模型下载完成",
            "results": results,
        }

    print(json.dumps(final_result, ensure_ascii=False))
    sys.stdout.flush()


if __name__ == "__main__":
    try:
        main()
    except Exception as e:
        print(json.dumps({"success": False, "error": str(e)}, ensure_ascii=False))
        sys.exit(1)
