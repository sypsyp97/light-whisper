# Graph Report - .  (2026-04-24)

## Corpus Check
- 111 files · ~91,331 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1168 nodes · 1952 edges · 99 communities detected
- Extraction: 98% EXTRACTED · 2% INFERRED · 0% AMBIGUOUS · INFERRED: 41 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Hotkey|Hotkey]]
- [[_COMMUNITY_LLM Provider|LLM Provider]]
- [[_COMMUNITY_Server Common|Server Common]]
- [[_COMMUNITY_LLM Client|LLM Client]]
- [[_COMMUNITY_App State|App State]]
- [[_COMMUNITY_Funasr Service|Funasr Service]]
- [[_COMMUNITY_Web Search Service|Web Search Service]]
- [[_COMMUNITY_Tauri|Tauri]]
- [[_COMMUNITY_Codex OAuth Service|Codex OAuth Service]]
- [[_COMMUNITY_Profile Service|Profile Service]]
- [[_COMMUNITY_Paths|Paths]]
- [[_COMMUNITY_User Profile|User Profile]]
- [[_COMMUNITY_Ai Polish Service|Ai Polish Service]]
- [[_COMMUNITY_Release Packaging|Release Packaging]]
- [[_COMMUNITY_Funasr|Funasr]]
- [[_COMMUNITY_Profile|Profile]]
- [[_COMMUNITY_Settings Page|Settings Page]]
- [[_COMMUNITY_Alibaba ASR Service|Alibaba ASR Service]]
- [[_COMMUNITY_Foreground Context|Foreground Context]]
- [[_COMMUNITY_Window|Window]]
- [[_COMMUNITY_Test Download Models Regression|Test Download Models Regression]]
- [[_COMMUNITY_Audio|Audio]]
- [[_COMMUNITY_Clipboard|Clipboard]]
- [[_COMMUNITY_Rebuild Graphify|Rebuild Graphify]]
- [[_COMMUNITY_Download Models|Download Models]]
- [[_COMMUNITY_Lib|Lib]]
- [[_COMMUNITY_Ai Polish|Ai Polish]]
- [[_COMMUNITY_HF Cache Utils|HF Cache Utils]]
- [[_COMMUNITY_Updater|Updater]]
- [[_COMMUNITY_Download Service|Download Service]]
- [[_COMMUNITY_Capture|Capture]]
- [[_COMMUNITY_Finalize|Finalize]]
- [[_COMMUNITY_Hotkey Normalization|Hotkey Normalization]]
- [[_COMMUNITY_Openai Fast Mode OAuth Tests|Openai Fast Mode OAuth Tests]]
- [[_COMMUNITY_Assistant Commands|Assistant Commands]]
- [[_COMMUNITY_Monitor|Monitor]]
- [[_COMMUNITY_Sound|Sound]]
- [[_COMMUNITY_Engine|Engine]]
- [[_COMMUNITY_Status Indicator UI|Status Indicator UI]]
- [[_COMMUNITY_Model Status Hook|Model Status Hook]]
- [[_COMMUNITY_Theme Hook|Theme Hook]]
- [[_COMMUNITY_Resample|Resample]]
- [[_COMMUNITY_Frontend Entry|Frontend Entry]]
- [[_COMMUNITY_Hotkey Storage Hook|Hotkey Storage Hook]]
- [[_COMMUNITY_Use Smooth Text|Use Smooth Text]]
- [[_COMMUNITY_Codex OAuth Commands|Codex OAuth Commands]]
- [[_COMMUNITY_Ai Polish Transport Retry Tests|Ai Polish Transport Retry Tests]]
- [[_COMMUNITY_Wav|Wav]]
- [[_COMMUNITY_App Errors|App Errors]]
- [[_COMMUNITY_Promo App Window|Promo App Window]]
- [[_COMMUNITY_Recording Context|Recording Context]]
- [[_COMMUNITY_Use Recording|Use Recording]]
- [[_COMMUNITY_Local Storage Helpers|Local Storage Helpers]]
- [[_COMMUNITY_Interim|Interim]]
- [[_COMMUNITY_Audio Service Module|Audio Service Module]]
- [[_COMMUNITY_Promo Root|Promo Root]]
- [[_COMMUNITY_Promo Cursor|Promo Cursor]]
- [[_COMMUNITY_Title Bar|Title Bar]]
- [[_COMMUNITY_Recording Context.test|Recording Context.test]]
- [[_COMMUNITY_Debounced Callback Hook|Debounced Callback Hook]]
- [[_COMMUNITY_Exclusive Picker Hook|Exclusive Picker Hook]]
- [[_COMMUNITY_Hotkey Capture Hook|Hotkey Capture Hook]]
- [[_COMMUNITY_Use Recording.test|Use Recording.test]]
- [[_COMMUNITY_Fast Mode|Fast Mode]]
- [[_COMMUNITY_Tauri Event Mock|Tauri Event Mock]]
- [[_COMMUNITY_Vite Config|Vite Config]]
- [[_COMMUNITY_Vitest.config|Vitest.config]]
- [[_COMMUNITY_Remotion Config|Remotion Config]]
- [[_COMMUNITY_Promo Src Index|Promo Src Index]]
- [[_COMMUNITY_Promo Showcase Video|Promo Showcase Video]]
- [[_COMMUNITY_Promo Theme|Promo Theme]]
- [[_COMMUNITY_Promo Cinematic Scene|Promo Cinematic Scene]]
- [[_COMMUNITY_Promo Mic Button|Promo Mic Button]]
- [[_COMMUNITY_Promo Result Card|Promo Result Card]]
- [[_COMMUNITY_Promo Subtitle Capsule|Promo Subtitle Capsule]]
- [[_COMMUNITY_Promo Assistant Scene|Promo Assistant Scene]]
- [[_COMMUNITY_Promo Dictation Scene|Promo Dictation Scene]]
- [[_COMMUNITY_Promo Edit Scene|Promo Edit Scene]]
- [[_COMMUNITY_Promo Intro Scene|Promo Intro Scene]]
- [[_COMMUNITY_Promo Outro Scene|Promo Outro Scene]]
- [[_COMMUNITY_Promo Translation Scene|Promo Translation Scene]]
- [[_COMMUNITY_Vite Env Types|Vite Env Types]]
- [[_COMMUNITY_Tauri.fast Mode.test|Tauri.fast Mode.test]]
- [[_COMMUNITY_Kbd|Kbd]]
- [[_COMMUNITY_Recording Button UI|Recording Button UI]]
- [[_COMMUNITY_Transcription History UI|Transcription History UI]]
- [[_COMMUNITY_Transcription Result UI|Transcription Result UI]]
- [[_COMMUNITY_English I18n|English I18n]]
- [[_COMMUNITY_I18n Index|I18n Index]]
- [[_COMMUNITY_Chinese I18n|Chinese I18n]]
- [[_COMMUNITY_App Constants|App Constants]]
- [[_COMMUNITY_Fast Mode.test|Fast Mode.test]]
- [[_COMMUNITY_Main Page|Main Page]]
- [[_COMMUNITY_Setup|Setup]]
- [[_COMMUNITY_Types Index|Types Index]]
- [[_COMMUNITY_Commands Module|Commands Module]]
- [[_COMMUNITY_Services Module|Services Module]]
- [[_COMMUNITY_State Module|State Module]]
- [[_COMMUNITY_Utils Module|Utils Module]]

## God Nodes (most connected - your core abstractions)
1. `invokeCommand()` - 50 edges
2. `AppState` - 31 edges
3. `BaseASRServer` - 25 edges
4. `build_llm_body()` - 21 edges
5. `Light-Whisper overview` - 18 edges
6. `openai_endpoint()` - 16 edges
7. `endpoint_for_preview()` - 16 edges
8. `register_custom_hotkey()` - 15 edges
9. `login()` - 15 edges
10. `adapt_body_for_backend()` - 14 edges

## Surprising Connections (you probably didn't know these)
- `BaseASRServer` --implements--> `Python ASR runtime`  [INFERRED]
  C:\Users\sun\Downloads\light-whisper\src-tauri\resources\server_common.py → README.md
- `FunASRServer` --implements--> `SenseVoice engine`  [INFERRED]
  C:\Users\sun\Downloads\light-whisper\src-tauri\resources\funasr_server.py → README.md
- `WhisperServer` --implements--> `Faster Whisper engine`  [INFERRED]
  C:\Users\sun\Downloads\light-whisper\src-tauri\resources\whisper_server.py → README.md
- `register_custom_hotkey()` --implements--> `One-key dictation`  [INFERRED]
  C:\Users\sun\Downloads\light-whisper\src-tauri\src\commands\hotkey.rs → README.md
- `register_translation_hotkey()` --implements--> `Real-time translation`  [INFERRED]
  C:\Users\sun\Downloads\light-whisper\src-tauri\src\commands\hotkey.rs → README.md

## Hyperedges (group relationships)
- **Application stack** — architecture_react_ui, architecture_rust_core, architecture_python_asr_runtime, architecture_llm_integration [EXTRACTED 1.00]
- **Speech engines** — engine_sensevoice, engine_faster_whisper, engine_glm_asr [EXTRACTED 1.00]
- **Assistant context stack** — feature_voice_assistant, feature_web_search_context, feature_screen_context, architecture_llm_integration [INFERRED 0.90]
- **Release pipeline** — release_engine_packaging, release_installer_build, release_github_release [EXTRACTED 1.00]

## Communities

### Community 0 - "Hotkey"
Cohesion: 0.07
Nodes (75): all_modifiers_down(), build_hook_state(), build_hook_state_with_backend(), classify_backend(), dispatch_channel(), dispatch_hotkey_press(), dispatch_hotkey_release(), DispatchEvent (+67 more)

### Community 1 - "LLM Provider"
Cohesion: 0.05
Nodes (66): apply_reasoning_controls(), assistant_endpoint_for_config(), assistant_endpoint_uses_separate_model_for_builtin_provider(), assistant_endpoint_uses_separate_model_for_custom_provider(), build_auth_headers(), builds_cerebras_image_support_probe_url(), cerebras_glm_reports_reasoning_support(), cerebras_public_model_probe_url() (+58 more)

### Community 2 - "Server Common"
Cohesion: 0.04
Nodes (33): BaseASRServer, _disable_funasr_auto_requirement_install(), FunASRServer, 首次推理会懒加载 CUDA kernel / 计算图，冷启动 2-4s。         加载后立刻用一段 1s 低幅噪声跑一次 dummy generate, Skip FunASR's model-side pip auto-install in bundled runtime.      Some FunASR, 加载ASR模型（SenseVoiceSmall + fsmn-vad）, apply_hf_env_defaults(), BaseASRServer (+25 more)

### Community 3 - "LLM Client"
Cohesion: 0.09
Nodes (52): adapt_body_for_backend(), anthropic_output_tokens(), api_error_message_falls_back_to_openai_compat_parser(), build_llm_body(), build_stream_event_payload(), cerebras_json_output_disables_stream_to_preserve_response_format(), cerebras_without_json_output_keeps_stream(), chat_body_keeps_provider_default_reasoning() (+44 more)

### Community 4 - "App State"
Cohesion: 0.04
Nodes (16): AppState, DictationOutputMode, DownloadTask, EngineState, FunasrProcess, HotkeyDiagnosticState, InterimCache, MicrophoneLevelMonitor (+8 more)

### Community 5 - "Funasr Service"
Cohesion: 0.07
Nodes (52): check_model_files(), check_status(), create_temp_audio_path(), encode_pcm16_base64(), encode_wav_bytes(), engine_extraction_preserves_existing_engine_until_archive_succeeds(), EngineRuntime, expected_engine_install_fingerprint() (+44 more)

### Community 6 - "Web Search Service"
Cohesion: 0.05
Nodes (42): LLM integration layer, Python ASR runtime, React UI, Rust core, assistant_input_preserves_symbols_and_splits_cdata(), build_assistant_user_content_with_selection(), generate_content(), render_assistant_user_content() (+34 more)

### Community 7 - "Tauri"
Cohesion: 0.07
Nodes (50): addCustomProvider(), addHotWord(), copyToClipboard(), getAiPolishApiKey(), getAlibabaAsrConfig(), getAssistantApiKey(), getLlmReasoningSupport(), getModelsDir() (+42 more)

### Community 8 - "Codex OAuth Service"
Cohesion: 0.08
Nodes (49): accept_callback_connection(), AuthClaims, base64_url_encode(), bind_callback_listeners(), build_authorize_url(), callback_html(), CallbackListeners, ChatgptBearerToken (+41 more)

### Community 9 - "Profile Service"
Cohesion: 0.11
Nodes (42): Adaptive learning, add_hot_word(), cleanup_profile(), collect_diff_correction_pairs(), collect_diff_correction_pairs_merges_and_dedupes_baselines(), contains_sentence_punctuation(), extract_diff_segments(), finalize_learning() (+34 more)

### Community 10 - "Paths"
Cohesion: 0.1
Nodes (34): atomic_write(), default_hf_cache_root(), engine_json_array_normalizes_to_empty_object(), engine_json_object_or_empty(), engine_json_string_normalizes_to_empty_object(), get_data_dir(), get_download_script_path(), get_effective_models_dir() (+26 more)

### Community 11 - "User Profile"
Cohesion: 0.08
Nodes (19): ApiFormat, CorrectionPattern, CorrectionSource, CustomProvider, default_llm_provider_config_has_fast_mode_disabled(), default_max_results(), falls_back_to_last_remaining_provider_when_removing_first(), falls_back_to_previous_provider_after_removal() (+11 more)

### Community 12 - "Ai Polish Service"
Cohesion: 0.12
Nodes (30): ai_polish_transport_label(), ai_polish_transport_plan(), build_polish_user_input(), build_system_prompt(), build_user_content(), CorrectionItem, edit_text(), emit_polish_status() (+22 more)

### Community 13 - "Release Packaging"
Cohesion: 0.1
Nodes (26): emit_rerun_hints(), create_tar_xz(), create_tar_xz_with_7z(), create_tar_xz_with_python(), find_7z_executable(), get_size_mb(), main(), 删除目录；在 Windows 文件句柄尚未释放时做有限重试。 (+18 more)

### Community 14 - "Funasr"
Cohesion: 0.11
Nodes (9): active_online_keyring_user(), copy_dir_recursive(), migrate_model_dirs(), online_status_payload(), reload_online_asr_key(), set_engine(), set_models_dir(), set_online_asr_api_key() (+1 more)

### Community 15 - "Profile"
Cohesion: 0.1
Nodes (9): Real-time translation, extract_corrections_via_llm(), parse_correction_pairs(), parse_invalid_indices(), run_correction_validation(), set_translation_target(), submit_user_correction(), update_validation_timestamp() (+1 more)

### Community 16 - "Settings Page"
Cohesion: 0.13
Nodes (7): findLlmPreset(), handleEngineSwitch(), isBuiltinCustomPreset(), isFixedPresetProvider(), isOnlineEngineKey(), resolveLlmBaseUrl(), resolveLlmModel()

### Community 17 - "Alibaba ASR Service"
Cohesion: 0.15
Nodes (15): b64(), DashScopeAsrChoice, DashScopeAsrContent, DashScopeAsrContentField, DashScopeAsrMessage, DashScopeAsrOutput, DashScopeAsrResponse, exceeds_dashscope_limit() (+7 more)

### Community 18 - "Foreground Context"
Cohesion: 0.2
Nodes (11): ForegroundApp, format_prompt_context(), get_foreground_app(), get_process_name(), get_window_title(), normalize_whitespace(), preserves_xml_sensitive_characters_in_prompt_context(), prompt_context_block() (+3 more)

### Community 19 - "Window"
Cohesion: 0.27
Nodes (14): Subtitle overlay, SubtitleOverlay(), apply_subtitle_layout(), create_subtitle_window(), find_cursor_monitor(), force_window_topmost(), hide_main_window(), hide_subtitle_window() (+6 more)

### Community 20 - "Test Download Models Regression"
Cohesion: 0.15
Nodes (4): FakeCompleteResponse, FakeRangeNotSatisfiableResponse, FakeStreamingResponse, ModelDownloadAtomicityTests

### Community 21 - "Audio"
Cohesion: 0.21
Nodes (6): clear_pending_recording_if_current(), start_recording(), start_recording_inner(), stop_microphone_level_monitor(), stop_recording(), stop_recording_inner()

### Community 22 - "Clipboard"
Cohesion: 0.27
Nodes (10): copy_to_clipboard(), grab_selected_text(), grab_selected_text_uia(), make_key_input(), paste_text(), paste_text_impl(), release_stuck_modifiers(), send_inputs() (+2 more)

### Community 23 - "Rebuild Graphify"
Cohesion: 0.3
Nodes (10): build_ast_extraction(), build_labels(), build_outputs(), build_semantic_layer(), detect_corpus(), main(), make_edge(), make_hyperedge() (+2 more)

### Community 24 - "Download Models"
Cohesion: 0.35
Nodes (10): _candidate_endpoints(), _cleanup_locks(), _download_file(), download_model(), _emit(), _get_repo_info(), main(), 清理残留的 .lock 和 .incomplete 文件 (+2 more)

### Community 25 - "Lib"
Cohesion: 0.38
Nodes (10): focus_main_window(), hide_main_window(), mark_setup_once(), run(), setup_system_tray(), spawn_funasr_startup(), spawn_profile_maintenance(), spawn_subtitle_prewarm() (+2 more)

### Community 26 - "Ai Polish"
Cohesion: 0.24
Nodes (7): AiModelInfo, AiModelListPayload, anthropic_models(), codex_oauth_models(), list_ai_models(), set_ai_polish_config(), set_assistant_api_key()

### Community 27 - "HF Cache Utils"
Cohesion: 0.29
Nodes (9): cleanup_incomplete_files(), get_hf_cache_root(), is_hf_repo_ready(), HuggingFace 缓存检测共享工具模块  供 funasr_server.py 和 download_models.py 共同使用， 避免重复实现缓, 返回 HuggingFace 缓存根目录      优先级：HF_HUB_CACHE（由 Rust 设置的自定义路径）> HF_HOME/hub > 默认, 检查 HuggingFace 模型是否已缓存且包含实际模型权重文件。      仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshot, 删除某个 repo 缓存目录下残留的 .incomplete 文件，返回删除数量。, _snapshot_matches_completion_manifest() (+1 more)

### Community 28 - "Updater"
Cohesion: 0.33
Nodes (9): AppUpdateInfo, check_app_update(), fetch_latest_release(), GitHubRelease, is_version_newer(), normalize_version(), open_app_release_page(), open_external_url() (+1 more)

### Community 29 - "Download Service"
Cohesion: 0.24
Nodes (4): clear_download_task(), DownloadLine, emit_download_status(), run_download()

### Community 30 - "Capture"
Cohesion: 0.33
Nodes (8): compute_waveform_bars(), load_best_input_config(), mix_to_mono_f32(), mix_to_mono_i16(), mix_to_mono_u16(), resolve_input_device(), spawn_audio_capture_thread(), spawn_waveform_emitter()

### Community 31 - "Finalize"
Cohesion: 0.42
Nodes (8): do_final_asr(), do_paste(), emit_done(), emit_error(), emit_recording_state_if_current(), finalize_recording(), flush_pending_paste(), schedule_hide()

### Community 32 - "Hotkey Normalization"
Cohesion: 0.36
Nodes (7): collectModifiers(), eventMainKey(), isModifierOnlyCombo(), keyboardEventToHotkey(), modifierFromKeyboardEvent(), normalizeHotkey(), normalizeMainKeyToken()

### Community 33 - "Openai Fast Mode OAuth Tests"
Cohesion: 0.39
Nodes (8): build_auth_headers_unwraps_oauth_derived_openai_api_key(), CapturedRequest, find_subsequence(), openai_responses_endpoint(), parse_header_value(), send_llm_request_injects_priority_and_unwraps_oauth_derived_openai_api_key(), spawn_request_capture_server(), wrapped_oauth_derived_api_key()

### Community 34 - "Assistant Commands"
Cohesion: 0.36
Nodes (3): get_web_search_api_key(), set_web_search_api_key(), web_search_keyring_user()

### Community 35 - "Monitor"
Cohesion: 0.43
Nodes (6): mono_peak_f32(), mono_peak_i16(), mono_peak_u16(), peak_to_meter(), start_microphone_level_monitor(), stop_microphone_level_monitor()

### Community 36 - "Sound"
Cohesion: 0.54
Nodes (7): generate_double_tone(), generate_tone(), play_assistant_start_sound(), play_assistant_stop_sound(), play_start_sound(), play_stop_sound(), play_wav_async()

### Community 37 - "Engine"
Cohesion: 0.53
Nodes (5): cmd_download(), cmd_serve(), main(), PyInstaller frozen 环境下，将 _internal/ 加入 sys.path, _setup_frozen_paths()

### Community 38 - "Status Indicator UI"
Cohesion: 0.4
Nodes (0): 

### Community 39 - "Model Status Hook"
Cohesion: 0.4
Nodes (0): 

### Community 40 - "Theme Hook"
Cohesion: 0.5
Nodes (2): getSystemPrefersDark(), resolveIsDark()

### Community 41 - "Resample"
Cohesion: 0.6
Nodes (3): f32_to_i16(), invalid_sample_rate_is_not_reported_as_successful_16k_audio(), resample_to_16k()

### Community 42 - "Frontend Entry"
Cohesion: 0.5
Nodes (0): 

### Community 43 - "Hotkey Storage Hook"
Cohesion: 0.5
Nodes (0): 

### Community 44 - "Use Smooth Text"
Cohesion: 0.67
Nodes (2): segmentGraphemes(), useSmoothText()

### Community 45 - "Codex OAuth Commands"
Cohesion: 0.5
Nodes (0): 

### Community 46 - "Ai Polish Transport Retry Tests"
Cohesion: 0.83
Nodes (3): ai_polish_transport_plan_uses_nostream_json_before_stream_nojson_without_partial_pref(), ai_polish_transport_plan_uses_stream_nojson_before_nostream_json_with_partial_pref(), assert_plan_stage()

### Community 47 - "Wav"
Cohesion: 0.83
Nodes (3): encode_wav(), test_encode_wav_handles_empty_samples(), test_encode_wav_returns_ok_for_normal_samples()

### Community 48 - "App Errors"
Cohesion: 0.5
Nodes (1): AppError

### Community 49 - "Promo App Window"
Cohesion: 0.67
Nodes (0): 

### Community 50 - "Recording Context"
Cohesion: 0.67
Nodes (0): 

### Community 51 - "Use Recording"
Cohesion: 1.0
Nodes (2): useRecording(), useTauriEvent()

### Community 52 - "Local Storage Helpers"
Cohesion: 0.67
Nodes (0): 

### Community 53 - "Interim"
Cohesion: 1.0
Nodes (2): adjust_interval(), spawn_interim_loop()

### Community 54 - "Audio Service Module"
Cohesion: 0.67
Nodes (2): InputDeviceInfo, InputDeviceListPayload

### Community 55 - "Promo Root"
Cohesion: 1.0
Nodes (0): 

### Community 56 - "Promo Cursor"
Cohesion: 1.0
Nodes (0): 

### Community 57 - "Title Bar"
Cohesion: 1.0
Nodes (0): 

### Community 58 - "Recording Context.test"
Cohesion: 1.0
Nodes (0): 

### Community 59 - "Debounced Callback Hook"
Cohesion: 1.0
Nodes (0): 

### Community 60 - "Exclusive Picker Hook"
Cohesion: 1.0
Nodes (0): 

### Community 61 - "Hotkey Capture Hook"
Cohesion: 1.0
Nodes (0): 

### Community 62 - "Use Recording.test"
Cohesion: 1.0
Nodes (0): 

### Community 63 - "Fast Mode"
Cohesion: 1.0
Nodes (0): 

### Community 64 - "Tauri Event Mock"
Cohesion: 1.0
Nodes (0): 

### Community 65 - "Vite Config"
Cohesion: 1.0
Nodes (0): 

### Community 66 - "Vitest.config"
Cohesion: 1.0
Nodes (0): 

### Community 67 - "Remotion Config"
Cohesion: 1.0
Nodes (0): 

### Community 68 - "Promo Src Index"
Cohesion: 1.0
Nodes (0): 

### Community 69 - "Promo Showcase Video"
Cohesion: 1.0
Nodes (0): 

### Community 70 - "Promo Theme"
Cohesion: 1.0
Nodes (0): 

### Community 71 - "Promo Cinematic Scene"
Cohesion: 1.0
Nodes (0): 

### Community 72 - "Promo Mic Button"
Cohesion: 1.0
Nodes (0): 

### Community 73 - "Promo Result Card"
Cohesion: 1.0
Nodes (0): 

### Community 74 - "Promo Subtitle Capsule"
Cohesion: 1.0
Nodes (0): 

### Community 75 - "Promo Assistant Scene"
Cohesion: 1.0
Nodes (0): 

### Community 76 - "Promo Dictation Scene"
Cohesion: 1.0
Nodes (0): 

### Community 77 - "Promo Edit Scene"
Cohesion: 1.0
Nodes (0): 

### Community 78 - "Promo Intro Scene"
Cohesion: 1.0
Nodes (0): 

### Community 79 - "Promo Outro Scene"
Cohesion: 1.0
Nodes (0): 

### Community 80 - "Promo Translation Scene"
Cohesion: 1.0
Nodes (0): 

### Community 81 - "Vite Env Types"
Cohesion: 1.0
Nodes (0): 

### Community 82 - "Tauri.fast Mode.test"
Cohesion: 1.0
Nodes (0): 

### Community 83 - "Kbd"
Cohesion: 1.0
Nodes (0): 

### Community 84 - "Recording Button UI"
Cohesion: 1.0
Nodes (0): 

### Community 85 - "Transcription History UI"
Cohesion: 1.0
Nodes (0): 

### Community 86 - "Transcription Result UI"
Cohesion: 1.0
Nodes (0): 

### Community 87 - "English I18n"
Cohesion: 1.0
Nodes (0): 

### Community 88 - "I18n Index"
Cohesion: 1.0
Nodes (0): 

### Community 89 - "Chinese I18n"
Cohesion: 1.0
Nodes (0): 

### Community 90 - "App Constants"
Cohesion: 1.0
Nodes (0): 

### Community 91 - "Fast Mode.test"
Cohesion: 1.0
Nodes (0): 

### Community 92 - "Main Page"
Cohesion: 1.0
Nodes (0): 

### Community 93 - "Setup"
Cohesion: 1.0
Nodes (0): 

### Community 94 - "Types Index"
Cohesion: 1.0
Nodes (0): 

### Community 95 - "Commands Module"
Cohesion: 1.0
Nodes (0): 

### Community 96 - "Services Module"
Cohesion: 1.0
Nodes (0): 

### Community 97 - "State Module"
Cohesion: 1.0
Nodes (0): 

### Community 98 - "Utils Module"
Cohesion: 1.0
Nodes (0): 

## Knowledge Gaps
- **103 isolated node(s):** `删除目录；在 Windows 文件句柄尚未释放时做有限重试。`, `删除可安全裁剪的 CUDA DLL，返回节省的 MB 数`, `删除运行时不需要的链接/调试产物，返回节省的 MB 数。`, `校验 torch_cuda.dll 的直接 CUDA 依赖仍然存在。`, `使用 Python 标准库压缩为 tar.xz，返回压缩包大小 MB。` (+98 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Promo Root`** (2 nodes): `Root.tsx`, `RemotionRoot()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Cursor`** (2 nodes): `BlinkingCursor()`, `Cursor.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Title Bar`** (2 nodes): `TitleBar.tsx`, `startDrag()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Recording Context.test`** (2 nodes): `RecordingContext.test.tsx`, `flushPromises()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Debounced Callback Hook`** (2 nodes): `useDebouncedCallback.ts`, `useDebouncedCallback()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Exclusive Picker Hook`** (2 nodes): `useExclusivePicker.ts`, `useExclusivePicker()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Hotkey Capture Hook`** (2 nodes): `useHotkeyCapture.ts`, `useHotkeyCapture()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Use Recording.test`** (2 nodes): `useRecording.test.tsx`, `flushMicrotasks()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Fast Mode`** (2 nodes): `fastMode.ts`, `shouldShowFastModeToggle()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Tauri Event Mock`** (2 nodes): `tauriEventMock.ts`, `createTauriEventController()`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Vite Config`** (1 nodes): `vite.config.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Vitest.config`** (1 nodes): `vitest.config.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Remotion Config`** (1 nodes): `remotion.config.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Src Index`** (1 nodes): `index.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Showcase Video`** (1 nodes): `ShowcaseVideo.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Theme`** (1 nodes): `theme.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Cinematic Scene`** (1 nodes): `Cinematic.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Mic Button`** (1 nodes): `MicButton.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Result Card`** (1 nodes): `ResultCard.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Subtitle Capsule`** (1 nodes): `SubtitleCapsule.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Assistant Scene`** (1 nodes): `Assistant.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Dictation Scene`** (1 nodes): `Dictation.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Edit Scene`** (1 nodes): `EditMode.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Intro Scene`** (1 nodes): `Intro.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Outro Scene`** (1 nodes): `Outro.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Promo Translation Scene`** (1 nodes): `Translation.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Vite Env Types`** (1 nodes): `vite-env.d.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Tauri.fast Mode.test`** (1 nodes): `tauri.fastMode.test.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Kbd`** (1 nodes): `Kbd.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Recording Button UI`** (1 nodes): `RecordingButton.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Transcription History UI`** (1 nodes): `TranscriptionHistory.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Transcription Result UI`** (1 nodes): `TranscriptionResult.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `English I18n`** (1 nodes): `en.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `I18n Index`** (1 nodes): `index.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Chinese I18n`** (1 nodes): `zh.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `App Constants`** (1 nodes): `constants.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Fast Mode.test`** (1 nodes): `fastMode.test.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Main Page`** (1 nodes): `MainPage.tsx`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Setup`** (1 nodes): `setup.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Types Index`** (1 nodes): `index.ts`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Commands Module`** (1 nodes): `mod.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Services Module`** (1 nodes): `mod.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `State Module`** (1 nodes): `mod.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.
- **Thin community `Utils Module`** (1 nodes): `mod.rs`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Light-Whisper overview` connect `Web Search Service` to `Profile Service`, `Window`, `Profile`?**
  _High betweenness centrality (0.134) - this node is a cross-community bridge._
- **Why does `LLM integration layer` connect `Web Search Service` to `LLM Provider`, `LLM Client`?**
  _High betweenness centrality (0.096) - this node is a cross-community bridge._
- **Why does `polish_text()` connect `Ai Polish Service` to `Release Packaging`, `Web Search Service`?**
  _High betweenness centrality (0.070) - this node is a cross-community bridge._
- **Are the 9 inferred relationships involving `BaseASRServer` (e.g. with `FunASRServer` and `Skip FunASR's model-side pip auto-install in bundled runtime.      Some FunASR`) actually correct?**
  _`BaseASRServer` has 9 INFERRED edges - model-reasoned connections that need verification._
- **What connects `删除目录；在 Windows 文件句柄尚未释放时做有限重试。`, `删除可安全裁剪的 CUDA DLL，返回节省的 MB 数`, `删除运行时不需要的链接/调试产物，返回节省的 MB 数。` to the rest of the system?**
  _103 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `Hotkey` be split into smaller, more focused modules?**
  _Cohesion score 0.07 - nodes in this community are weakly interconnected._
- **Should `LLM Provider` be split into smaller, more focused modules?**
  _Cohesion score 0.05 - nodes in this community are weakly interconnected._