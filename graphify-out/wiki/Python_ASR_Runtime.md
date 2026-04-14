# Python ASR Runtime

> 69 nodes · cohesion 0.04

## Key Concepts

- **BaseASRServer** (25 connections) — `src-tauri\resources\server_common.py`
- **WhisperServer** (13 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\whisper_server.py`
- **FunASRServer** (12 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **server_common.py** (11 connections) — `src-tauri\resources\server_common.py`
- **.run()** (7 connections) — `src-tauri\resources\server_common.py`
- **.initialize()** (5 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\whisper_server.py`
- **.initialize()** (4 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **.__init__()** (4 connections) — `src-tauri\resources\server_common.py`
- **StdoutSuppressor** (4 connections) — `src-tauri\resources\server_common.py`
- **._load_asr_model()** (3 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **._warmup_inference()** (3 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **._detect_device()** (3 connections) — `src-tauri\resources\server_common.py`
- **._get_model_repos()** (3 connections) — `src-tauri\resources\server_common.py`
- **decode_inline_audio()** (3 connections) — `src-tauri\resources\server_common.py`
- **ensure_safe_cuda_env()** (3 connections) — `src-tauri\resources\server_common.py`
- **get_log_path()** (3 connections) — `src-tauri\resources\server_common.py`
- **get_wav_duration_seconds()** (3 connections) — `src-tauri\resources\server_common.py`
- **_has_nvidia_gpu()** (3 connections) — `src-tauri\resources\server_common.py`
- **setup_rotating_logger()** (3 connections) — `src-tauri\resources\server_common.py`
- **._warmup_inference()** (3 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\whisper_server.py`
- **BaseASRServer** (2 connections)
- **funasr_server.py** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **_disable_funasr_auto_requirement_install()** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **.transcribe_audio()** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- **首次推理会懒加载 CUDA kernel / 计算图，冷启动 2-4s。         加载后立刻用一段 1s 低幅噪声跑一次 dummy generate** (2 connections) — `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- *... and 44 more nodes in this community*

## Relationships

- No strong cross-community connections detected

## Source Files

- `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py`
- `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\whisper_server.py`
- `src-tauri\resources\server_common.py`

## Audit Trail

- EXTRACTED: 172 (90%)
- INFERRED: 19 (10%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*