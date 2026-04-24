# HF Cache Utils

> 10 nodes · cohesion 0.29

## Key Concepts

- **hf_cache_utils.py** (6 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **is_hf_repo_ready()** (5 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **get_hf_cache_root()** (4 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **cleanup_incomplete_files()** (3 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **_snapshot_matches_completion_manifest()** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **_snapshot_matches_legacy_weight_check()** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **HuggingFace 缓存检测共享工具模块  供 funasr_server.py 和 download_models.py 共同使用， 避免重复实现缓** (1 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **返回 HuggingFace 缓存根目录      优先级：HF_HUB_CACHE（由 Rust 设置的自定义路径）> HF_HOME/hub > 默认** (1 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。      仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshot** (1 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`
- **删除某个 repo 缓存目录下残留的 .incomplete 文件，返回删除数量。** (1 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`

## Relationships

- No strong cross-community connections detected

## Source Files

- `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\hf_cache_utils.py`

## Audit Trail

- EXTRACTED: 26 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*