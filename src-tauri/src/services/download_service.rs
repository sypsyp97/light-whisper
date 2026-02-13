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
    // 查找 Python
    let python_path = funasr_service::find_python().await?;

    // 获取下载脚本路径，清理 Windows \\?\ 前缀
    let download_script = paths::get_download_script_path(app_handle);
    let download_script_str = paths::strip_win_prefix(&download_script);

    if !download_script.exists() {
        return Err(AppError::FunASR(format!(
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
            return Err(AppError::FunASR(
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
    let mut child = match Command::new(&python_path)
        .arg("-u")
        .arg(&download_script_str)
        .arg("--engine")
        .arg(if engine == "whisper" {
            "whisper"
        } else {
            "sensevoice"
        })
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            clear_download_task(state).await;
            return Err(AppError::FunASR(format!("启动模型下载脚本失败: {}", e)));
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            clear_download_task(state).await;
            return Err(AppError::FunASR("无法读取模型下载脚本输出".to_string()));
        }
    };

    let mut reader = BufReader::new(stdout);
    let mut final_result: Option<DownloadLine> = None;
    let mut cancelled = false;
    let mut read_error: Option<AppError> = None;

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
                        read_error = Some(AppError::FunASR(format!("读取模型下载输出失败: {}", e)));
                        break;
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
            return Err(AppError::FunASR(format!("模型下载进程异常退出: {}", e)));
        }
    };

    let final_success = final_result
        .as_ref()
        .and_then(|r| r.success)
        .unwrap_or(status.success());

    // 清理下载任务
    clear_download_task(state).await;

    if let Some(err) = read_error {
        return Err(err);
    }

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

        Err(AppError::FunASR(error_msg))
    }
}
