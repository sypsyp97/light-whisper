//! 模型下载服务
//!
//! 从 commands/funasr.rs 中提取的模型下载逻辑。

use crate::services::funasr_service;
use crate::state::AppState;
use crate::utils::{paths, AppError};
use std::process::Stdio;
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

async fn clear_download_task(state: &AppState) {
    let mut guard = state.download_task.lock().await;
    guard.take();
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
    // 查找引擎运行时
    let runtime = funasr_service::find_engine(app_handle).await?;

    // 获取下载脚本路径，清理 Windows \\?\ 前缀
    let download_script = paths::get_download_script_path(app_handle);
    let download_script_str = paths::strip_win_prefix(&download_script);

    // 仅开发模式需要检查脚本是否存在
    if matches!(runtime, funasr_service::EngineRuntime::Development { .. }) && !download_script.exists() {
        return Err(AppError::Download(format!(
            "模型下载脚本不存在: {}",
            download_script_str
        )));
    }

    let data_dir = paths::strip_win_prefix(&paths::get_data_dir());

    let (cancel_tx, mut cancel_rx) = oneshot::channel();
    {
        // 防止重复下载
        let mut guard = state.download_task.lock().await;
        if guard.is_some() {
            return Err(AppError::Download(
                "已有下载任务正在进行，请先取消或等待完成".to_string(),
            ));
        }
        *guard = Some(crate::state::DownloadTask { cancel: cancel_tx });
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
    let engine_arg = if engine == "whisper" { "whisper" } else { "sensevoice" };

    let mut cmd = match &runtime {
        funasr_service::EngineRuntime::Bundled { exe_path } => {
            let mut c = Command::new(exe_path);
            c.arg("download").arg("--engine").arg(engine_arg);
            c
        }
        funasr_service::EngineRuntime::Development { python_path } => {
            let mut c = Command::new(python_path);
            c.arg("-u").arg(&download_script_str).arg("--engine").arg(engine_arg);
            c
        }
    };

    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
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
            clear_download_task(state).await;
            return Err(AppError::Download(format!("启动模型下载脚本失败: {}", e)));
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            clear_download_task(state).await;
            return Err(AppError::Download("无法读取模型下载脚本输出".to_string()));
        }
    };

    let mut reader = BufReader::new(stdout);
    let mut final_result: Option<DownloadLine> = None;
    let mut cancelled = false;
    loop {
        let mut line = String::new();
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
            bytes = reader.read_line(&mut line) => {
                let bytes = match bytes {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        // 非 UTF-8 输出（如 tqdm 进度条）跳过，不终止下载
                        log::warn!("下载输出解码错误（跳过）: {}", e);
                        continue;
                    }
                };
                if bytes == 0 {
                    break;
                }

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(payload) = serde_json::from_str::<DownloadLine>(trimmed) {
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
    }

    let status = match child.wait().await {
        Ok(status) => status,
        Err(e) => {
            clear_download_task(state).await;
            return Err(AppError::Download(format!("模型下载进程异常退出: {}", e)));
        }
    };

    let final_success = final_result
        .as_ref()
        .and_then(|r| r.success)
        .unwrap_or(status.success());

    // 清理下载任务
    clear_download_task(state).await;

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
