





import os
import json
import hashlib

QWEN3_ASR_MODELS = {
    "qwen3-asr-0.6b": {
        "repo_id": "audio-cpp/audio.cpp-gguf",
        "filename": "Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf",
        "revision": "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5",
        "size": 1_151_272_416,
        "sha256": "6c44ec2fb4cee513892d7863c1fcc3ea6b699ffa4d899b0ef4ab19956d9544f7",
    },
}

R2T2_MODEL = {
    "repo_id": "davidxifeng/Confucius4-R2T2-gguf",
    "filename": "r2t2-q8_0.gguf",
    "revision": "a8e6b385d7df7eae9519363e07034a209004797a",
    "size": 2_477_512_064,
    "sha256": "19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5",
}

LOCAL_ASR_MODELS = {**QWEN3_ASR_MODELS, "confucius4-r2t2": R2T2_MODEL}

_WEIGHT_EXTS = (".pt", ".bin", ".safetensors", ".onnx", ".gguf")
_MIN_WEIGHT_SIZE = 1_000_000
COMPLETE_MANIFEST_NAME = ".light_whisper_complete.json"


def get_hf_cache_root():




    hf_hub_cache = os.environ.get("HF_HUB_CACHE")
    if hf_hub_cache:
        return hf_hub_cache
    hf_home = os.environ.get("HF_HOME")
    if hf_home:
        return os.path.join(hf_home, "hub")
    return os.path.join(os.path.expanduser("~"), ".cache", "huggingface", "hub")


def is_hf_repo_ready(repo_id):





    return any(_snapshot_is_ready(path) for path in _snapshot_paths(repo_id))


def find_hf_snapshot_file(repo_id, filename):

    for snapshot_path in _snapshot_paths(repo_id):
        if _snapshot_is_ready(snapshot_path, filename):
            return os.path.join(snapshot_path, filename.replace("/", os.sep))
    return None


def _snapshot_paths(repo_id):
    cache_root = get_hf_cache_root()
    repo_dir = os.path.join(cache_root, "models--" + repo_id.replace("/", "--"))
    snapshots_dir = os.path.join(repo_dir, "snapshots")
    try:
        names = sorted(os.listdir(snapshots_dir))
    except OSError:
        return []

    snapshot_names = []
    try:
        with open(os.path.join(repo_dir, "refs", "main"), "r", encoding="utf-8") as f:
            revision = f.read().strip()
        if revision in names:
            snapshot_names.append(revision)
    except (OSError, UnicodeError):
        pass
    snapshot_names.extend(name for name in names if name not in snapshot_names)
    return [
        os.path.join(snapshots_dir, name)
        for name in snapshot_names
        if os.path.isdir(os.path.join(snapshots_dir, name))
    ]


def _snapshot_is_ready(snapshot_path, required_file=None):
    if required_file is not None:
        candidate = os.path.join(snapshot_path, required_file.replace("/", os.sep))
        try:
            if not os.path.isfile(candidate) or os.path.getsize(candidate) < _MIN_WEIGHT_SIZE:
                return False
        except OSError:
            return False
    if os.path.lexists(os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)):
        return _snapshot_matches_completion_manifest(snapshot_path, required_file)
    return _snapshot_matches_legacy_weight_check(snapshot_path)


def cleanup_incomplete_files(repo_id):

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


def _snapshot_matches_completion_manifest(snapshot_path, required_file=None):
    manifest_path = os.path.join(snapshot_path, COMPLETE_MANIFEST_NAME)
    try:
        with open(manifest_path, "r", encoding="utf-8") as f:
            manifest = json.load(f)
    except (OSError, ValueError, UnicodeError):
        return False

    if not isinstance(manifest, dict):
        return False
    files = manifest.get("files")
    if not isinstance(files, list) or not files:
        return False

    has_weight = False
    has_required_file = required_file is None
    for item in files:
        if not isinstance(item, dict):
            return False
        rel_path = item.get("path")
        expected_size = item.get("size")
        expected_sha256 = item.get("sha256")
        if (
            not isinstance(rel_path, str)
            or type(expected_size) is not int
            or expected_size < 0
        ):
            return False
        if expected_sha256 is not None and (
            not isinstance(expected_sha256, str)
            or len(expected_sha256) != 64
            or any(char not in "0123456789abcdefABCDEF" for char in expected_sha256)
        ):
            return False
        if (
            not rel_path
            or os.path.isabs(rel_path)
            or os.path.splitdrive(rel_path)[0]
            or ".." in rel_path.replace("\\", "/").split("/")
        ):
            return False
        path = os.path.join(snapshot_path, rel_path.replace("/", os.sep))
        try:
            if not os.path.isfile(path):
                return False
            actual_size = os.path.getsize(path)
            if actual_size != expected_size:
                return False
            if expected_sha256 is not None and _sha256_file(path).lower() != expected_sha256.lower():
                return False
        except OSError:
            return False
        if rel_path == required_file:
            has_required_file = True
        if rel_path.endswith(_WEIGHT_EXTS) and actual_size >= _MIN_WEIGHT_SIZE:
            has_weight = True

    return has_weight and has_required_file


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
