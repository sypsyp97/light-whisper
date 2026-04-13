# Funasr Server

> 25 nodes · cohesion 0.09

## Key Concepts

- **WhisperServer** (11 connections) — `src-tauri\resources\whisper_server.py`
- **FunASRServer** (10 connections) — `src-tauri\resources\funasr_server.py`
- **.initialize()** (4 connections) — `src-tauri\resources\whisper_server.py`
- **.initialize()** (3 connections) — `src-tauri\resources\funasr_server.py`
- **._load_asr_model()** (3 connections) — `src-tauri\resources\funasr_server.py`
- **BaseASRServer** (2 connections)
- **_disable_funasr_auto_requirement_install()** (2 connections) — `src-tauri\resources\funasr_server.py`
- **.transcribe_audio()** (2 connections) — `src-tauri\resources\funasr_server.py`
- **funasr_server.py** (2 connections) — `src-tauri\resources\funasr_server.py`
- **._detect_device()** (2 connections) — `src-tauri\resources\whisper_server.py`
- **._load_model()** (2 connections) — `src-tauri\resources\whisper_server.py`
- **.transcribe_audio()** (2 connections) — `src-tauri\resources\whisper_server.py`
- **.check_status()** (1 connections) — `src-tauri\resources\funasr_server.py`
- **._get_model_repos()** (1 connections) — `src-tauri\resources\funasr_server.py`
- **.get_performance_stats()** (1 connections) — `src-tauri\resources\funasr_server.py`
- **.__init__()** (1 connections) — `src-tauri\resources\funasr_server.py`
- **Skip FunASR's model-side pip auto-install in bundled runtime.      Some FunASR H** (1 connections) — `src-tauri\resources\funasr_server.py`
- **加载ASR模型（SenseVoiceSmall + fsmn-vad）** (1 connections) — `src-tauri\resources\funasr_server.py`
- **whisper_server.py** (1 connections) — `src-tauri\resources\whisper_server.py`
- **Whisper-specific device detection: also checks CTranslate2 CUDA support.** (1 connections) — `src-tauri\resources\whisper_server.py`
- **初始化 Faster Whisper 模型** (1 connections) — `src-tauri\resources\whisper_server.py`
- **.check_status()** (1 connections) — `src-tauri\resources\whisper_server.py`
- **._get_model_repos()** (1 connections) — `src-tauri\resources\whisper_server.py`
- **.get_performance_stats()** (1 connections) — `src-tauri\resources\whisper_server.py`
- **.__init__()** (1 connections) — `src-tauri\resources\whisper_server.py`

## Relationships

- No strong cross-community connections detected

## Source Files

- `src-tauri\resources\funasr_server.py`
- `src-tauri\resources\whisper_server.py`

## Audit Trail

- EXTRACTED: 56 (97%)
- INFERRED: 2 (3%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*