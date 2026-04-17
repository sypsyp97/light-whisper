use std::sync::atomic::Ordering;

use tauri::{Emitter, Manager};

use super::resample::resample_to_16k;
use super::wav::encode_wav;
use super::{
    EDIT_GRAB_WAIT_MS, EMPTY_RESULT_HIDE_DELAY_MS, INTERIM_MAX_AUDIO_WINDOW_SEC,
    MIN_AUDIO_DURATION_SEC, PASTE_DELAY_MS, RESULT_HIDE_DELAY_MS, TARGET_SAMPLE_RATE,
};
use crate::services::{
    ai_polish_service, alibaba_asr_service, assistant_service, funasr_service, glm_asr_service,
};
use crate::state::{AppState, DictationOutputMode, RecordingMode, RecordingSession, RecordingSlot};
use crate::utils::paths;

// ---------- 最终转写 + 粘贴 ----------

pub async fn finalize_recording(app_handle: tauri::AppHandle, session: RecordingSession) {
    let RecordingSession {
        session_id,
        trigger,
        sample_rate,
        audio_thread,
        interim_task,
        samples,
        interim_cache,
        edit_grab,
        ..
    } = session;

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
    let edit_context: Option<String> = match edit_grab {
        Some(handle) => {
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(
                std::time::Duration::from_millis(EDIT_GRAB_WAIT_MS),
                handle,
            )
            .await
            {
                Ok(Ok(Some(selected))) => Some(selected),
                Ok(Ok(None)) => None,
                Ok(Err(join_err)) => {
                    log::debug!("选中文本抓取任务 join 失败: {}", join_err);
                    None
                }
                Err(_) => {
                    log::debug!(
                        "选中文本抓取超过 {}ms，按普通听写处理",
                        EDIT_GRAB_WAIT_MS
                    );
                    abort_handle.abort();
                    None
                }
            }
        }
        None => None,
    };

    let state = app_handle.state::<AppState>();

    let final_count = samples.lock().len();
    let cached = interim_cache.lock().clone();
    let duration_sec = final_count as f64 / sample_rate as f64;
    let mode = trigger.mode();

    if duration_sec < MIN_AUDIO_DURATION_SEC {
        log::info!("录音时间过短 ({:.2}s)，跳过转写", duration_sec);
        emit_done(
            &app_handle,
            session_id,
            mode,
            "",
            "",
            duration_sec,
            false,
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
    let max_interim_window_samples =
        (sample_rate as f64 * INTERIM_MAX_AUDIO_WINDOW_SEC) as usize;
    let tail_gap_threshold_samples = (sample_rate as f64 * 0.25) as usize;
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
            emit_error(&app_handle, session_id, mode, &e);
            flush_pending_paste(&app_handle);
            return;
        }
    };

    let lang_ref = detected_lang.as_deref();

    if text.is_empty() {
        emit_done(
            &app_handle,
            session_id,
            mode,
            "",
            "",
            duration_sec,
            false,
            lang_ref,
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
                    mode,
                    &result,
                    &result,
                    duration_sec,
                    true,
                    lang_ref,
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
                let _ = app_handle.emit(
                    "recording-error",
                    serde_json::json!({ "message": format!("编辑失败: {}", e) }),
                );
                emit_done(
                    &app_handle,
                    session_id,
                    mode,
                    "",
                    &selected_text,
                    duration_sec,
                    false,
                    lang_ref,
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
                    mode,
                    &result,
                    &text,
                    duration_sec,
                    false,
                    lang_ref,
                );
                if let Err(err) =
                    crate::commands::window::set_subtitle_window_interactive(&app_handle, true)
                {
                    log::warn!("助手结果显示时切换字幕窗口交互态失败: {}", err);
                }
            }
            Err(err) => {
                emit_error(&app_handle, session_id, mode, &err.to_string());
                flush_pending_paste(&app_handle);
            }
        }
    } else {
        // 普通听写模式
        let original = text.clone();
        let translation_override = match trigger.dictation_output() {
            DictationOutputMode::Original => Some(None),
            DictationOutputMode::Translated => None,
        };
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
        let polished = text != original;
        emit_done(
            &app_handle,
            session_id,
            mode,
            &text,
            &original,
            duration_sec,
            polished,
            lang_ref,
        );

        if !text.is_empty() {
            let app = app_handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                do_paste(&app, &text).await;
            });
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
    let data = samples.lock().clone();
    let resampled = resample_to_16k(&data, sample_rate);

    let engine = paths::read_engine_config();
    let result = if paths::is_online_engine(&engine) {
        let wav = encode_wav(&resampled, TARGET_SAMPLE_RATE)
            .map_err(|e| format!("WAV 编码失败: {}", e))?;
        match engine.as_str() {
            "alibaba-asr" => alibaba_asr_service::transcribe(state, wav).await,
            _ => glm_asr_service::transcribe(state, wav).await,
        }
    } else {
        funasr_service::transcribe_pcm16(state, &resampled, TARGET_SAMPLE_RATE, app_handle).await
    };

    match result {
        Ok(r) if r.success => Ok(r),
        Ok(r) => Err(r.error.unwrap_or_else(|| "语音识别失败".into())),
        Err(e) => Err(format!("语音识别失败: {}", e)),
    }
}

// ---------- 事件发送 ----------

#[allow(clippy::too_many_arguments)]
fn emit_done(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    text: &str,
    original_text: &str,
    dur: f64,
    polished: bool,
    language: Option<&str>,
) {
    let delay = if text.is_empty() {
        EMPTY_RESULT_HIDE_DELAY_MS
    } else {
        RESULT_HIDE_DELAY_MS
    };
    emit_recording_state_if_current(app, sid, mode, false, false, None);
    let _ = app.emit(
        "transcription-result",
        serde_json::json!({
            "sessionId": sid, "text": text, "interim": false,
            "durationSec": dur, "charCount": text.chars().count(), "polished": polished,
            "language": language, "mode": mode.as_str(), "originalText": original_text,
        }),
    );
    if mode != RecordingMode::Assistant || text.is_empty() {
        schedule_hide(app, delay);
    }
}

fn emit_error(app: &tauri::AppHandle, sid: u64, mode: RecordingMode, error: &str) {
    emit_recording_state_if_current(app, sid, mode, false, false, Some(error));
    schedule_hide(app, EMPTY_RESULT_HIDE_DELAY_MS);
}

fn emit_recording_state_if_current(
    app: &tauri::AppHandle,
    sid: u64,
    mode: RecordingMode,
    recording: bool,
    processing: bool,
    error: Option<&str>,
) {
    let state = app.state::<AppState>();
    if let Some(active) = state
        .recording
        .recording
        .lock()
        .as_ref()
        .map(RecordingSlot::session_id)
    {
        if active != sid {
            log::info!("跳过过期会话状态广播 (session {}, active {})", sid, active);
            return;
        }
    }
    let mut payload = serde_json::json!({
        "sessionId": sid, "isRecording": recording, "isProcessing": processing,
        "mode": mode.as_str(),
    });
    if let Some(err) = error {
        payload["error"] = serde_json::json!(err);
    }
    let _ = app.emit("recording-state", payload);
}

// ---------- 粘贴逻辑 ----------

fn schedule_hide(app: &tauri::AppHandle, delay_ms: u64) {
    let app = app.clone();
    let gen = app
        .state::<AppState>()
        .recording
        .subtitle_show_gen
        .load(Ordering::Relaxed);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let state = app.state::<AppState>();
        if state.recording.subtitle_show_gen.load(Ordering::Relaxed) != gen {
            return;
        }
        if state.recording.recording.lock().is_some() {
            return;
        }
        let _ = crate::commands::window::hide_subtitle_window_inner(&app);
    });
}

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
    let state = app.state::<AppState>();
    if state.recording.recording.lock().is_some() {
        state.recording.pending_paste.lock().push(text.to_string());
        log::info!("录音进行中，文本已加入待粘贴队列（{} 个字符）", text.len());
        return;
    }

    let mut full = String::new();
    for t in state.recording.pending_paste.lock().drain(..) {
        full.push_str(&t);
    }
    full.push_str(text);

    let method = state.ui.input_method.lock().clone();
    if let Err(e) = crate::commands::clipboard::paste_text_impl(app, &full, &method).await {
        log::error!("自动粘贴失败: {}", e);
    }
}
