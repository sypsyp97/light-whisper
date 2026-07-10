use std::sync::atomic::Ordering;
use std::time::Instant;

use serde::Serialize;
use tauri::{Emitter, Manager};

use super::resample::ChunkedResampler;
use super::wav::encode_wav;
use super::{
    EDIT_GRAB_WAIT_MS, EMPTY_RESULT_HIDE_DELAY_MS, INTERIM_MAX_AUDIO_WINDOW_SEC,
    MIN_AUDIO_DURATION_SEC, PASTE_DELAY_MS, RESULT_HIDE_DELAY_MS, TARGET_SAMPLE_RATE,
};
use crate::services::{
    ai_polish_service, alibaba_asr_service, assistant_service, funasr_service, glm_asr_service,
};
use crate::state::{
    AppState, DictationOutputMode, RecordingMode, RecordingOutcomeKind, RecordingPhase,
    RecordingSession, RecordingSnapshot, RecordingTrigger,
};
use crate::utils::{paths, AppError};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_first: Option<RawFirstTiming>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RawFirstTiming {
    status: RawFirstStatus,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum RawFirstStatus {
    PreviewOnly,
    Pasted,
    Replaced,
    KeptRaw,
    FinalFallback,
    Unchanged,
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}

pub async fn finalize_recording(app_handle: tauri::AppHandle, session: RecordingSession) {
    let RecordingSession {
        session_id,
        subtitle_show_gen,
        trigger,
        sample_rate,
        audio_thread,
        interim_task,
        samples,
        interim_cache,
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

    let final_count = samples.lock().len();
    let cached = interim_cache.lock().clone();
    let duration_sec = final_count as f64 / sample_rate as f64;
    let mode = trigger.mode();

    if duration_sec < MIN_AUDIO_DURATION_SEC {
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

    // 优先复用 interim 缓存，否则重新 ASR。
    //
    // 复用条件（全部成立才复用，否则重跑 do_final_asr）：
    //   1. 录音必须完整落在 interim 窗口内 (final_count <= 12s * sample_rate)。
    //      interim 缓存的是"最后 12 秒"的转写，用它顶替更长的 final 会直接丢
    //      录音开头那段的文本（比率 >0.9 也一样丢，只是用户感知为"前几个字没了"）。
    //   2. 尾部间隙 <= 250ms。以前是 "覆盖率 >=90%"，在短录音 / 快语速下可能把
    //      250ms~500ms 的尾部音节丢掉。250ms 绝对阈值比百分比更保守，在长录音上
    //      也不会放宽门槛；最差只会丢掉一次 interim 间隔内的静音/换气。
    //   3. interim 确实返回了非空文本。
    let max_interim_window_samples = (sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
    let tail_gap_threshold_samples = (sample_rate as f64 * 0.25) as usize;
    let asr_start = Instant::now();
    let (asr_text, detected_lang): (Result<String, String>, Option<String>) = match cached {
        Some(ref c)
            if final_count > 0
                && final_count <= max_interim_window_samples
                && c.sample_count <= final_count
                && (final_count - c.sample_count) <= tail_gap_threshold_samples
                && !c.text.trim().is_empty() =>
        {
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
    };

    let text = match asr_text {
        Ok(t) => t.trim().to_string(),
        Err(e) => {
            emit_error(
                &app_handle,
                session_id,
                subtitle_show_gen,
                mode,
                RecordingOutcomeKind::AsrError,
                &e,
            );
            flush_pending_paste(&app_handle);
            return;
        }
    };
    let asr_elapsed_ms = elapsed_ms(asr_start);

    let lang_ref = detected_lang.as_deref();

    if text.is_empty() {
        emit_terminal_outcome(
            &app_handle,
            session_id,
            subtitle_show_gen,
            mode,
            RecordingOutcomeKind::NoSpeech,
            None,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    if mode == RecordingMode::Dictation && edit_context.is_some() {
        let selected_text = edit_context.unwrap_or_default();
        // 编辑模式：ASR 结果是语音指令，用它改写选中文本
        log::info!(
            "编辑模式：指令=\"{}\"，选中文本长度={}",
            text,
            selected_text.len()
        );
        match ai_polish_service::edit_text(
            state.inner(),
            &selected_text,
            &text,
            &app_handle,
            session_id,
        )
        .await
        {
            Ok(result) => {
                emit_done(
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
                );
                if !result.is_empty() {
                    let app = app_handle.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                        do_paste(&app, &result).await;
                    });
                } else {
                    flush_pending_paste(&app_handle);
                }
            }
            Err(e) => {
                log::warn!("编辑选中文本失败，不替换原文: {}", e);
                emit_error(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    RecordingOutcomeKind::ProcessingError,
                    &format!("编辑失败: {}", e),
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else if mode == RecordingMode::Assistant {
        match assistant_service::generate_content(
            state.inner(),
            &text,
            edit_context.as_deref(),
            &app_handle,
            session_id,
        )
        .await
        {
            Ok(result) => {
                emit_done(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    &result,
                    &text,
                    duration_sec,
                    false,
                    lang_ref,
                    edit_grab_status,
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
            }
            Err(err) => {
                emit_error(
                    &app_handle,
                    session_id,
                    subtitle_show_gen,
                    mode,
                    RecordingOutcomeKind::ProcessingError,
                    &err.to_string(),
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else {
        // 普通听写模式
        let original = text.clone();
        let ai_polish_enabled = state.profile.ai_polish_enabled.load(Ordering::Acquire);
        let raw_preview_stage = dictation_raw_preview_stage(trigger, ai_polish_enabled);
        let raw_paste_replacement = if should_raw_first_paste(trigger, ai_polish_enabled, true) {
            crate::commands::clipboard::capture_raw_paste_replacement_target(&original)
        } else {
            None
        };
        let raw_was_pasted = if raw_paste_replacement.is_some() {
            match do_paste_result(&app_handle, &original).await {
                Ok(()) => true,
                Err(err) => {
                    log::warn!(
                        "raw-first 听写原文粘贴失败，回退到 final-only 粘贴: {}",
                        err
                    );
                    false
                }
            }
        } else {
            false
        };
        let raw_first_preview_status = raw_first_preview_status_for_paste(raw_was_pasted);
        if let Some(stage) = raw_preview_stage {
            let timing = TranscriptionTiming {
                asr_ms: Some(asr_elapsed_ms),
                polish_ms: None,
                total_ms: Some(elapsed_ms(finalize_start)),
                raw_first: Some(RawFirstTiming {
                    status: raw_first_preview_status,
                }),
            };
            emit_transcription_result(
                &app_handle,
                session_id,
                mode,
                &original,
                &original,
                duration_sec,
                false,
                lang_ref,
                edit_grab_status,
                Some(stage),
                Some(timing),
            );
        }
        let translation_override = match trigger.dictation_output() {
            DictationOutputMode::Original => Some(None),
            DictationOutputMode::Translated => None,
        };
        let polish_start = Instant::now();
        let text = ai_polish_service::polish_text(
            state.inner(),
            &text,
            &app_handle,
            session_id,
            translation_override,
        )
        .await
        .unwrap_or_else(|e| {
            log::warn!("AI 润色失败，使用原文: {}", e);
            text
        });
        let polish_elapsed_ms = elapsed_ms(polish_start);
        let polished = text != original;
        let result_stage = dictation_final_result_stage(raw_preview_stage, polished);
        let mut should_paste_final = false;
        let raw_first_final_status = if raw_preview_stage.is_some() {
            Some(if let Some(token) = raw_paste_replacement.as_ref() {
                if raw_was_pasted {
                    if polished {
                        match crate::commands::clipboard::replace_raw_paste_suffix_if_unchanged(
                            token, &text,
                        ) {
                            Ok(true) => {
                                log::info!("raw-first 听写已替换为 AI 润色结果");
                                RawFirstStatus::Replaced
                            }
                            Ok(false) => {
                                log::warn!(
                                    "raw-first 听写未替换：目标内容已变化，保留原始 ASR 粘贴结果"
                                );
                                RawFirstStatus::KeptRaw
                            }
                            Err(err) => {
                                log::warn!("raw-first 听写替换失败: {}", err);
                                RawFirstStatus::KeptRaw
                            }
                        }
                    } else {
                        RawFirstStatus::Unchanged
                    }
                } else {
                    should_paste_final = true;
                    RawFirstStatus::FinalFallback
                }
            } else {
                should_paste_final = true;
                RawFirstStatus::PreviewOnly
            })
        } else {
            should_paste_final = true;
            None
        };
        let timing = TranscriptionTiming {
            asr_ms: Some(asr_elapsed_ms),
            polish_ms: Some(polish_elapsed_ms),
            total_ms: Some(elapsed_ms(finalize_start)),
            raw_first: raw_first_final_status.map(|status| RawFirstTiming { status }),
        };
        emit_done_with_stage(
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
            result_stage,
            Some(timing),
        );

        if !text.is_empty() {
            if should_paste_final {
                let app = app_handle.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                    do_paste(&app, &text).await;
                });
            }
        } else {
            flush_pending_paste(&app_handle);
        }
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
    log::info!("已丢弃录音会话 (session {})", session.session_id);
}

async fn do_final_asr(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    samples: &parking_lot::Mutex<Vec<i16>>,
    sample_rate: u32,
) -> Result<funasr_service::TranscriptionResult, String> {
    let data = std::mem::take(&mut *samples.lock());
    let (asr_audio, asr_sample_rate) = match ChunkedResampler::new(sample_rate) {
        Ok(mut resampler) => {
            let mut output = Vec::with_capacity(
                ((data.len() as f64 * TARGET_SAMPLE_RATE as f64 / sample_rate as f64).ceil()
                    as usize)
                    + 8,
            );
            match resampler
                .process_chunk(&data, &mut output)
                .and_then(|_| resampler.finish(&mut output))
            {
                Ok(()) => {
                    if sample_rate == TARGET_SAMPLE_RATE {
                        (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
                    } else {
                        (std::borrow::Cow::Owned(output), TARGET_SAMPLE_RATE)
                    }
                }
                Err(err) => {
                    log::warn!(
                        "最终音频重采样失败，保留原始采样率 {}Hz: {}",
                        sample_rate,
                        err
                    );
                    (std::borrow::Cow::Borrowed(data.as_slice()), sample_rate)
                }
            }
        }
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

fn dictation_raw_preview_stage(
    trigger: RecordingTrigger,
    ai_polish_enabled: bool,
) -> Option<&'static str> {
    (ai_polish_enabled
        && trigger.mode() == RecordingMode::Dictation
        && trigger.dictation_output() == DictationOutputMode::Original)
        .then_some("raw")
}

fn dictation_final_result_stage(
    raw_preview_stage: Option<&str>,
    polished: bool,
) -> Option<&'static str> {
    if raw_preview_stage.is_some() {
        Some("polished")
    } else {
        polished.then_some("polished")
    }
}

fn raw_first_preview_status_for_paste(raw_was_pasted: bool) -> RawFirstStatus {
    if raw_was_pasted {
        RawFirstStatus::Pasted
    } else {
        RawFirstStatus::PreviewOnly
    }
}

fn should_raw_first_paste(
    trigger: RecordingTrigger,
    ai_polish_enabled: bool,
    can_safely_replace_raw: bool,
) -> bool {
    dictation_raw_preview_stage(trigger, ai_polish_enabled).is_some() && can_safely_replace_raw
}

#[allow(clippy::too_many_arguments)]
fn emit_done(
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
) {
    emit_done_with_stage(
        app,
        sid,
        show_gen,
        mode,
        text,
        original_text,
        dur,
        polished,
        language,
        edit_grab_status,
        None,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_done_with_stage(
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
    result_stage: Option<&str>,
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
        result_stage,
        timing,
    );
    if mode != RecordingMode::Assistant || text.is_empty() {
        crate::commands::window::schedule_subtitle_hide(app, sid, show_gen, mode, delay);
    }
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
    result_stage: Option<&str>,
    timing: Option<TranscriptionTiming>,
) {
    let mut payload = serde_json::json!({
        "sessionId": sid, "text": text, "interim": false,
        "durationSec": dur, "charCount": text.chars().count(), "polished": polished,
        "language": language, "mode": mode.as_str(), "originalText": original_text,
        "editGrabStatus": edit_grab_status.as_str(),
    });
    if let Some(stage) = result_stage {
        payload["resultStage"] = serde_json::json!(stage);
    }
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

fn flush_pending_paste(app: &tauri::AppHandle) {
    let texts: Vec<String> = app
        .state::<AppState>()
        .recording
        .pending_paste
        .lock()
        .drain(..)
        .collect();
    if texts.is_empty() {
        return;
    }
    let combined: String = texts.into_iter().collect();
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
        do_paste(&app, &combined).await;
    });
}

async fn do_paste(app: &tauri::AppHandle, text: &str) {
    if let Err(e) = do_paste_result(app, text).await {
        log::error!("自动粘贴失败: {}", e);
    }
}

async fn do_paste_result(app: &tauri::AppHandle, text: &str) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    if state.recording.recording.lock().is_some() {
        state.recording.pending_paste.lock().push(text.to_string());
        log::info!("录音进行中，文本已加入待粘贴队列（{} 个字符）", text.len());
        return Ok(());
    }

    let mut full = String::new();
    for t in state.recording.pending_paste.lock().drain(..) {
        full.push_str(&t);
    }
    full.push_str(text);

    let method = state.ui.input_method.lock().clone();
    crate::commands::clipboard::paste_text_impl(app, &full, &method)
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RecordingTrigger;

    #[test]
    fn raw_preview_stage_is_enabled_for_original_dictation_with_ai_polish() {
        assert_eq!(
            dictation_raw_preview_stage(RecordingTrigger::DictationOriginal, true),
            Some("raw")
        );
    }

    #[test]
    fn raw_preview_stage_is_disabled_for_translation_and_unpolished_dictation() {
        assert_eq!(
            dictation_raw_preview_stage(RecordingTrigger::DictationTranslated, true),
            None
        );
        assert_eq!(
            dictation_raw_preview_stage(RecordingTrigger::DictationOriginal, false),
            None
        );
    }

    #[test]
    fn final_stage_after_raw_preview_means_polish_flow_completed_even_when_text_unchanged() {
        assert_eq!(
            dictation_final_result_stage(Some("raw"), false),
            Some("polished")
        );
        assert_eq!(
            dictation_final_result_stage(Some("raw"), true),
            Some("polished")
        );
        assert_eq!(dictation_final_result_stage(None, false), None);
        assert_eq!(dictation_final_result_stage(None, true), Some("polished"));
    }

    #[test]
    fn raw_first_preview_status_tracks_actual_paste_result() {
        assert_eq!(
            serde_json::to_value(raw_first_preview_status_for_paste(true)).unwrap(),
            serde_json::json!("pasted")
        );
        assert_eq!(
            serde_json::to_value(raw_first_preview_status_for_paste(false)).unwrap(),
            serde_json::json!("preview_only")
        );
    }

    #[test]
    fn raw_first_paste_requires_original_dictation_ai_polish_and_safe_replacement() {
        assert!(should_raw_first_paste(
            RecordingTrigger::DictationOriginal,
            true,
            true
        ));
        assert!(!should_raw_first_paste(
            RecordingTrigger::DictationOriginal,
            true,
            false
        ));
        assert!(!should_raw_first_paste(
            RecordingTrigger::DictationOriginal,
            false,
            true
        ));
        assert!(!should_raw_first_paste(
            RecordingTrigger::DictationTranslated,
            true,
            true
        ));
        assert!(!should_raw_first_paste(
            RecordingTrigger::Assistant,
            true,
            true
        ));
    }

    #[test]
    fn transcription_timing_serializes_as_frontend_camel_case_payload() {
        let value = serde_json::to_value(TranscriptionTiming {
            asr_ms: Some(42),
            polish_ms: None,
            total_ms: Some(45),
            raw_first: None,
        })
        .expect("timing should serialize");

        assert_eq!(value, serde_json::json!({ "asrMs": 42, "totalMs": 45 }));
    }

    #[test]
    fn transcription_timing_includes_raw_first_status_when_present() {
        let value = serde_json::to_value(TranscriptionTiming {
            asr_ms: Some(42),
            polish_ms: Some(900),
            total_ms: Some(948),
            raw_first: Some(RawFirstTiming {
                status: RawFirstStatus::Replaced,
            }),
        })
        .expect("timing should serialize");

        assert_eq!(
            value,
            serde_json::json!({
                "asrMs": 42,
                "polishMs": 900,
                "totalMs": 948,
                "rawFirst": { "status": "replaced" }
            })
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
