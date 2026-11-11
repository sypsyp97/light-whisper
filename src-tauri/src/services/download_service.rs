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
    request_id: Option<String>,
) -> Result<String, AppError> {
    let preparation = async {
        // 获取下载脚本路径，清理 Windows \\?\ 前缀
        let download_script = paths::get_download_script_path(app_handle);
        let download_script_str = paths::strip_win_prefix(&download_script);

        let data_dir = paths::strip_win_prefix(paths::get_data_dir());

        // 查找引擎运行时；失败时也必须释放刚登记的下载槽。
        let generation = state.engine.funasr_generation.load(Ordering::SeqCst);
        let runtime = funasr_service::find_engine(app_handle, state, generation).await?;

        // 仅开发模式需要检查脚本是否存在；此时 task 已登记，错误路径必须按 ID 清理。
        if matches!(runtime, funasr_service::EngineRuntime::Development { .. })
            && !download_script.exists()
        {
            return Err(AppError::Download(format!(
                "模型下载脚本不存在: {}",
                download_script_str
            )));
        }

        // 启动下载脚本（逐行读取 stdout 以转发进度）
        // 模型从 HuggingFace 下载，使用 HF 默认缓存目录
        let engine = paths::read_engine_config();
        let engine_arg = match engine.as_str() {
            "qwen3-asr-0.6b" | "confucius4-r2t2" => engine.as_str(),
            _ => "qwen3-asr-0.6b",
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

        Ok(cmd)
    };
    run_download_worker(state, request_id, preparation, |payload| {
        emit_download_status(app_handle, payload);
    })
    .await
}

async fn run_download_worker<F, E>(
    state: &AppState,
    request_id: Option<String>,
    preparation: F,
    emit: E,
) -> Result<String, AppError>
where
    F: std::future::Future<Output = Result<Command, AppError>>,
    E: Fn(serde_json::Value),
{
    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    let (done_tx, done) = tokio::sync::watch::channel(false);
    let task_id = NEXT_DOWNLOAD_TASK_ID.fetch_add(1, Ordering::Relaxed);
    {
        // Reserve before waiting for lifecycle work so cancellation can always
        // identify this attempt. Drop the slot lock before taking lifecycle.
        let mut guard = state.engine.download_task.lock().await;
        if guard.is_some() {
            return Err(AppError::Download(
                "已有下载任务正在进行，请先取消或等待完成".to_string(),
            ));
        }
        *guard = Some(crate::state::DownloadTask {
            id: task_id,
            request_id: request_id.clone(),
            cancel: Some(cancel_tx),
            done,
        });
    }
    let publish = |mut payload: serde_json::Value| {
        payload["requestId"] = serde_json::json!(request_id);
        emit(payload);
    };

    let outcome: Result<bool, AppError> = async {
        // Finish any migration that acquired lifecycle before this reservation.
        // Once reserved, configuration writers reject the active download slot.
        tokio::select! {
            biased;
            _ = &mut cancel_rx => return Ok(false),
            guard = state.engine.funasr_lifecycle_op.lock() => drop(guard),
        }
        // Engine preparation can own a spawn_blocking archive extraction. Never
        // drop it on cancellation: retain ownership until its side effects stop,
        // then discard its command rather than starting a cancelled download.
        let prepared = preparation.await;
        if !matches!(cancel_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)) {
            return Ok(false);
        }
        let mut cmd = prepared?;
        cmd.kill_on_drop(true);
        publish(serde_json::json!({
            "status": "downloading",
            "message": "开始下载模型文件..."
        }));
        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Download(format!("启动模型下载脚本失败: {}", e)))?;
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(AppError::Download("无法读取模型下载脚本输出".to_string()));
        };
        let mut reader = BufReader::new(stdout);
        let mut final_result: Option<DownloadLine> = None;
        let mut cancelled = false;
        let mut line_bytes = Vec::new();
        loop {
            line_bytes.clear();
            tokio::select! {
                biased;
                _ = &mut cancel_rx => {
                    cancelled = true;
                    let _ = child.kill().await;
                    break;
                }
                bytes = reader.read_until(b'\n', &mut line_bytes) => {
                    let bytes = match bytes {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            return Err(AppError::Download(format!("读取下载输出失败: {}", e)));
                        }
                    };
                    if bytes == 0 {
                        break;
                    }
                    let Some(mut payload) = parse_json_line_with_recovery::<DownloadLine>(&line_bytes, "模型下载输出") else {
                        continue;
                    };
                    // The script emits stage:error and then success:false. Both
                    // describe one outcome, published only after process exit.
                    if payload.stage.as_deref() == Some("error") {
                        payload.success = Some(false);
                    }
                    if payload.success.is_some() {
                        final_result = Some(payload);
                        continue;
                    }
                    let progress = payload.overall_progress.or(payload.progress).unwrap_or(0.0);
                    let message = payload.message.or_else(|| {
                        payload.model.map(|model| format!("{} 下载中", model))
                    });
                    publish(serde_json::json!({
                        "status": "progress",
                        "progress": progress,
                        "message": message.unwrap_or_else(|| "模型下载中...".to_string()),
                        "error": payload.error
                    }));
                }
            }
        }
        // A consumed oneshot receiver must not be polled again.
        let waited = if cancelled {
            child.wait().await.map(|status| (status, true))
        } else {
            wait_download_exit(&mut child, &mut cancel_rx).await
        };
        let (status, cancelled) = waited
            .map_err(|e| AppError::Download(format!("模型下载进程异常退出: {}", e)))?;
        if cancelled {
            return Ok(false);
        }
        if download_completed_successfully(
            final_result.as_ref().and_then(|result| result.success),
            status.success(),
        ) {
            Ok(true)
        } else {
            Err(AppError::Download(
                final_result
                    .and_then(|result| result.error.or(result.message))
                    .unwrap_or_else(|| "模型下载失败".to_string()),
            ))
        }
    }.await;

    // Terminal publication, release and acknowledgement form one ordered exit.
    let result = match outcome {
        Ok(completed) => {
            let (status, message) = if completed {
                ("completed", "模型下载完成")
            } else {
                ("cancelled", "模型下载已取消")
            };
            publish(serde_json::json!({
                "status": status,
                "message": message,
                "progress": if completed { Some(100) } else { None }
            }));
            Ok(message.to_string())
        }
        Err(error) => {
            publish(serde_json::json!({
                "status": "error",
                "message": error.to_string()
            }));
            Err(error)
        }
    };
    clear_download_task(state, task_id).await;
    let _ = done_tx.send(true);
    result
}

async fn wait_download_exit(
    child: &mut tokio::process::Child,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<(std::process::ExitStatus, bool), std::io::Error> {
    tokio::select! {
        biased;
        _ = cancel => {
            let _ = child.kill().await;
            child.wait().await.map(|status| (status, true))
        }
        status = child.wait() => status.map(|status| (status, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_download_task, download_completed_successfully, parse_json_line_with_recovery,
        DownloadLine,
    };
    use crate::state::{AppState, DownloadTask};
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    use tauri::Manager;

    #[tokio::test]
    async fn cancellation_ack_waits_until_the_worker_releases_its_slot() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = app.state::<AppState>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 41,
            request_id: Some("download-a".to_string()),
            cancel: Some(cancel_tx),
            done,
        });
        let mut cancellation = Box::pin(crate::commands::funasr::cancel_model_download(
            app.state::<AppState>(),
            None,
        ));
        let mut context = Context::from_waker(Waker::noop());

        assert!(
            cancellation.as_mut().poll(&mut context).is_pending(),
            "sending cancellation is not acknowledgement that the worker has stopped"
        );
        assert_eq!(cancel_rx.try_recv(), Ok(()));
        assert_eq!(
            state
                .engine
                .download_task
                .lock()
                .await
                .as_ref()
                .map(|task| task.id),
            Some(41),
            "the worker must keep its slot while stopping"
        );

        // The test worker finishes without launching a downloader or touching model files.
        clear_download_task(state.inner(), 41).await;
        assert!(
            cancellation.as_mut().poll(&mut context).is_pending(),
            "slot disappearance alone must not acknowledge an unfinished worker"
        );
        done_tx
            .send(true)
            .expect("announce worker completion after terminal publication and cleanup");
        assert!(matches!(
            cancellation.as_mut().poll(&mut context),
            Poll::Ready(Ok(_))
        ));
        assert!(state.engine.download_task.lock().await.is_none());
    }

    #[tokio::test]
    async fn repeated_cancellation_waits_for_the_same_worker_to_finish() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = app.state::<AppState>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 42,
            request_id: Some("download-b".to_string()),
            cancel: Some(cancel_tx),
            done,
        });
        let mut first = Box::pin(crate::commands::funasr::cancel_model_download(
            app.state::<AppState>(),
            Some("download-b".to_string()),
        ));
        let mut second = Box::pin(crate::commands::funasr::cancel_model_download(
            app.state::<AppState>(),
            Some("download-b".to_string()),
        ));
        let mut context = Context::from_waker(Waker::noop());

        assert!(first.as_mut().poll(&mut context).is_pending());
        assert_eq!(cancel_rx.try_recv(), Ok(()));
        assert!(
            second.as_mut().poll(&mut context).is_pending(),
            "a second caller must wait even though the cancellation signal was already taken"
        );
        assert!(state.engine.download_task.lock().await.is_some());

        clear_download_task(state.inner(), 42).await;
        assert!(first.as_mut().poll(&mut context).is_pending());
        assert!(second.as_mut().poll(&mut context).is_pending());
        done_tx
            .send(true)
            .expect("announce the shared worker completion");
        assert!(matches!(
            first.as_mut().poll(&mut context),
            Poll::Ready(Ok(_))
        ));
        assert!(matches!(
            second.as_mut().poll(&mut context),
            Poll::Ready(Ok(_))
        ));
    }

    #[tokio::test]
    async fn stale_request_cancellation_does_not_signal_the_replacement_worker() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = app.state::<AppState>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        let (_done_tx, done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 44,
            request_id: Some("download-b".to_string()),
            cancel: Some(cancel_tx),
            done,
        });
        let mut stale_cancellation = Box::pin(crate::commands::funasr::cancel_model_download(
            app.state::<AppState>(),
            Some("download-a".to_string()),
        ));
        let mut context = Context::from_waker(Waker::noop());

        let _ = stale_cancellation.as_mut().poll(&mut context);

        assert_eq!(
            cancel_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        );
        let slot = state.engine.download_task.lock().await;
        let replacement = slot.as_ref().expect("replacement must keep its slot");
        assert_eq!(replacement.id, 44);
        assert_eq!(replacement.request_id.as_deref(), Some("download-b"));
        assert!(
            replacement.cancel.is_some(),
            "the real owner's cancellation signal must remain available"
        );
    }

    #[tokio::test]
    async fn cancellation_can_settle_when_the_finished_worker_closes_its_done_channel() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = app.state::<AppState>();
        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 45,
            request_id: None,
            cancel: Some(cancel_tx),
            done,
        });
        let mut cancellation = Box::pin(crate::commands::funasr::cancel_model_download(
            app.state::<AppState>(),
            None,
        ));
        let mut context = Context::from_waker(Waker::noop());
        assert!(cancellation.as_mut().poll(&mut context).is_pending());
        assert_eq!(cancel_rx.try_recv(), Ok(()));

        clear_download_task(state.inner(), 45).await;
        drop(done_tx);

        assert!(matches!(
            cancellation.as_mut().poll(&mut context),
            Poll::Ready(Ok(_))
        ));
    }

    #[tokio::test]
    async fn old_download_cleanup_must_not_clear_replacement_task() {
        let state = AppState::new();
        let (old_cancel, _old_cancel_rx) = tokio::sync::oneshot::channel();
        let (_old_done_tx, old_done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 1,
            request_id: Some("old-download".to_string()),
            cancel: Some(old_cancel),
            done: old_done,
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
        let (_replacement_done_tx, replacement_done) = tokio::sync::watch::channel(false);
        *state.engine.download_task.lock().await = Some(DownloadTask {
            id: 2,
            request_id: Some("replacement-download".to_string()),
            cancel: Some(replacement_cancel),
            done: replacement_done,
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

    include!("download_worker_tests.rs");
}
