# Release Packaging

> 30 nodes · cohesion 0.10

## Key Concepts

- **build_engine.py** (10 connections) — `scripts\build_engine.py`
- **main()** (8 connections) — `scripts\build_engine.py`
- **create_tar_xz()** (6 connections) — `scripts\build_engine.py`
- **build.rs** (6 connections) — `src-tauri\build.rs`
- **create_tar_xz_with_python()** (4 connections) — `scripts\build_engine.py`
- **main()** (4 connections) — `src-tauri\build.rs`
- **Release workflow** (4 connections) — `RELEASE_GUIDE.md`
- **create_tar_xz_with_7z()** (3 connections) — `scripts\build_engine.py`
- **remove_tree()** (3 connections) — `scripts\build_engine.py`
- **strip_cuda_dlls()** (3 connections) — `scripts\build_engine.py`
- **strip_dev_artifacts()** (3 connections) — `scripts\build_engine.py`
- **validate_torch_cuda_deps()** (3 connections) — `scripts\build_engine.py`
- **selected_engine_archive_path()** (3 connections) — `src-tauri\build.rs`
- **Codex OAuth polish support** (3 connections) — `RELEASE_GUIDE.md`
- **Python engine packaging** (3 connections) — `RELEASE_GUIDE.md`
- **emit_rerun_hints()** (2 connections) — `src-tauri\build.rs`
- **find_7z_executable()** (2 connections) — `scripts\build_engine.py`
- **get_size_mb()** (2 connections) — `scripts\build_engine.py`
- **has_non_empty_archive()** (2 connections) — `src-tauri\build.rs`
- **Installer build** (2 connections) — `RELEASE_GUIDE.md`
- **compute_file_fingerprint()** (1 connections) — `src-tauri\build.rs`
- **删除目录；在 Windows 文件句柄尚未释放时做有限重试。** (1 connections) — `scripts\build_engine.py`
- **删除可安全裁剪的 CUDA DLL，返回节省的 MB 数** (1 connections) — `scripts\build_engine.py`
- **删除运行时不需要的链接/调试产物，返回节省的 MB 数。** (1 connections) — `scripts\build_engine.py`
- **校验 torch_cuda.dll 的直接 CUDA 依赖仍然存在。** (1 connections) — `scripts\build_engine.py`
- *... and 5 more nodes in this community*

## Relationships

- No strong cross-community connections detected

## Source Files

- `RELEASE_GUIDE.md`
- `scripts\build_engine.py`
- `src-tauri\build.rs`

## Audit Trail

- EXTRACTED: 78 (91%)
- INFERRED: 8 (9%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*