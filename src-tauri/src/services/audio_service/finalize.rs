use std::sync::atomic::Ordering;

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
    AppState, DictationOutputMode, LastDictation, RecordingMode, RecordingSession, RecordingSlot,
};
use crate::utils::foreground::{get_foreground_app, ForegroundApp};
use crate::utils::paths;

// ---------- 重说纠错参数 ----------

/// 「思考间隔」窗口（毫秒）：从上一句结果出现，到本次重新开口说话之间允许的最大间隔。
/// 注意这里测的是「上一句出结果」到「本次开始录音」的纯思考时间，已扣除本次说话与
/// 识别耗时，所以不用设很大。超过则视为新的一句。
const REDO_WINDOW_MS: u64 = 12000;
/// 判定为「重说同一句」的最小字符相似度（0~1，基于编辑距离）。
const REDO_SIMILARITY_THRESHOLD: f64 = 0.5;
/// 参与重说判定的最短文本长度（字符）。太短的话语相似度噪声大，直接跳过。
const REDO_MIN_CHARS: usize = 2;

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
        emit_done(
            &app_handle,
            session_id,
            mode,
            "",
            "",
            duration_sec,
            false,
            None,
            edit_grab_status,
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
            edit_grab_status,
        );
        flush_pending_paste(&app_handle);
        return;
    }

    if mode == RecordingMode::Dictation && edit_context.is_some() {
        // 编辑选中文本会改变光标 / 内容状态，之前记的「上一句听写」不再可靠，清空。
        *state.recording.last_dictation.lock() = None;
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
                    edit_grab_status,
                );
                flush_pending_paste(&app_handle);
            }
        }
    } else if mode == RecordingMode::Assistant {
        // 助手模式不向目标窗口直接输入听写文本，清空重说记忆避免误判。
        *state.recording.last_dictation.lock() = None;
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
                    edit_grab_status,
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

        // —— 重说纠错判定 ——
        // 取出上一句听写记忆，若本次满足「同程序 + 思考间隔内 + 足够相似」，视为对上一句的重说修正。
        // current_start_ms：本次大致的开始录音时刻 = 现在 - 本次音频时长，窗口只算思考停顿。
        let now_ms = now_unix_ms();
        let current_start_ms = now_ms.saturating_sub((duration_sec * 1000.0) as u64);
        let fg = get_foreground_app();
        let redo_target = state
            .recording
            .last_dictation
            .lock()
            .clone()
            .filter(|prev| is_redo_correction(prev, current_start_ms, &fg, &text));

        if let Some(prev) = redo_target {
            log::info!(
                "重说纠错触发：上一句=\"{}\"，本次=\"{}\"",
                prev.text,
                text
            );
            // 用 LLM 把上一句和重说合并成最终文本；失败则直接用本次文本替换。
            let merged = ai_polish_service::merge_redo(
                state.inner(),
                &prev.text,
                &text,
                &app_handle,
                session_id,
            )
            .await
            .unwrap_or_else(|e| {
                log::warn!("重说合并失败，改用本次文本替换上一句: {}", e);
                text.clone()
            });

            emit_done(
                &app_handle,
                session_id,
                mode,
                &merged,
                &original,
                duration_sec,
                true,
                lang_ref,
                edit_grab_status,
            );
            record_last_dictation(state.inner(), &merged, &fg, now_ms);

            if !merged.is_empty() {
                let app = app_handle.clone();
                let delete_count = prev.char_count;
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(PASTE_DELAY_MS)).await;
                    do_replace_paste(&app, delete_count, &merged).await;
                });
            } else {
                flush_pending_paste(&app_handle);
            }
        } else {
            emit_done(
                &app_handle,
                session_id,
                mode,
                &text,
                &original,
                duration_sec,
                polished,
                lang_ref,
                edit_grab_status,
            );
            record_last_dictation(state.inner(), &text, &fg, now_ms);

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
    edit_grab_status: EditGrabStatus,
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
            "editGrabStatus": edit_grab_status.as_str(),
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

/// 重说纠错的输入：先删掉上一句已输入的 `delete_count` 个字符，再粘贴合并后的文本。
async fn do_replace_paste(app: &tauri::AppHandle, delete_count: usize, text: &str) {
    let state = app.state::<AppState>();

    // 已经开始新一轮录音 → 删除不再安全（光标 / 焦点可能已变），退回普通追加。
    if state.recording.recording.lock().is_some() {
        state.recording.pending_paste.lock().push(text.to_string());
        log::info!("重说纠错时检测到新录音，已退回普通追加（{} 个字符）", text.len());
        return;
    }

    if let Err(e) = crate::commands::clipboard::send_backspaces(delete_count).await {
        // 删除失败就不要再粘贴，否则会在旧文本后面追加出重复内容。
        log::error!("重说纠错删除旧文本失败，跳过替换: {}", e);
        return;
    }
    // 给目标窗口一点时间处理退格，再粘贴新文本。
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let mut full = String::new();
    for t in state.recording.pending_paste.lock().drain(..) {
        full.push_str(&t);
    }
    full.push_str(text);

    let method = state.ui.input_method.lock().clone();
    if let Err(e) = crate::commands::clipboard::paste_text_impl(app, &full, &method).await {
        log::error!("重说纠错粘贴失败: {}", e);
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 字符级编辑距离（Levenshtein），用于判断两句话是否是「重说的同一句」。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let n = b.len();
    if a.is_empty() {
        return n;
    }
    if n == 0 {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur: Vec<usize> = vec![0; n + 1];
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

/// 归一化字符相似度，范围 0~1，1 表示完全相同。
fn char_similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    1.0 - (levenshtein(&a, &b) as f64 / max_len as f64)
}

/// 判断这次听写结果是否是对上一句的「重说纠错」：
/// 思考间隔够近 + 同一前台程序 + 与上一句足够相似。
/// `current_start_ms` 是本次「开始录音」的大致时刻（= 出结果时刻 - 本次音频时长），
/// 这样窗口只衡量用户的思考停顿，不把说话与识别耗时算进去。
/// 全程打日志，方便排查为什么触发 / 不触发。
fn is_redo_correction(
    prev: &LastDictation,
    current_start_ms: u64,
    fg: &Option<ForegroundApp>,
    new_text: &str,
) -> bool {
    let gap_ms = current_start_ms.saturating_sub(prev.at_ms);
    let new_chars = new_text.chars().count();
    let fg_proc = fg.as_ref().map(|a| a.process_name.as_str()).unwrap_or("");
    let proc_match = !fg_proc.is_empty() && fg_proc == prev.process_name;
    let sim = char_similarity(&prev.text, new_text);

    let decision = gap_ms <= REDO_WINDOW_MS
        && new_chars >= REDO_MIN_CHARS
        && prev.char_count >= REDO_MIN_CHARS
        && proc_match
        && sim >= REDO_SIMILARITY_THRESHOLD;

    log::info!(
        "重说判定: 间隔={}ms(上限{}) 进程匹配={}(now=\"{}\" prev=\"{}\") 相似度={:.2}(下限{:.2}) 上一句=\"{}\" 本次=\"{}\" => {}",
        gap_ms,
        REDO_WINDOW_MS,
        proc_match,
        fg_proc,
        prev.process_name,
        sim,
        REDO_SIMILARITY_THRESHOLD,
        prev.text,
        new_text,
        if decision { "判为重说纠错" } else { "判为新内容" }
    );

    decision
}

/// 记录最近一次成功输入的听写结果，供下一次重说判定使用；空文本则清空记忆。
fn record_last_dictation(state: &AppState, text: &str, fg: &Option<ForegroundApp>, now_ms: u64) {
    if text.is_empty() {
        *state.recording.last_dictation.lock() = None;
        return;
    }
    let process_name = match fg {
        Some(app) => app.process_name.clone(),
        None => String::new(),
    };
    log::info!(
        "记住上一句听写: \"{}\"（{} 字, 程序=\"{}\"）供下次重说纠错比对",
        text,
        text.chars().count(),
        process_name
    );
    *state.recording.last_dictation.lock() = Some(LastDictation {
        char_count: text.chars().count(),
        text: text.to_string(),
        at_ms: now_ms,
        process_name,
    });
}
