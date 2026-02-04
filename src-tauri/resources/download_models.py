#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
FunASR Paraformer 模型下载脚本
从 HuggingFace 顺序下载 Paraformer 相关模型文件
"""

import sys
import json
import threading
import os
from pathlib import Path

from huggingface_hub import snapshot_download


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

    status = {
        "stage": stage,
        "model": model_type,
        "progress": percent,
        "overall_progress": round(overall, 1),
        "completed": _completed_count,
        "total": _total_count,
    }
    if error:
        status["error"] = error
    if message:
        status["message"] = message

    print(json.dumps(status, ensure_ascii=False))
    sys.stdout.flush()


# ---------------------------------------------------------------------------
# 缓存检测
# ---------------------------------------------------------------------------

def _get_hf_cache_root():
    """获取 HuggingFace 缓存根目录"""
    hf_home = os.environ.get("HF_HOME", str(Path.home() / ".cache" / "huggingface"))
    return os.path.join(hf_home, "hub")


def _is_repo_cached(repo_id):
    """检查 HuggingFace 模型是否已缓存（refs 任意分支或 snapshots 存在即可）"""
    cache_root = _get_hf_cache_root()
    dir_name = f"models--{repo_id.replace('/', '--')}"
    repo_dir = os.path.join(cache_root, dir_name)
    if not os.path.isdir(repo_dir):
        return False

    refs_dir = os.path.join(repo_dir, "refs")
    if os.path.isdir(refs_dir):
        for name in os.listdir(refs_dir):
            if os.path.isfile(os.path.join(refs_dir, name)):
                return True

    snapshots_dir = os.path.join(repo_dir, "snapshots")
    if os.path.isdir(snapshots_dir):
        for name in os.listdir(snapshots_dir):
            if os.path.isdir(os.path.join(snapshots_dir, name)):
                return True

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

    result_box = {}
    done_event = threading.Event()

    def _snapshot(repo_id, repo_revision):
        if repo_revision:
            snapshot_download(repo_id=repo_id, revision=repo_revision)
        else:
            snapshot_download(repo_id=repo_id)

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

    # 每 2 秒发一次心跳，让 UI 知道还活着
    tick = 0
    while not done_event.wait(timeout=2.0):
        tick += 1
        fake_pct = min(95, tick * 5)
        _emit(model_type, "downloading", fake_pct,
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
        {"name": "funasr/paraformer-zh", "type": "asr", "revision": "v2.0.4"},
        {"name": "funasr/fsmn-vad", "type": "vad", "revision": "v2.0.4"},
        {
            "name": "funasr/ct-punc",
            "type": "punc",
            "revision": "v2.0.4",
        },
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
