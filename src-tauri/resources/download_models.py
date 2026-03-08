#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ASR 模型下载脚本
使用 requests 直接从 HuggingFace 下载，绕过 huggingface_hub 库的 Windows 文件锁 bug。
"""

import sys
import json
import os
import requests

from hf_cache_utils import is_hf_repo_ready, get_hf_cache_root, ASR_REPO_ID, VAD_REPO_ID, WHISPER_REPO_ID

HF_ENDPOINT = os.environ.get("HF_ENDPOINT", "https://huggingface.co")

_progress = {}
_completed_count = 0
_total_count = 0


def _emit(model_type, stage, percent, error=None, message=None):
    global _completed_count

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


def _get_repo_info(repo_id):
    """通过 HF API 获取仓库文件列表"""
    url = f"{HF_ENDPOINT}/api/models/{repo_id}"
    resp = requests.get(url, timeout=30)
    resp.raise_for_status()
    info = resp.json()
    commit_hash = info.get("sha", "main")
    siblings = info.get("siblings", [])
    files = [s["rfilename"] for s in siblings]
    return commit_hash, files


def _download_file(repo_id, filename, dest_path, model_type, file_idx, file_total):
    """下载单个文件，支持断点续传和进度上报"""
    url = f"{HF_ENDPOINT}/{repo_id}/resolve/main/{filename}"

    dest_dir = os.path.dirname(dest_path)
    os.makedirs(dest_dir, exist_ok=True)

    # 断点续传
    downloaded = 0
    if os.path.exists(dest_path):
        downloaded = os.path.getsize(dest_path)

    headers = {}
    if downloaded > 0:
        headers["Range"] = f"bytes={downloaded}-"

    resp = requests.get(url, headers=headers, stream=True, timeout=60, allow_redirects=True)

    if resp.status_code == 416:
        # 已经下完了
        return

    resp.raise_for_status()

    total_size = downloaded
    content_length = resp.headers.get("Content-Length")
    if content_length:
        total_size += int(content_length)

    mode = "ab" if downloaded > 0 else "wb"
    current = downloaded
    last_pct = -1

    with open(dest_path, mode) as f:
        for chunk in resp.iter_content(chunk_size=1024 * 1024):
            f.write(chunk)
            current += len(chunk)
            if total_size > 0:
                pct = int(current * 100 / total_size)
                if pct != last_pct:
                    last_pct = pct
                    _emit(model_type, "downloading", pct,
                          message=f"[{file_idx}/{file_total}] {filename} {pct}%")


def _cleanup_locks(repo_id):
    """清理残留的 .lock 和 .incomplete 文件"""
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")

    import glob
    blobs_dir = os.path.join(cache_root, dir_name, "blobs")
    if os.path.isdir(blobs_dir):
        for f in glob.glob(os.path.join(blobs_dir, "*.incomplete")):
            try:
                os.remove(f)
            except OSError:
                pass

    locks_dir = os.path.join(cache_root, ".locks", dir_name)
    if os.path.isdir(locks_dir):
        for f in glob.glob(os.path.join(locks_dir, "*.lock")):
            try:
                os.remove(f)
            except OSError:
                pass


def download_model(model_config):
    """下载模型到 HF 缓存结构"""
    model_name = model_config["name"]
    model_type = model_config["type"]

    _cleanup_locks(model_name)

    if is_hf_repo_ready(model_name):
        _emit(model_type, "completed", 100,
              message=f"{model_name} 已缓存，跳过下载")
        return {"success": True, "model": model_type}

    _emit(model_type, "downloading", 0, message=f"正在获取 {model_name} 文件列表...")

    try:
        commit_hash, files = _get_repo_info(model_name)
    except Exception as e:
        _emit(model_type, "error", 0, str(e),
              message=f"获取 {model_name} 文件列表失败")
        return {"success": False, "model": model_type, "error": str(e)}

    # 构建 HF 缓存目录结构
    cache_root = get_hf_cache_root()
    dir_name = "models--" + model_name.replace("/", "--")
    repo_dir = os.path.join(cache_root, dir_name)
    snapshot_dir = os.path.join(repo_dir, "snapshots", commit_hash)
    refs_dir = os.path.join(repo_dir, "refs")
    os.makedirs(snapshot_dir, exist_ok=True)
    os.makedirs(refs_dir, exist_ok=True)

    # 写入 refs/main
    with open(os.path.join(refs_dir, "main"), "w") as f:
        f.write(commit_hash)

    file_total = len(files)
    for idx, filename in enumerate(files, 1):
        dest_path = os.path.join(snapshot_dir, filename.replace("/", os.sep))

        # 跳过已存在且非空的文件
        if os.path.exists(dest_path) and os.path.getsize(dest_path) > 0:
            continue

        try:
            _download_file(model_name, filename, dest_path, model_type, idx, file_total)
        except Exception as e:
            _emit(model_type, "error", 0, str(e),
                  message=f"{filename} 下载失败: {e}")
            return {"success": False, "model": model_type, "error": str(e)}

    _emit(model_type, "completed", 100, message=f"{model_name} 下载完成")
    return {"success": True, "model": model_type}


def main():
    global _total_count

    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--engine", default="sensevoice", choices=["sensevoice", "whisper"])
    args = parser.parse_args()

    if args.engine == "whisper":
        models = [
            {"name": WHISPER_REPO_ID, "type": "asr"},
        ]
    else:
        models = [
            {"name": ASR_REPO_ID, "type": "asr"},
            {"name": VAD_REPO_ID, "type": "vad"},
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
