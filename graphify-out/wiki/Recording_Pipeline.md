# Recording Pipeline

> 35 nodes · cohesion 0.11

## Key Concepts

- **audio_service.rs** (33 connections) — `src-tauri\src\services\audio_service.rs`
- **start_microphone_level_monitor()** (8 connections) — `src-tauri\src\services\audio_service.rs`
- **finalize_recording()** (6 connections) — `src-tauri\src\services\audio_service.rs`
- **spawn_audio_capture_thread()** (6 connections) — `src-tauri\src\services\audio_service.rs`
- **do_final_asr()** (5 connections) — `src-tauri\src\services\audio_service.rs`
- **emit_done()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **emit_error()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **f32_to_i16()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **load_best_input_config()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **resample_to_16k()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **resolve_input_device()** (4 connections) — `src-tauri\src\services\audio_service.rs`
- **One-key dictation** (4 connections) — `README.md`
- **do_paste()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **emit_recording_state_if_current()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **flush_pending_paste()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **mix_to_mono_f32()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **mix_to_mono_u16()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **mono_peak_f32()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **mono_peak_u16()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **schedule_hide()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **spawn_interim_loop()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **test_microphone_sync()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **u16_to_i16()** (3 connections) — `src-tauri\src\services\audio_service.rs`
- **adjust_interval()** (2 connections) — `src-tauri\src\services\audio_service.rs`
- **compute_waveform_bars()** (2 connections) — `src-tauri\src\services\audio_service.rs`
- *... and 10 more nodes in this community*

## Relationships

- No strong cross-community connections detected

## Source Files

- `README.md`
- `src-tauri\src\services\audio_service.rs`

## Audit Trail

- EXTRACTED: 135 (97%)
- INFERRED: 4 (3%)
- AMBIGUOUS: 0 (0%)

---

*Part of the graphify knowledge wiki. See [[index]] to navigate.*