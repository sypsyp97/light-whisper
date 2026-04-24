# BaseASRServer

> God node · 25 connections · `C:\Users\sun\Downloads\light-whisper\src-tauri\resources\server_common.py`

## Connections by Relation

### contains
- [[server_common.py]] `EXTRACTED`

### implements
- [[Python ASR runtime]] `INFERRED`

### method
- [[.run()]] `EXTRACTED`
- [[.__init__()]] `EXTRACTED`
- [[._detect_device()]] `EXTRACTED`
- [[._get_model_repos()]] `EXTRACTED`
- [[._setup_runtime_environment()]] `EXTRACTED`
- [[._get_audio_duration()]] `EXTRACTED`
- [[._cleanup_memory()]] `EXTRACTED`
- [[._maybe_cleanup()]] `EXTRACTED`
- [[._get_gpu_device_info()]] `EXTRACTED`
- [[.initialize()]] `EXTRACTED`
- [[.check_status()]] `EXTRACTED`
- [[.get_performance_stats()]] `EXTRACTED`
- [[.transcribe_audio()]] `EXTRACTED`
- [[._signal_handler()]] `EXTRACTED`

### rationale_for
- [[Base class for ASR server implementations.      Subclasses must implement:]] `EXTRACTED`

### uses
- [[WhisperServer]] `INFERRED`
- [[FunASRServer]] `INFERRED`
- [[Skip FunASR's model-side pip auto-install in bundled runtime.      Some FunASR]] `INFERRED`
- [[加载ASR模型（SenseVoiceSmall + fsmn-vad）]] `INFERRED`
- [[首次推理会懒加载 CUDA kernel / 计算图，冷启动 2-4s。         加载后立刻用一段 1s 低幅噪声跑一次 dummy generate]] `INFERRED`
- [[Whisper-specific device detection: also checks CTranslate2 CUDA support.]] `INFERRED`
- [[初始化 Faster Whisper 模型]] `INFERRED`
- [[CTranslate2 / Silero VAD 首次推理会懒加载 CUDA 内核和 VAD 模型，         冷启动 2-3s。加载后立刻用一段 1s]] `INFERRED`

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*