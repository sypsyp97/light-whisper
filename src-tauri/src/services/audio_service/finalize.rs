use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager};

use super::resample::ResamplerState;
use super::wav::encode_wav;
use super::{
    EDIT_GRAB_WAIT_MS, EMPTY_RESULT_HIDE_DELAY_MS, INTERIM_MAX_AUDIO_WINDOW_SEC,
    MIN_AUDIO_DURATION_SEC, PASTE_DELAY_MS, RESULT_HIDE_DELAY_MS, TARGET_SAMPLE_RATE,
};
use crate::services::{
    ai_polish_service, alibaba_asr_service, assistant_service, funasr_service, glm_asr_service,
    history_service,
};
use crate::state::user_profile::{ResolvedAppProfile, UserProfile};
use crate::state::{
    AppState, DictationOutputMode, RecordingMode, RecordingOutcomeKind, RecordingPhase,
    RecordingSession, RecordingSnapshot,
};
use crate::utils::foreground::ForegroundApp;
use crate::utils::paths;

const ASSISTANT_PIPELINE_TIMEOUT_SECS: u64 = 180;

pub(super) fn history_audio_snapshot(
    samples: &parking_lot::Mutex<Vec<i16>>,
    enabled: bool,
) -> Option<Vec<i16>> {
    enabled.then(|| samples.lock().clone())
}

fn can_reuse_interim(
    cache: &crate::state::InterimCache,
    final_count: usize,
    sample_rate: u32,
) -> bool {
    let max_window_samples = (sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
    final_count > 0
        && final_count <= max_window_samples
        && cache.sample_count == final_count
        && !cache.text.trim().is_empty()
}

// ---------- 最终转写 + 粘贴 ----------

#[derive(Clone, Copy)]
enum EditGrabStatus {
    Ok,
    Timeout,
    Empty,
    Unsupported,
}

impl EditGrabStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Timeout => "timeout",
            Self::Empty => "empty",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionTiming {
    #[serde(skip_serializing_if = "Option::is_none")]
    asr_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    polish_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_ms: Option<u64>,
}

struct HistorySessionContext {
    enabled: bool,
    retention_days: u32,
    session_id: u64,
    mode: RecordingMode,
    workflow: String,
    duration_sec: f64,
    engine: String,
    app_process: Option<String>,
    app_window_title: Option<String>,
    app_rule_name: Option<String>,
    audio_samples: Option<Vec<i16>>,
    sample_rate: u32,
}

impl HistorySessionContext {
    #[allow(clippy::too_many_arguments)]
    async fn persist(
        self,
        app_handle: &tauri::AppHandle,
        status: &str,
        text: &str,
        original_text: &str,
        source_text: Option<&str>,
        language: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
        timing: Option<TranscriptionTiming>,
        error: Option<&str>,
        receipt: Option<tokio::sync::oneshot::Receiver<Option<u64>>>,
    ) {
        let session_id = self.session_id;
        let persist = async move {
            if !self.enabled {
                return Ok(None);
            }
            let audio_file = if let Some(samples) = self.audio_samples {
                let sample_rate = self.sample_rate;
                let encoded =
                    tokio::task::spawn_blocking(move || encode_wav(&samples, sample_rate)).await;
                match encoded {
                    Ok(Ok(wav)) => match history_service::save_audio(session_id, wav).await {
                        Ok(file_name) => Some(file_name),
                        Err(error) => {
                            log::warn!("保存历史音频失败，继续仅保存文本: {error}");
                            None
                        }
                    },
                    Ok(Err(error)) => {
                        log::warn!("编码历史音频失败，继续仅保存文本: {error}");
                        None
                    }
                    Err(error) => {
                        log::warn!("编码历史音频任务异常，继续仅保存文本: {error}");
                        None
                    }
                }
            } else {
                None
            };
            crate::commands::history::persist_history_insert(
                app_handle,
                history_service::HistoryDraft {
                    session_id,
                    mode: self.mode.as_str().to_string(),
                    workflow: self.workflow,
                    status: status.to_string(),
                    text: text.to_string(),
                    original_text: original_text.to_string(),
                    source_text: source_text.map(str::to_string),
                    duration_sec: Some(self.duration_sec),
                    language: language.map(str::to_string),
                    engine: self.engine,
                    provider: provider.map(str::to_string),
                    model: model.map(str::to_string),
                    app_process: self.app_process,
                    app_window_title: self.app_window_title,
                    app_rule_name: self.app_rule_name,
                    audio_file,
                    asr_ms: timing.and_then(|value| value.asr_ms),
                    polish_ms: timing.and_then(|value| value.polish_ms),
                    total_ms: timing.and_then(|value| value.total_ms),
                    error: error.map(str::to_string),
                    reprocessed_from_id: None,
                },
                self.retention_days,
            )
            .await
            .map(Some)
            .inspect_err(|error| log::warn!("保存转写历史失败: {error}"))
        };
        let _ = super::output::settle_history_output(
            persist,
            receipt,
            |total_ms| {
                let _ = app_handle.emit(
                    "transcription-timing",
                    serde_json::json!({ "sessionId": session_id, "totalMs": total_ms }),
                );
            },
            |id, total_ms| async move {
                let updated = history_service::update_total_ms(id, total_ms)
                    .await
                    .inspect_err(|error| log::warn!("更新历史总耗时失败: {error}"))?;
                if updated {
                    let _ = app_handle.emit("history-updated", serde_json::json!({ "id": id }));
                }
                Ok(updated)
            },
        )
        .await;
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn screen_context_allowed(
    requested: bool,
    captured: Option<&ForegroundApp>,
    current: Option<&ForegroundApp>,
) -> bool {
    requested && captured.is_some() && captured == current
}

fn resolve_recording_app_profile(
    profile: &UserProfile,
    foreground_app: Option<&ForegroundApp>,
) -> ResolvedAppProfile {
    match foreground_app {
        Some(app) if !app.process_name.trim().is_empty() => {
            profile.resolve_app_profile(&app.process_name, &app.window_title)
        }
        // 无法确认进程身份时，不能确定用户是否为该应用配置了隐私规则。
        // 仅关闭会捕获或持久化内容的功能；听写和显式助手请求仍可继续。
        _ => ResolvedAppProfile {
            screen_context_enabled: Some(false),
            history_enabled: Some(false),
            ..Default::default()
        },
    }
}

pub async fn finalize_recording(
    app_handle: tauri::AppHandle,
    session: RecordingSession,
    polish_cancel: Option<tokio::sync::oneshot::Receiver<()>>,
) {
    let RecordingSession {
        session_id,
        subtitle_show_gen,
        trigger,
        sample_rate,
        audio_thread,
        interim_task,
        native_recording,
        samples,
        interim_cache,
        foreground_app,
        paste_target,
        edit_grab,
        ..
    } = session;
    let finalize_start = Instant::now();

    if let Some(h) = audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = h.join();
        })
        .await;
    }
    // 等待 interim 任务自然结束；超时则 abort 以释放 funasr_process 锁
    if let Some(t) = interim_task {
        let abort_handle = t.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(5), t)
            .await
            .is_err()
        {
            log::warn!("interim 任务超时 (5s)，强制中止");
            abort_handle.abort();
        }
    }

    // The capture thread is joined: Finish now includes its final samples and
    // the resampler tail. A native result never falls back to whole-clip ASR.
    let native_finish_start = Instant::now();
    let native_result = match native_recording {
        Some(recording) => Some(recording.finish().await.map_err(|error| error.to_string())),
        None => None,
    };
    let native_finish_ms = native_result
        .as_ref()
        .map(|_| elapsed_ms(native_finish_start));

    // 选中文本保留在本地变量里，不写全局。两个 finalize 并发时也彼此隔离：
    // edit_grab 来自各自 session 的 RecordingSession，edit_context 只在本函数
    // 作用域存活，不存在跨会话串位的机会。
    let (edit_context, edit_grab_status): (Option<String>, EditGrabStatus) = match edit_grab {
        Some(handle) => {
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(std::time::Duration::from_millis(EDIT_GRAB_WAIT_MS), handle)
                .await
            {
                Ok(Ok(Some(selected))) => (Some(selected), EditGrabStatus::Ok),
                Ok(Ok(None)) => {
                    log::debug!("选中文本抓取完成：当前没有选中文本");
                    (None, EditGrabStatus::Empty)
                }
                Ok(Err(join_err)) => {
                    log::debug!("选中文本抓取任务 join 失败: {}", join_err);
                    (None, EditGrabStatus::Unsupported)
                }
                Err(_) => {
                    log::warn!(
                        "选中文本抓取超过 {}ms，按普通听写处理 (session {})",
                        EDIT_GRAB_WAIT_MS,
                        session_id
                    );
                    abort_handle.abort();
                    (None, EditGrabStatus::Timeout)
                }
            }
        }
        None => {
            log::debug!("当前会话没有选中文本抓取任务");
            (None, EditGrabStatus::Unsupported)
        }
    };

    let state = app_handle.state::<AppState>();
    let app_profile = state
        .with_profile(|profile| resolve_recording_app_profile(profile, foreground_app.as_ref()));
    if foreground_app
        .as_ref()
        .is_none_or(|app| app.process_name.trim().is_empty())
    {
        log::warn!("无法确认录音目标进程，已禁用本次屏幕上下文和历史保存");
    }
    let history_settings = state.with_profile(|profile| profile.history_settings.clone());
    let history_enabled = app_profile
        .history_enabled
        .unwrap_or(history_settings.enabled);
    let app_context = foreground_app.as_ref().and_then(|app| {
        crate::utils::foreground::prompt_context_from_parts(&app.process_name, &app.window_title)
    });
    let history_engine = paths::read_engine_config();

    let final_count = samples.lock().len();
    let cached = interim_cache.lock().clone();
    let duration_sec = final_count as f64 / sample_rate as f64;
    let mode = trigger.mode();

    if duration_sec < MIN_AUDIO_DURATION_SEC && native_result.is_none() {
        log::info!("录音时间过短 ({:.2}s)，跳过转写", duration_sec);
        emit_terminal_outcome(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            RecordingOutcomeKind::TooShort,
            None,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    let history_workflow = if mode == RecordingMode::Assistant {
        "assistant"
    } else if edit_context.is_some() {
        "edit"
    } else {
        "dictation"
    };
    let history = HistorySessionContext {
        enabled: history_enabled,
        retention_days: history_settings.retention_days,
        session_id,
        mode,
        workflow: history_workflow.to_string(),
        duration_sec,
        engine: history_engine.clone(),
        app_process: foreground_app.as_ref().map(|app| app.process_name.clone()),
        app_window_title: foreground_app.as_ref().map(|app| app.window_title.clone()),
        app_rule_name: app_profile.rule_name.clone(),
        audio_samples: history_audio_snapshot(
            &samples,
            history_enabled && history_settings.save_audio,
        ),
        sample_rate,
    };

    // 优先复用 interim 缓存，否则重新 ASR。
    //
    // 复用条件（全部成立才复用，否则重跑 do_final_asr）：
    //   1. 录音必须完整落在 interim 窗口内 (final_count <= 12s * sample_rate)。
    //      interim 缓存的是"最后 12 秒"的转写，用它顶替更长的 final 会直接丢
    //      录音开头那段的文本（比率 >0.9 也一样丢，只是用户感知为"前几个字没了"）。
    //   2. 缓存必须覆盖每一个最终样本，任何尾部间隙都需要重新识别。
    //      未处理的尾部可能包含语音，不能仅凭间隙时长判定为静音。
    //   3. interim 确实返回了非空文本。
    let asr_start = Instant::now();
    let (asr_text, detected_lang): (Result<String, String>, Option<String>) =
        if let Some(result) = native_result {
            match result {
                Ok(result) => (Ok(result.text), result.language),
                Err(error) => (Err(error), None),
            }
        } else {
            match cached {
                Some(ref c) if can_reuse_interim(c, final_count, sample_rate) => {
                    log::info!(
                        "复用 interim 缓存 (尾部间隙 {:.0}ms)",
                        (final_count - c.sample_count) as f64 * 1000.0 / sample_rate as f64
                    );
                    (Ok(c.text.clone()), c.language.clone())
                }
                _ => match do_final_asr(&app_handle, state.inner(), &samples, sample_rate).await {
                    Ok(r) => (Ok(r.text), r.language),
                    Err(e) => (Err(e), None),
                },
            }
        };

    let asr_elapsed_ms = native_finish_ms.unwrap_or_else(|| elapsed_ms(asr_start));
    let text = match asr_text {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            let timing = TranscriptionTiming {
                asr_ms: Some(asr_elapsed_ms),
                polish_ms: None,
                total_ms: Some(elapsed_ms(finalize_start)),
            };
            emit_error(
                &app_handle,
                session_id,
                subtitle_show_gen,
                mode,
                RecordingOutcomeKind::AsrError,
                &e,
            );
            flush_pending_paste(&app_handle);
            history
                .persist(
                    &app_handle,
                    "asr_error",
                    "",
                    "",
                    None,
                    None,
                    None,
                    None,
                    Some(timing),
                    Some(&e),
                    None,
                )
                .await;
            return;
        }
    };

    let lang_ref = detected_lang.as_deref();

    if text.is_empty() {
        let timing = TranscriptionTiming {
            asr_ms: Some(asr_elapsed_ms),
            polish_ms: None,
            total_ms: Some(elapsed_ms(finalize_start)),
        };
        emit_terminal_outcome(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            RecordingOutcomeKind::NoSpeech,
            None,
        );
        flush_pending_paste(&app_handle);
        history
            .persist(
                &app_handle,
                "no_speech",
                "",
                "",
                None,
                lang_ref,
                None,
                None,
                Some(timing),
                Some("未检测到语音"),
                None,
            )
            .await;
        return;
    }

    // 先发布结果并排队输出；历史音频编码、写盘和清理不阻塞用户结果。
    if mode == RecordingMode::Dictation && edit_context.is_some() {
        let selected_text = edit_context.unwrap_or_default();
        let edit_started = Instant::now();
        // 编辑模式：ASR 结果是语音指令，用它改写选中文本
        log::info!(
            "编辑模式：指令{}字符，选中文本{}字符",
            text.chars().count(),
            selected_text.chars().count()
        );
        match ai_polish_service::run_cancellable_polish(
            ai_polish_service::edit_text(
                state.inner(),
                &selected_text,
                &text,
                &app_handle,
                session_id,
            ),
            polish_cancel,
        )
        .await
        {
            Ok(outcome) => {
                let result = outcome.text;
                let edit_elapsed_ms = elapsed_ms(edit_started);
                let timing = TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: Some(edit_elapsed_ms),
                    total_ms: Some(elapsed_ms(finalize_start)),
                };
                emit_done_with_timing(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    &result,
                    &result,
                    duration_sec,
                    true,
                    lang_ref,
                    edit_grab_status,
                    Some(timing),
                );
                let receipt = if !result.is_empty() {
                    Some(do_paste(
                        &app_handle,
                        &result,
                        session_id,
                        paste_target,
                        finalize_start,
                    ))
                } else {
                    flush_pending_paste(&app_handle);
                    None
                };
                history
                    .persist(
                        &app_handle,
                        "success",
                        &result,
                        &text,
                        Some(&selected_text),
                        lang_ref,
                        Some(&outcome.provider),
                        Some(&outcome.model),
                        Some(timing),
                        None,
                        receipt,
                    )
                    .await;
            }
            Err(e) => {
                let cancelled = e == ai_polish_service::POLISH_CANCELLED_ERROR;
                if cancelled {
                    log::info!("已取消选区编辑，保留原文 (session {})", session_id);
                } else {
                    log::warn!("编辑选中文本失败，不替换原文: {}", e);
                }
                let edit_elapsed_ms = elapsed_ms(edit_started);
                let timing = TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: Some(edit_elapsed_ms),
                    total_ms: Some(elapsed_ms(finalize_start)),
                };
                if cancelled {
                    emit_done_with_timing(
                        &app_handle,
                        session_id,
                        subtitle_show_gen,
                        mode,
                        &selected_text,
                        &selected_text,
                        duration_sec,
                        false,
                        lang_ref,
                        edit_grab_status,
                        Some(timing),
                    );
                } else {
                    emit_error(
                        &app_handle,
                        session_id,
                        subtitle_show_gen,
                        mode,
                        RecordingOutcomeKind::ProcessingError,
                        &format!("编辑失败: {}", e),
                    );
                }
                flush_pending_paste(&app_handle);
                history
                    .persist(
                        &app_handle,
                        if cancelled {
                            "success"
                        } else {
                            "processing_error"
                        },
                        if cancelled { &selected_text } else { "" },
                        &text,
                        Some(&selected_text),
                        lang_ref,
                        None,
                        None,
                        Some(timing),
                        (!cancelled).then_some(e.as_str()),
                        None,
                    )
                    .await;
            }
        }
    } else if mode == RecordingMode::Assistant {
        let original_request = text;
        let assistant_started = Instant::now();
        let requested_polish_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let screen_context_explicit = app_profile.screen_context_enabled.is_some();
        let screen_routing_enabled = state.with_profile(|profile| profile.jev.screen_routing);
        let requested_polish_screen_context = if screen_routing_enabled && !screen_context_explicit
        {
            false
        } else {
            requested_polish_screen_context
        };
        let assistant_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let assistant_request_context = assistant_service::AssistantRequestContext::for_recording(
            assistant_screen_context,
            foreground_app.clone(),
            app_context.clone(),
        )
        .with_explicit_screen_context(screen_context_explicit);
        let (assistant_generation, cancel_rx) =
            crate::commands::assistant::begin_assistant_chat_task(state.inner());
        let assistant_pipeline = async {
            // 搜索判断、搜索关键词和最终生成都基于校正后的语音请求。
            // 显式关闭翻译覆盖，避免“翻译输出”设置改变用户实际发给助手的指令。
            let polish_outcome = ai_polish_service::polish_text_with_overrides_detailed(
                state.inner(),
                &original_request,
                &app_handle,
                session_id,
                ai_polish_service::PolishOverrides {
                    ai_polish_enabled: app_profile.ai_polish_enabled,
                    translation_target: Some(None),
                    custom_prompt: app_profile.custom_prompt.clone(),
                    screen_context_enabled: Some(requested_polish_screen_context),
                    screen_context_explicit,
                    screen_context_foreground: foreground_app.clone(),
                    app_context: app_context.clone(),
                    ..Default::default()
                },
            )
            .await;
            let (assistant_request, screen_context_description) = match polish_outcome {
                Ok(outcome) => (outcome.text, outcome.screen_context_description),
                Err(err) => {
                    log::warn!("助手请求预润色失败，使用原始转写: {}", err);
                    (original_request.clone(), None)
                }
            };
            let assistant_request_context = assistant_request_context
                .with_screen_context_description(screen_context_description);

            assistant_service::generate_content_with_context(
                state.inner(),
                &assistant_request,
                edit_context.as_deref(),
                &app_handle,
                session_id,
                assistant_request_context,
            )
            .await
        };
        let timeout_message = format!(
            "助手请求超时（{}秒），请重试",
            ASSISTANT_PIPELINE_TIMEOUT_SECS
        );
        let assistant_result = crate::commands::assistant::run_cancellable_assistant_task(
            assistant_pipeline,
            cancel_rx,
            Duration::from_secs(ASSISTANT_PIPELINE_TIMEOUT_SECS),
            &timeout_message,
            "助手请求已取消",
        )
        .await;
        crate::commands::assistant::clear_assistant_chat_task(state.inner(), assistant_generation);
        let assistant_elapsed_ms = elapsed_ms(assistant_started);

        match assistant_result {
            Ok(outcome) => {
                let result = outcome.text;
                let timing = TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: Some(assistant_elapsed_ms),
                    total_ms: Some(elapsed_ms(finalize_start)),
                };
                emit_done_with_timing(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    &result,
                    &original_request,
                    duration_sec,
                    false,
                    lang_ref,
                    edit_grab_status,
                    Some(timing),
                );
                match crate::commands::window::set_subtitle_window_interactive_for_session(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    true,
                )
                .await
                {
                    Ok(true) => {}
                    Ok(false) => {
                        log::info!("跳过过期助手会话的字幕交互切换 (session {})", session_id)
                    }
                    Err(err) => log::warn!("助手结果显示时切换字幕窗口交互态失败: {}", err),
                }
                history
                    .persist(
                        &app_handle,
                        "success",
                        &result,
                        &original_request,
                        None,
                        lang_ref,
                        Some(&outcome.provider),
                        Some(&outcome.model),
                        Some(timing),
                        None,
                        None,
                    )
                    .await;
            }
            Err(err) => {
                let timing = TranscriptionTiming {
                    asr_ms: Some(asr_elapsed_ms),
                    polish_ms: Some(assistant_elapsed_ms),
                    total_ms: Some(elapsed_ms(finalize_start)),
                };
                emit_error(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    RecordingOutcomeKind::ProcessingError,
                    &err.to_string(),
                );
                flush_pending_paste(&app_handle);
                history
                    .persist(
                        &app_handle,
                        "processing_error",
                        "",
                        &original_request,
                        None,
                        lang_ref,
                        None,
                        None,
                        Some(timing),
                        Some(&err.to_string()),
                        None,
                    )
                    .await;
            }
        }
    } else {
        // 普通听写模式
        let original = text.clone();
        let ai_polish_enabled = app_profile
            .ai_polish_enabled
            .unwrap_or_else(|| state.profile.ai_polish_enabled.load(Ordering::Acquire));
        let translation_override = app_profile
            .translation_target
            .clone()
            .or_else(|| match trigger.dictation_output() {
                DictationOutputMode::Original => Some(None),
                DictationOutputMode::Translated => None,
            });
        let (profile_translation_target, polish_audit_enabled) = state
            .with_profile(|profile| (profile.translation_target.clone(), profile.jev.polish_audit));
        let effective_translation_target = translation_override
            .clone()
            .unwrap_or(profile_translation_target);
        let audit_policy = if ai_polish_enabled
            && polish_audit_enabled
            && effective_translation_target.is_none()
        {
            Some(ai_polish_service::build_jev_audit_policy(
                state.inner(),
                &original,
                translation_override.clone(),
                app_profile.custom_prompt.as_deref(),
                app_context.as_deref(),
            ))
        } else {
            None
        };
        let requested_screen_context = app_profile
            .screen_context_enabled
            .unwrap_or_else(|| state.with_profile(|profile| profile.screen_context_enabled()));
        let current_foreground = requested_screen_context
            .then(crate::utils::foreground::get_foreground_app)
            .flatten();
        let allow_screen_context = screen_context_allowed(
            requested_screen_context,
            foreground_app.as_ref(),
            current_foreground.as_ref(),
        );
        if requested_screen_context && !allow_screen_context {
            log::warn!(
                "AI 润色截图已跳过：前台窗口已离开录音开始时的目标应用 (session {})",
                session_id
            );
        }
        let polish_start = Instant::now();
        let polish_result = ai_polish_service::run_cancellable_polish(
            ai_polish_service::polish_text_with_overrides_detailed(
                state.inner(),
                &text,
                &app_handle,
                session_id,
                ai_polish_service::PolishOverrides {
                    ai_polish_enabled: Some(ai_polish_enabled),
                    allow_jev_gate: true,
                    translation_target: translation_override,
                    custom_prompt: app_profile.custom_prompt.clone(),
                    screen_context_enabled: Some(allow_screen_context),
                    screen_context_explicit: app_profile.screen_context_enabled.is_some(),
                    screen_context_foreground: foreground_app.clone(),
                    app_context: app_context.clone(),
                    ..Default::default()
                },
            ),
            polish_cancel,
        )
        .await;
        let elapsed_polish_ms = elapsed_ms(polish_start);
        let (text, history_provider, history_model, polish_elapsed_ms) = match polish_result {
            Ok(outcome) => {
                let timing = outcome.executed.then_some(elapsed_polish_ms);
                (outcome.text, outcome.provider, outcome.model, timing)
            }
            Err(error) if error == ai_polish_service::POLISH_CANCELLED_ERROR => {
                log::info!("已跳过润色，使用识别原文 (session {})", session_id);
                (original.clone(), None, None, Some(elapsed_polish_ms))
            }
            Err(error) => {
                log::warn!("AI 润色失败，使用原文: {}", error);
                (original.clone(), None, None, None)
            }
        };
        let polished = text != original;
        if polished {
            if let Some(policy) = audit_policy {
                crate::services::jev_review::schedule_polish_audit(
                    &app_handle,
                    state.inner(),
                    session_id,
                    &original,
                    &text,
                    policy,
                );
            }
        }
        let timing = TranscriptionTiming {
            asr_ms: Some(asr_elapsed_ms),
            polish_ms: polish_elapsed_ms,
            total_ms: Some(elapsed_ms(finalize_start)),
        };
        emit_done_with_timing(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            &text,
            &original,
            duration_sec,
            polished,
            lang_ref,
            edit_grab_status,
            Some(timing),
        );

        let receipt = if !text.is_empty() {
            Some(do_paste(
                &app_handle,
                &text,
                session_id,
                paste_target,
                finalize_start,
            ))
        } else {
            flush_pending_paste(&app_handle);
            None
        };
        history
            .persist(
                &app_handle,
                "success",
                &text,
                &original,
                None,
                lang_ref,
                history_provider.as_deref(),
                history_model.as_deref(),
                Some(timing),
                None,
                receipt,
            )
            .await;
    }
}

pub async fn discard_recording(session: RecordingSession) {
    // 中止本会话持有的 grab handle（spawn_blocking 不可抢占，但 abort 会让
    // JoinHandle 提前 detach，结果被丢弃，不会影响后续会话）。
    if let Some(grab) = session.edit_grab {
        grab.abort();
    }
    if let Some(h) = session.audio_thread {
        let _ = tokio::task::spawn_blocking(move || {
            let _ = h.join();
        })
        .await;
    }
    if let Some(t) = session.interim_task {
        let abort_handle = t.abort_handle();
        if tokio::time::timeout(std::time::Duration::from_secs(5), t)
            .await
            .is_err()
        {
            abort_handle.abort();
        }
    }
    if let Some(recording) = session.native_recording {
        if let Err(error) = recording.cancel().await {
            log::warn!("R2T2 录音取消失败: {error}");
        }
    }
    log::info!("已丢弃录音会话 (session {})", session.session_id);
}

async fn do_final_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    samples: &parking_lot::Mutex<Vec<i16>>,
    sample_rate: u32,
) -> Result<funasr_service::TranscriptionResult, String> {
    let data = std::mem::take(&mut *samples.lock());
    let (asr_audio, asr_sample_rate) = match ResamplerState::new(sample_rate)
        .and_then(|mut resampler| resampler.push_i16(&data))
    {
        Ok(output) => (output, TARGET_SAMPLE_RATE),
        Err(err) => {
            log::warn!(
                "最终音频重采样失败，保留原始采样率 {}Hz: {}",
                sample_rate,
                err
            );
            (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
        }
    };

    let engine = paths::read_engine_config();
    let result = if paths::is_online_engine(&engine) {
        let wav =
            encode_wav(&asr_audio, asr_sample_rate).map_err(|e| format!("WAV 编码失败: {}", e))?;
        match engine.as_str() {
            "alibaba-asr" => alibaba_asr_service::transcribe(state, wav).await,
            _ => glm_asr_service::transcribe(state, wav).await,
        }
    } else {
        funasr_service::transcribe_pcm16(state, &asr_audio, asr_sample_rate, app_handle).await
    };

    match result {
        Ok(r) if r.success => Ok(r),
        Ok(r) => Err(r.error.unwrap_or_else(|| "语音识别失败".into())),
        Err(e) => Err(format!("语音识别失败: {}", e)),
    }
}

// ---------- 事件发送 ----------

#[allow(clippy::too_many_arguments)]
fn emit_done_with_timing(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
    edit_grab_status: EditGrabStatus,
    timing: Option<TranscriptionTiming>,
) {
    let delay = if text.is_empty() {
        EMPTY_RESULT_HIDE_DELAY_MS
    } else {
        RESULT_HIDE_DELAY_MS
    };
    let idle = app
        .state::<AppState>()
        .recording
        .transition_snapshot_if_current(sid, RecordingPhase::Idle, mode, None, None);
    if let Some(snapshot) = idle.as_ref() {
        app.state::<AppState>()
            .recording
            .clear_snapshot_if_session(sid);
        emit_recording_state_snapshot(app, snapshot, None);
    }
    emit_transcription_result(
        app,
        sid,
        mode,
        text,
        original_text,
        dur,
        polished,
        language,
        edit_grab_status,
        timing,
    );
    if mode != RecordingMode::Assistant || text.is_empty() {
        crate::commands::window::schedule_subtitle_hide(app, sid, show_gen, mode, delay);
    }
    flush_pending_paste(app);
}

#[allow(clippy::too_many_arguments)]
fn emit_transcription_result(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
    edit_grab_status: EditGrabStatus,
    timing: Option<TranscriptionTiming>,
) {
    let mut payload = serde_json::json!({
        "sessionId": sid, "text": text, "interim": false,
        "durationSec": dur, "charCount": text.chars().count(), "polished": polished,
        "language": language, "mode": mode.as_str(), "originalText": original_text,
        "editGrabStatus": edit_grab_status.as_str(),
    });
    if let Some(timing) = timing {
        payload["timing"] = serde_json::json!(timing);
    }
    let _ = app.emit("transcription-result", payload);
}

fn recording_outcome_payload(
    sid: u64,
    revision: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    detail: Option<&str>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "sessionId": sid,
        "revision": revision,
        "phase": RecordingPhase::Outcome,
        "outcome": outcome,
        "mode": mode.as_str(),
    });
    if let Some(detail) = detail {
        payload["detail"] = serde_json::json!(detail);
    }
    payload
}

fn emit_recording_outcome(app: &tauri::AppHandle, snapshot: &RecordingSnapshot) {
    let Some(outcome) = snapshot.outcome else {
        return;
    };
    let _ = app.emit(
        "recording-outcome",
        recording_outcome_payload(
            snapshot.session_id,
            snapshot.revision,
            snapshot.mode,
            outcome,
            snapshot.detail.as_deref(),
        ),
    );
}

fn emit_terminal_outcome(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    detail: Option<&str>,
) {
    flush_pending_paste(app);
    let snapshot = app
        .state::<AppState>()
        .recording
        .transition_snapshot_if_current(sid, RecordingPhase::Outcome, mode, Some(outcome), detail);
    if let Some(snapshot) = snapshot.as_ref() {
        emit_recording_state_snapshot(app, snapshot, detail);
        emit_recording_outcome(app, snapshot);
        crate::commands::window::schedule_subtitle_hide(
            app,
            sid,
            show_gen,
            mode,
            RESULT_HIDE_DELAY_MS,
        );
    }
}

fn emit_error(
    app: &tauri::AppHandle,
    sid: u64,
    show_gen: u64,
    mode: RecordingMode,
    outcome: RecordingOutcomeKind,
    error: &str,
) {
    emit_terminal_outcome(app, sid, show_gen, mode, outcome, Some(error));
}

fn emit_recording_state_snapshot(
    app: &tauri::AppHandle,
    snapshot: &RecordingSnapshot,
    error: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "sessionId": snapshot.session_id,
        "revision": snapshot.revision,
        "phase": snapshot.phase,
        "isStarting": false,
        "isRecording": false,
        "isProcessing": snapshot.phase == RecordingPhase::Processing,
        "mode": snapshot.mode,
    });
    if let Some(err) = error {
        payload["error"] = serde_json::json!(err);
    }
    let _ = app.emit("recording-state", payload);
}

// ---------- 粘贴逻辑 ----------

pub(crate) fn flush_pending_paste(app: &tauri::AppHandle) {
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
        let state = app.state::<AppState>();
        let recoveries = drain_pending_pastes(
            &state,
            crate::services::selection_service::foreground_window_id,
            |output| {
                let app = app.clone();
                let method = state.ui.input_method.lock().clone();
                async move {
                    crate::commands::clipboard::paste_text_to_target(
                        &app,
                        &output.text,
                        &method,
                        output.target_window,
                    )
                    .await
                    .map(|_| ())
                    .map_err(|error| error.to_string())
                }
            },
        )
        .await;
        for (output, reason) in recoveries {
            let _ = app.emit(
                "paste-recovery",
                serde_json::json!({
                    "sessionId": output.session_id,
                    "text": output.text,
                    "reason": reason,
                }),
            );
        }
    });
}

/// Recording reservation shares this dispatch mutex; native focus and input
/// remain sampled OS operations, and failures are returned for explicit recovery.
pub(super) async fn drain_pending_pastes<F, Fut>(
    state: &AppState,
    current_window: impl Fn() -> Option<isize>,
    paste: F,
) -> Vec<(super::output::PendingPaste, &'static str)>
where
    F: Fn(super::output::PendingPaste) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let _dispatch = state.recording.paste_dispatch.lock().await;
    let outputs = {
        let recording = state.recording.recording.lock();
        state
            .recording
            .pending_paste
            .lock()
            .take_if_idle(recording.is_some())
    };
    let mut recoveries = Vec::new();
    for output in outputs {
        let reason = if !output.target_matches(current_window()) {
            Some(if output.target_window.is_none() {
                "target_unknown"
            } else {
                "target_changed"
            })
        } else if let Err(error) = paste(output.clone()).await {
            log::warn!("会话 {} 自动粘贴失败: {}", output.session_id, error);
            Some(if output.target_matches(current_window()) {
                "paste_failed"
            } else {
                "target_changed"
            })
        } else {
            None
        };
        state
            .recording
            .pending_paste
            .lock()
            .finish_output(output.session_id, reason.is_none());
        if let Some(reason) = reason {
            recoveries.push((output, reason));
        }
    }
    recoveries
}

fn do_paste(
    app: &tauri::AppHandle,
    text: &str,
    session_id: u64,
    target_window: Option<isize>,
    started_at: Instant,
) -> tokio::sync::oneshot::Receiver<Option<u64>> {
    let state = app.state::<AppState>();
    let receipt = {
        let _recording = state.recording.recording.lock();
        state.recording.pending_paste.lock().push_with_completion(
            super::output::PendingPaste {
                session_id,
                target_window,
                text: text.to_string(),
            },
            started_at,
        )
    };
    flush_pending_paste(app);
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interim_reuse_requires_every_final_sample() {
        let mut cache = crate::state::InterimCache {
            text: "unfinished sentence".into(),
            sample_count: 28_800,
            language: Some("zh".into()),
        };
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 31_999;
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 32_000;
        assert!(can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 32_001;
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
        cache.sample_count = 192_001;
        assert!(!can_reuse_interim(&cache, 192_001, 16_000));
        cache.sample_count = 0;
        assert!(!can_reuse_interim(&cache, 0, 16_000));
        cache.sample_count = 32_000;
        cache.text = " \n".into();
        assert!(!can_reuse_interim(&cache, 32_000, 16_000));
    }

    fn foreground(process_name: &str, window_title: &str) -> ForegroundApp {
        ForegroundApp {
            process_name: process_name.into(),
            window_title: window_title.into(),
        }
    }

    #[test]
    fn screen_context_requires_the_original_foreground_window() {
        let captured = foreground("Code.exe", "README - Code");
        let same = captured.clone();
        let switched = foreground("chrome.exe", "Inbox");

        assert!(screen_context_allowed(true, Some(&captured), Some(&same)));
        assert!(!screen_context_allowed(
            true,
            Some(&captured),
            Some(&switched)
        ));
        assert!(!screen_context_allowed(true, None, Some(&same)));
        assert!(!screen_context_allowed(false, Some(&captured), Some(&same)));
    }

    #[test]
    fn unidentified_foreground_disables_capture_and_history() {
        let profile = UserProfile::default();
        let missing_process = foreground("", "Protected document");

        for resolved in [
            resolve_recording_app_profile(&profile, None),
            resolve_recording_app_profile(&profile, Some(&missing_process)),
        ] {
            assert_eq!(resolved.screen_context_enabled, Some(false));
            assert_eq!(resolved.history_enabled, Some(false));
            assert_eq!(resolved.ai_polish_enabled, None);
        }
    }

    #[test]
    fn transcription_timing_serializes_as_frontend_camel_case_payload() {
        let value = serde_json::to_value(TranscriptionTiming {
            asr_ms: Some(42),
            polish_ms: Some(900),
            total_ms: Some(948),
        })
        .expect("timing should serialize");

        assert_eq!(
            value,
            serde_json::json!({
                "asrMs": 42,
                "polishMs": 900,
                "totalMs": 948
            })
        );
    }

    #[test]
    fn dictation_pastes_once_after_polish_completes() {
        let source = include_str!("finalize.rs");
        let dictation_branch = source
            .split("// 普通听写模式")
            .nth(1)
            .and_then(|source| source.split("pub async fn discard_recording").next())
            .expect("normal dictation branch should exist");
        let normalized: String = dictation_branch
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        let polish_completed = normalized
            .find("letpolished=text!=original;")
            .expect("dictation should resolve AI polish before pasting");
        let paste = normalized
            .find("do_paste(")
            .expect("dictation should paste the final text");
        let (_, paste_arguments) = normalized[paste..]
            .split_once(',')
            .expect("paste should receive an app handle before its output arguments");
        assert!(
            paste_arguments.starts_with("&text,session_id,paste_target,"),
            "paste must use the final text, its recording session, and its captured target"
        );

        assert!(
            polish_completed < paste,
            "paste must happen after AI polish resolves"
        );
        assert_eq!(
            dictation_branch.matches("do_paste").count(),
            1,
            "normal dictation must have exactly one paste path"
        );
    }

    #[test]
    fn recording_outcome_payload_is_session_scoped_and_frontend_ready() {
        assert_eq!(
            recording_outcome_payload(
                42,
                7,
                RecordingMode::Dictation,
                RecordingOutcomeKind::TooShort,
                None,
            ),
            serde_json::json!({
                "sessionId": 42,
                "revision": 7,
                "phase": "outcome",
                "outcome": "too_short",
                "mode": "dictation",
            })
        );

        assert_eq!(
            recording_outcome_payload(
                43,
                8,
                RecordingMode::Assistant,
                RecordingOutcomeKind::ProcessingError,
                Some("provider failed"),
            ),
            serde_json::json!({
                "sessionId": 43,
                "revision": 8,
                "phase": "outcome",
                "outcome": "processing_error",
                "mode": "assistant",
                "detail": "provider failed",
            })
        );
    }
}
