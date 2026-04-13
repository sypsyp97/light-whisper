# HF Cache Utilities

> 6 nodes · cohesion 0.40

## Key Concepts

- **get_hf_cache_root()** (3 connections) — `src-tauri\resources\hf_cache_utils.py`
- **is_hf_repo_ready()** (3 connections) — `src-tauri\resources\hf_cache_utils.py`
- **hf_cache_utils.py** (3 connections) — `src-tauri\resources\hf_cache_utils.py`
- **HuggingFace 缓存检测共享工具模块  供 funasr_server.py 和 download_models.py 共同使用， 避免重复实现缓** (1 connections) — `src-tauri\resources\hf_cache_utils.py`
- **返回 HuggingFace 缓存根目录      优先级：HF_HUB_CACHE（由 Rust 设置的自定义路径）> HF_HOME/hub > 默认** (1 connections) — `src-tauri\resources\hf_cache_utils.py`
- **检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。      仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshot** (1 connections) — `src-tauri\resources\hf_cache_utils.py`

## Relationships

- No strong cross-community connections detected

## Source Files

- `src-tauri\resources\hf_cache_utils.py`

## Audit Trail

- EXTRACTED: 12 (100%)
- INFERRED: 0 (0%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*