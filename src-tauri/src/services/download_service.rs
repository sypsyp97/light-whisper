//! 模型下载服务
//!
//! 从 commands/funasr.rs 中提取的模型下载逻辑。

use crate::services::funasr_service;
use crate::state::AppState;
use crate::utils::{paths, AppError};
use serde::de::DeserializeOwned;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;

/// 下载进度行的 JSON 结构
#[derive(serde::Deserialize)]
struct DownloadLine {
    success: Option<bool>,
    stage: Option<String>,
    model: Option<String>,
    progress: Option<f64>,
    overall_progress: Option<f64>,
    message: Option<String>,
    error: Option<String>,
}

static NEXT_DOWNLOAD_TASK_ID: AtomicU64 = AtomicU64::new(1);

fn download_completed_successfully(protocol_success: Option<bool>, process_success: bool) -> bool {
    process_success && protocol_success.unwrap_or(true)
}

fn parse_json_line_with_recovery<T>(raw: &[u8], context: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    let line = match std::str::from_utf8(raw) {
        Ok(line) => std::borrow::Cow::Borrowed(line),
        Err(err) => {
            log::warn!(
                "{}收到非 UTF-8 输出，已按损坏文本容错处理: {}",
                context,
                err
            );
            String::from_utf8_lossy(raw)
        }
    };

    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(payload) = serde_json::from_str::<T>(trimmed) {
        return Some(payload);
    }

    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            if let Ok(payload) = serde_json::from_str::<T>(&trimmed[start..=end]) {
                log::warn!("{}从混合输出中恢复了 JSON 响应", context);
                return Some(payload);
            }
        }
    }

    log::warn!(
        "{}收到非 JSON 输出（{}字符）",
        context,
        trimmed.chars().count()
    );
    None
}

async fn clear_download_task(state: &AppState, task_id: u64) {
    let mut guard = state.engine.download_task.lock().await;
    if guard.as_ref().is_some_and(|task| task.id == task_id) {
        guard.take();
    }
}

fn emit_download_status(app_handle: &tauri::AppHandle, payload: serde_json::Value) {
    let _ = app_handle.emit("model-download-status", payload);
}

/// 执行模型下载
///
/// 启动 Python 下载脚本，逐行读取进度并通过 Tauri 事件转发给前端。
/// 支持通过 cancel channel 取消下载。
pub async fn run_download(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, AppError> {
    // 获取下载脚本路径，清理 Windows \\?\ 前缀
    let download_script = paths::get_download_script_path(app_handle);
    let download_script_str = paths::strip_win_prefix(&download_script);

    let data_dir = paths::strip_win_prefix(paths::get_data_dir());

    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    let task_id = NEXT_DOWNLOAD_TASK_ID.fetch_add(1, Ordering::Relaxed);
    {
        // 与模型目录切换串行登记。登记完成后 set_models_dir 会看到 active slot
        // 并拒绝迁移，直到下载子进程真正退出并清理自己的 task ID。
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        let mut guard = state.engine.download_task.lock().await;
        if guard.is_some() {
            return Err(AppError::Download(
                "已有下载任务正在进行，请先取消或等待完成".to_string(),
            ));
        }
        *guard = Some(crate::state::DownloadTask {
            id: task_id,
            cancel: Some(cancel_tx),
        });
    }

    // 查找引擎运行时；失败时也必须释放刚登记的下载槽。
    let generation = state.engine.funasr_generation.load(Ordering::SeqCst);
    let runtime = match funasr_service::find_engine(app_handle, state, generation).await {
        Ok(runtime) => runtime,
        Err(err) => {
            clear_download_task(state, task_id).await;
            return Err(err);
        }
    };

    // 仅开发模式需要检查脚本是否存在；此时 task 已登记，错误路径必须按 ID 清理。
    if matches!(runtime, funasr_service::EngineRuntime::Development { .. })
        && !download_script.exists()
    {
        clear_download_task(state, task_id).await;
        return Err(AppError::Download(format!(
            "模型下载脚本不存在: {}",
            download_script_str
        )));
    }

    // 通知前端开始下载
    emit_download_status(
        app_handle,
        serde_json::json!({
            "status": "downloading",
            "message": "开始下载模型文件..."
        }),
    );

    // 启动下载脚本（逐行读取 stdout 以转发进度）
    // 模型从 HuggingFace 下载，使用 HF 默认缓存目录
    let engine = paths::read_engine_config();
    let engine_arg = if engine == "whisper" {
        "whisper"
    } else {
        "sensevoice"
    };

    let mut cmd = match &runtime {
        funasr_service::EngineRuntime::Bundled { exe_path } => {
            let mut c = Command::new(exe_path);
            c.arg("download").arg("--engine").arg(engine_arg);
            c
        }
        funasr_service::EngineRuntime::Development { python_path } => {
            let mut c = Command::new(python_path);
            c.arg("-X")
                .arg("utf8")
                .arg("-u")
                .arg(&download_script_str)
                .arg("--engine")
                .arg(engine_arg);
            c
        }
    };

    let models_dir = paths::strip_win_prefix(&paths::get_effective_models_dir());
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .env("HF_HUB_CACHE", &models_dir)
        .stdout(Stdio::piped())
        .stderr({
            let log_path = paths::get_data_dir().join("download_stderr.log");
            match std::fs::File::create(&log_path) {
                Ok(file) => Stdio::from(file),
                Err(_) => Stdio::null(),
            }
        });

    // Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            clear_download_task(state, task_id).await;
            return Err(AppError::Download(format!("启动模型下载脚本失败: {}", e)));
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            clear_download_task(state, task_id).await;
            return Err(AppError::Download("无法读取模型下载脚本输出".to_string()));
        }
    };

    let mut reader = BufReader::new(stdout);
    let mut final_result: Option<DownloadLine> = None;
    let mut cancelled = false;
    let mut line_bytes = Vec::new();
    loop {
        line_bytes.clear();
        tokio::select! {
            _ = &mut cancel_rx => {
                cancelled = true;
                let _ = child.kill().await;
                emit_download_status(app_handle, serde_json::json!({
                    "status": "cancelled",
                    "message": "下载已取消"
                }));
                break;
            }
            bytes = reader.read_until(b'\n', &mut line_bytes) => {
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        log::warn!("读取下载输出失败（跳过）: {}", e);
                        continue;
                    }
                };
                if bytes == 0 {
                    break;
                }

                let Some(payload) = parse_json_line_with_recovery::<DownloadLine>(&line_bytes, "模型下载输出") else {
                    continue;
                };

                if payload.success.is_some() {
                    final_result = Some(payload);
                    continue;
                }

                let progress = payload
                    .overall_progress
                    .or(payload.progress)
                    .unwrap_or(0.0);

                let message = payload.message.clone().or_else(|| {
                    payload.model.clone().map(|m| format!("{} 下载中", m))
                });

                let status = match payload.stage.as_deref() {
                    Some("error") => "error",
                    _ => "progress",
                };

                emit_download_status(app_handle, serde_json::json!({
                    "status": status,
                    "progress": progress,
                    "message": message.unwrap_or_else(|| "模型下载中...".to_string()),
                    "error": payload.error
                }));
            }
        }
    }

    let status = match child.wait().await {
        Ok(status) => status,
        Err(e) => {
            clear_download_task(state, task_id).await;
            return Err(AppError::Download(format!("模型下载进程异常退出: {}", e)));
        }
    };

    let final_success = download_completed_successfully(
        final_result.as_ref().and_then(|r| r.success),
        status.success(),
    );

    // 清理下载任务
    clear_download_task(state, task_id).await;

    if cancelled {
        return Ok("模型下载已取消".to_string());
    }

    if final_success {
        emit_download_status(
            app_handle,
            serde_json::json!({
                "status": "completed",
                "progress": 100,
                "message": "模型下载完成"
            }),
        );
        Ok("模型下载完成".to_string())
    } else {
        let error_msg = final_result
            .and_then(|r| r.error.or(r.message))
            .unwrap_or_else(|| "模型下载失败".to_string());

        emit_download_status(
            app_handle,
            serde_json::json!({
                "status": "error",
                "message": &error_msg
            }),
        );

        Err(AppError::Download(error_msg))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_download_task, download_completed_successfully, parse_json_line_with_recovery,
        DownloadLine,
    };
    use crate::state::{AppState, DownloadTask};

    #[tokio::test]
    async fn old_download_cleanup_must_not_clear_replacement_task() {
        let state = AppState::new();
        let (old_cancel, _old_cancel_rx) = tokio::sync::oneshot::channel();
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 1,
            cancel: Some(old_cancel),
        });

        let old_task = state
            .engine
            .download_task
            .lock()
            .await
            .take()
            .expect("old download task should be active before cancellation");
        let _ = old_task
            .cancel
            .expect("old task must be cancellable")
            .send(());

        let (replacement_cancel, _replacement_cancel_rx) = tokio::sync::oneshot::channel();
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 2,
            cancel: Some(replacement_cancel),
        });

        clear_download_task(&state, 1).await;

        assert!(
            state.engine.download_task.lock().await.is_some(),
            "cleanup from the cancelled download must not clear its replacement"
        );
    }

    #[test]
    fn final_success_requires_protocol_and_process_success() {
        assert!(download_completed_successfully(Some(true), true));
        assert!(!download_completed_successfully(Some(false), true));
        assert!(download_completed_successfully(None, true));
        assert!(
            !download_completed_successfully(Some(true), false),
            "a JSON success response must not override a failing process exit status"
        );
    }

    #[test]
    fn parse_download_line_accepts_valid_json() {
        let payload = parse_json_line_with_recovery::<DownloadLine>(
            br#"{"stage":"downloading","progress":50,"message":"ok"}"#,
            "test",
        )
        .unwrap();

        assert_eq!(payload.stage.as_deref(), Some("downloading"));
        assert_eq!(payload.progress, Some(50.0));
        assert_eq!(payload.message.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_download_line_recovers_from_non_utf8_prefix() {
        let payload = parse_json_line_with_recovery::<DownloadLine>(
            b"\xff\xfe{\"stage\":\"downloading\",\"progress\":50,\"message\":\"ok\"}",
            "test",
        )
        .unwrap();

        assert_eq!(payload.stage.as_deref(), Some("downloading"));
        assert_eq!(payload.progress, Some(50.0));
        assert_eq!(payload.message.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_download_line_recovers_json_from_mixed_output() {
        let payload = parse_json_line_with_recovery::<DownloadLine>(
            br#"noise >>> {"stage":"downloading","progress":50,"message":"ok"}"#,
            "test",
        )
        .unwrap();

        assert_eq!(payload.stage.as_deref(), Some("downloading"));
        assert_eq!(payload.progress, Some(50.0));
        assert_eq!(payload.message.as_deref(), Some("ok"));
    }

    #[test]
    fn parse_download_line_rejects_non_json_noise() {
        let payload =
            parse_json_line_with_recovery::<DownloadLine>(b"{'load_data': '0.001'}", "test");

        assert!(payload.is_none());
    }
}
