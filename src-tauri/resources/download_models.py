#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
ASR 模型下载脚本
使用 requests 直接从 HuggingFace 下载，绕过 huggingface_hub 库的 Windows 文件锁 bug。
"""

import sys
import json
import os
import hashlib
import re
import requests

from hf_cache_utils import (
    is_hf_repo_ready,
    get_hf_cache_root,
    cleanup_incomplete_files,
    ASR_REPO_ID,
    VAD_REPO_ID,
    WHISPER_REPO_ID,
)

DEFAULT_HF_ENDPOINT = "https://huggingface.co"
DEFAULT_HF_FALLBACK_ENDPOINT = "https://hf-mirror.com"

HF_ENDPOINT = os.environ.get("HF_ENDPOINT", DEFAULT_HF_ENDPOINT).rstrip("/")
HF_FALLBACK_ENDPOINT = os.environ.get("HF_FALLBACK_ENDPOINT", DEFAULT_HF_FALLBACK_ENDPOINT).rstrip("/")
COMPLETE_MANIFEST_NAME = ".light_whisper_complete.json"

_progress = {}
_completed_count = 0
_total_count = 0

_CONTENT_RANGE_RE = re.compile(r"^bytes (\d+)-(\d+)/(\d+|\*)$")
_UNSATISFIED_RANGE_RE = re.compile(r"^bytes \*/(\d+)$")


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


def _candidate_endpoints():
    endpoints = [HF_ENDPOINT]
    # 用户显式设置 HF_ENDPOINT 时，尊重其选择；未设置时才自动切镜像。
    if "HF_ENDPOINT" not in os.environ and HF_FALLBACK_ENDPOINT and HF_FALLBACK_ENDPOINT not in endpoints:
        endpoints.append(HF_FALLBACK_ENDPOINT)
    return endpoints


def _get_repo_info(repo_id, endpoint):
    """通过 HF API 获取仓库文件列表"""
    url = f"{endpoint}/api/models/{repo_id}"
    resp = requests.get(url, timeout=30)
    resp.raise_for_status()
    info = resp.json()
    commit_hash = info.get("sha", "main")
    siblings = info.get("siblings", [])
    files = []
    for sibling in siblings:
        filename = sibling.get("rfilename")
        if not filename:
            continue
        item = {"rfilename": filename, "size": sibling.get("size")}
        sha256 = sibling.get("lfs", {}).get("sha256")
        if sha256:
            item["sha256"] = sha256
        files.append(item)
    return commit_hash, files


def _remote_file_size(url):
    try:
        resp = requests.head(url, timeout=30, allow_redirects=True)
        resp.raise_for_status()
        value = resp.headers.get("Content-Length")
        return int(value) if value else None
    except Exception:
        return None


def _resolve_download_url(endpoint, repo_id, filename, revision):
    return f"{endpoint}/{repo_id}/resolve/{revision}/{filename}"


def _parse_content_range(value):
    if not value:
        return None
    match = _CONTENT_RANGE_RE.fullmatch(value.strip())
    if not match:
        return None
    start, end, total = match.groups()
    return int(start), int(end), None if total == "*" else int(total)


def _parse_unsatisfied_range_total(value):
    if not value:
        return None
    match = _UNSATISFIED_RANGE_RE.fullmatch(value.strip())
    return int(match.group(1)) if match else None


def _remove_if_exists(path):
    try:
        os.remove(path)
    except FileNotFoundError:
        pass


def _download_file(
    repo_id,
    filename,
    dest_path,
    model_type,
    file_idx,
    file_total,
    endpoint,
    expected_size=None,
    revision="main",
):
    """下载单个文件，支持断点续传和进度上报"""
    url = _resolve_download_url(endpoint, repo_id, filename, revision)

    dest_dir = os.path.dirname(dest_path)
    os.makedirs(dest_dir, exist_ok=True)

    if expected_size is None:
        expected_size = _remote_file_size(url)
    if os.path.exists(dest_path):
        final_size = os.path.getsize(dest_path)
        if expected_size is None and final_size > 0:
            return
        if expected_size is not None and final_size == expected_size:
            return
        stale_path = dest_path + ".incomplete"
        try:
            if not os.path.exists(stale_path) or os.path.getsize(stale_path) < final_size:
                os.replace(dest_path, stale_path)
            else:
                os.remove(dest_path)
        except OSError:
            os.remove(dest_path)

    tmp_path = dest_path + ".incomplete"

    # 断点续传
    downloaded = 0
    if os.path.exists(tmp_path):
        downloaded = os.path.getsize(tmp_path)
        if expected_size is not None and downloaded > expected_size:
            os.remove(tmp_path)
            downloaded = 0

    for attempt in range(2):
        headers = {"Accept-Encoding": "identity"}
        if downloaded > 0:
            headers["Range"] = f"bytes={downloaded}-"

        resp = requests.get(url, headers=headers, stream=True, timeout=60, allow_redirects=True)

        if resp.status_code == 416:
            remote_total = _parse_unsatisfied_range_total(resp.headers.get("Content-Range"))
            complete = (
                expected_size is not None
                and downloaded == expected_size
                and (remote_total is None or remote_total == expected_size)
            ) or (
                expected_size is None
                and remote_total is not None
                and downloaded == remote_total
            )
            getattr(resp, "close", lambda: None)()
            if complete and downloaded > 0:
                os.replace(tmp_path, dest_path)
                return
            _remove_if_exists(tmp_path)
            downloaded = 0
            if attempt == 0:
                continue
            raise RuntimeError(f"{filename} 服务器拒绝完整下载请求")

        try:
            resp.raise_for_status()
        except Exception:
            getattr(resp, "close", lambda: None)()
            raise
        total_size = expected_size or 0
        expected_range_end = None

        if resp.status_code == 206:
            parsed_range = _parse_content_range(resp.headers.get("Content-Range"))
            valid_range = parsed_range is not None
            if parsed_range is not None:
                range_start, range_end, range_total = parsed_range
                valid_range = range_start == downloaded and range_end >= range_start
                expected_range_end = range_end
                if expected_size is not None and range_total is not None:
                    valid_range = valid_range and range_total == expected_size
                if range_total is not None:
                    valid_range = valid_range and range_end < range_total
                if expected_size is None:
                    valid_range = valid_range and range_total is not None
                    if range_total is not None:
                        total_size = range_total

            if not valid_range:
                getattr(resp, "close", lambda: None)()
                _remove_if_exists(tmp_path)
                downloaded = 0
                if attempt == 0:
                    continue
                raise RuntimeError(f"{filename} 服务器返回了无效的 Content-Range")
            mode = "ab" if downloaded > 0 else "wb"
        elif resp.status_code == 200:
            # 服务器忽略 Range 时从零覆盖，不能把完整响应追加到 partial。
            downloaded = 0
            mode = "wb"
            content_length = resp.headers.get("Content-Length")
            if total_size == 0 and content_length:
                try:
                    parsed_content_length = int(content_length)
                except (TypeError, ValueError):
                    parsed_content_length = 0
                if parsed_content_length > 0:
                    total_size = parsed_content_length
        else:
            getattr(resp, "close", lambda: None)()
            raise RuntimeError(f"{filename} 下载返回异常状态码: {resp.status_code}")

        current = downloaded
        last_pct = -1
        try:
            with open(tmp_path, mode) as f:
                for chunk in resp.iter_content(chunk_size=1024 * 1024):
                    if not chunk:
                        continue
                    f.write(chunk)
                    current += len(chunk)
                    if total_size > 0:
                        pct = int(current * 100 / total_size)
                        if pct != last_pct:
                            last_pct = pct
                            _emit(model_type, "downloading", pct,
                                  message=f"[{file_idx}/{file_total}] {filename} {pct}%")
                f.flush()
                os.fsync(f.fileno())
        finally:
            getattr(resp, "close", lambda: None)()

        if expected_range_end is not None and current - 1 != expected_range_end:
            # 响应体与服务端声明的区间不一致，现有 partial 的字节边界不可信。
            _remove_if_exists(tmp_path)
            downloaded = 0
            if attempt == 0:
                continue
            raise RuntimeError(
                f"{filename} Content-Range 与响应长度不一致: "
                f"end={expected_range_end}, received_end={current - 1}"
            )
        if expected_size is not None and current != expected_size:
            raise RuntimeError(f"{filename} 下载不完整: got={current}, expected={expected_size}")
        if total_size > 0 and current != total_size:
            raise RuntimeError(f"{filename} 下载不完整: got={current}, expected={total_size}")

        os.replace(tmp_path, dest_path)
        return

    raise RuntimeError(f"{filename} 下载失败")


def _sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _write_completion_manifest(snapshot_dir, repo_id, commit_hash, files):
    manifest_files = []
    for item in files:
        filename = item["rfilename"]
        path = os.path.join(snapshot_dir, filename.replace("/", os.sep))
        size = item.get("size")
        if size is None:
            size = os.path.getsize(path)
        actual_size = os.path.getsize(path)
        if actual_size != size:
            raise RuntimeError(f"{filename} 文件大小校验失败: got={actual_size}, expected={size}")
        manifest_item = {"path": filename, "size": size}
        expected_sha256 = item.get("sha256")
        if expected_sha256:
            actual_sha256 = _sha256_file(path)
            if actual_sha256.lower() != expected_sha256.lower():
                raise RuntimeError(
                    f"{filename} SHA256 校验失败: got={actual_sha256}, expected={expected_sha256}"
                )
            manifest_item["sha256"] = expected_sha256
        manifest_files.append(manifest_item)

    manifest = {
        "repo_id": repo_id,
        "commit_hash": commit_hash,
        "files": manifest_files,
    }
    tmp_path = os.path.join(snapshot_dir, COMPLETE_MANIFEST_NAME + ".tmp")
    final_path = os.path.join(snapshot_dir, COMPLETE_MANIFEST_NAME)
    with open(tmp_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp_path, final_path)


def _cleanup_locks(repo_id):
    """清理残留的 .lock 和 .incomplete 文件"""
    cache_root = get_hf_cache_root()
    dir_name = "models--" + repo_id.replace("/", "--")

    import glob
    cleanup_incomplete_files(repo_id)

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

    last_error = None
    endpoints = _candidate_endpoints()

    for idx, endpoint in enumerate(endpoints, 1):
        if idx > 1:
            _emit(
                model_type,
                "downloading",
                0,
                message=f"主站不可用，正在切换镜像 {endpoint} ..."
            )
        else:
            _emit(model_type, "downloading", 0, message=f"正在获取 {model_name} 文件列表...")

        try:
            commit_hash, files = _get_repo_info(model_name, endpoint)

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
            for file_idx, file_info in enumerate(files, 1):
                filename = file_info["rfilename"]
                dest_path = os.path.join(snapshot_dir, filename.replace("/", os.sep))

                _download_file(
                    model_name,
                    filename,
                    dest_path,
                    model_type,
                    file_idx,
                    file_total,
                    endpoint,
                    expected_size=file_info.get("size"),
                    revision=commit_hash,
                )

            _write_completion_manifest(snapshot_dir, model_name, commit_hash, files)
            _emit(model_type, "completed", 100, message=f"{model_name} 下载完成")
            return {"success": True, "model": model_type, "endpoint": endpoint}
        except Exception as e:
            last_error = e

    error_message = str(last_error) if last_error else "模型下载失败"
    _emit(model_type, "error", 0, error_message, message=f"{model_name} 下载失败: {error_message}")
    return {"success": False, "model": model_type, "error": error_message}


def main(engine=None):
    global _total_count

    if engine is None:
        import argparse
        parser = argparse.ArgumentParser()
        parser.add_argument("--engine", default="sensevoice", choices=["sensevoice", "whisper"])
        args = parser.parse_args()
        engine = args.engine

    if engine == "whisper":
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
