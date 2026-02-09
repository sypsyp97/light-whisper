//! FunASR 命令模块
//!
//! 这个模块把 `funasr_service` 中的服务函数包装成 Tauri 命令，
//! 使前端可以通过 `invoke` 调用。
//!
//! # Rust 知识点：tauri::State
//! `tauri::State<'_, AppState>` 是 Tauri 的依赖注入机制。
//! 在 `lib.rs` 中通过 `.manage(AppState::new())` 注册状态后，
//! 任何 Tauri 命令都可以通过参数自动获取状态的引用。
//!
//! `'_` 是一个生命周期参数，这里让编译器自动推断。
//! 生命周期保证引用在使用期间一直有效（不会出现悬垂引用）。

use crate::services::funasr_service;
use crate::state::AppState;
use crate::utils::AppError;

/// 启动 FunASR 服务器
///
/// 查找 Python 解释器并启动 FunASR 语音识别服务。
/// 启动过程可能需要 1-2 分钟（首次加载模型时更久）。
///
/// # 前端调用示例
/// ```javascript
/// try {
///     await invoke('start_funasr');
///     console.log('FunASR 已启动');
/// } catch (error) {
///     console.error('启动失败:', error);
/// }
/// ```
#[tauri::command]
pub async fn start_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    // `state.inner()` 获取内部的 AppState 引用
    // Tauri 的 State 包裹器提供了线程安全的访问
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器启动成功".to_string())
}

/// 执行语音转写
///
/// 将录制的音频数据发送给 FunASR 进行语音识别。
///
/// # 参数
/// - `audio_data`：WAV 格式的音频数据（Base64 编码的字节数组）
///
/// # Rust 知识点：Vec<u8>
/// 前端传来的音频数据是一个字节数组。
/// Tauri 会自动把前端的 `Uint8Array` 或 `number[]` 转成 `Vec<u8>`。
///
/// # 前端调用示例
/// ```javascript
/// const audioBlob = await recorder.stop();
/// const arrayBuffer = await audioBlob.arrayBuffer();
/// const audioData = Array.from(new Uint8Array(arrayBuffer));
/// const result = await invoke('transcribe_audio', { audioData });
/// console.log('转写结果:', result.text);
/// ```
#[tauri::command]
pub async fn transcribe_audio(
    state: tauri::State<'_, AppState>,
    audio_data: Vec<u8>,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    funasr_service::transcribe(state.inner(), audio_data).await
}

/// 检查 FunASR 服务器的状态
///
/// 返回服务器是否正在运行、是否就绪、模型是否已加载等信息。
#[tauri::command]
pub async fn check_funasr_status(
    state: tauri::State<'_, AppState>,
) -> Result<funasr_service::FunASRStatus, AppError> {
    funasr_service::check_status(state.inner()).await
}

/// 检查模型文件是否已下载
///
/// 检查 FunASR 所需的三个模型文件是否都已经下载到本地缓存。
/// 前端可以根据结果决定是否需要先下载模型。
#[tauri::command]
pub async fn check_model_files(
) -> Result<funasr_service::ModelCheckResult, AppError> {
    funasr_service::check_model_files().await
}

/// 下载 FunASR 模型
///
/// 启动 Python 脚本来下载 FunASR 所需的语音识别模型。
/// 模型文件较大，下载可能需要一些时间。
///
/// # 流程
/// 1. 查找可用的 Python 解释器
/// 2. 运行下载脚本
/// 3. 通过事件通知前端下载进度
///
/// # Rust 知识点：spawn 和 await
/// `spawn` 启动子进程但不等待完成。
/// `wait().await` 异步等待子进程结束。
/// 这样在等待下载时不会阻塞 UI 线程。
#[tauri::command]
pub async fn download_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    use crate::utils::paths;
    use tauri::Emitter;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;
    use tokio::sync::oneshot;

    // 查找 Python
    let python_path = funasr_service::find_python().await?;

    // 获取下载脚本路径，清理 Windows \\?\ 前缀
    let download_script = paths::get_download_script_path(&app_handle);
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
            return Err(AppError::FunASR("已有下载任务正在进行，请先取消或等待完成".to_string()));
        }
        *guard = Some(crate::state::DownloadTask {
            cancel: cancel_tx,
        });
    }

    // 通知前端开始下载
    let _ = app_handle.emit("model-download-status", serde_json::json!({
        "status": "downloading",
        "message": "开始下载模型文件..."
    }));

    // 启动下载脚本（逐行读取 stdout 以转发进度）
    // 模型从 HuggingFace 下载，使用 HF 默认缓存目录
    let mut child = match Command::new(&python_path)
        .arg("-u")
        .arg(&download_script_str)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let mut guard = state.download_task.lock().await;
            guard.take();
            return Err(AppError::FunASR(format!("启动模型下载脚本失败: {}", e)));
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let mut guard = state.download_task.lock().await;
            guard.take();
            return Err(AppError::FunASR("无法读取模型下载脚本输出".to_string()));
        }
    };


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
                let _ = app_handle.emit("model-download-status", serde_json::json!({
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

                    let _ = app_handle.emit("model-download-status", serde_json::json!({
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
            let mut guard = state.download_task.lock().await;
            guard.take();
            return Err(AppError::FunASR(format!("模型下载进程异常退出: {}", e)));
        }
    };

    let final_success = final_result
        .as_ref()
        .and_then(|r| r.success)
        .unwrap_or(status.success());

    // 清理下载任务
    {
        let mut guard = state.download_task.lock().await;
        guard.take();
    }

    if let Some(err) = read_error {
        return Err(err);
    }

    if cancelled {
        return Ok("模型下载已取消".to_string());
    }

    if final_success {
        let _ = app_handle.emit("model-download-status", serde_json::json!({
            "status": "completed",
            "progress": 100,
            "message": "模型下载完成"
        }));
        Ok("模型下载完成".to_string())
    } else {
        let error_msg = final_result
            .and_then(|r| r.error.or(r.message))
            .unwrap_or_else(|| "模型下载失败".to_string());

        let _ = app_handle.emit("model-download-status", serde_json::json!({
            "status": "error",
            "message": &error_msg
        }));

        Err(AppError::FunASR(error_msg))
    }
}

/// 取消模型下载任务
#[tauri::command]
pub async fn cancel_model_download(
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let task = {
        let mut guard = state.download_task.lock().await;
        guard.take()
    };

    if let Some(task) = task {
        let _ = task.cancel.send(());
        Ok("已取消模型下载".to_string())
    } else {
        Ok("当前没有下载任务".to_string())
    }
}

/// 重启 FunASR 服务器
///
/// 先停止当前运行的服务器，等待一秒后重新启动。
/// 在服务器出现异常时可以用来恢复。
#[tauri::command]
pub async fn restart_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    log::info!("正在重启 FunASR 服务器...");

    // 先停止现有服务器
    funasr_service::stop_server(state.inner()).await?;

    // 等待 1 秒确保资源释放
    // `tokio::time::sleep` 是异步的 sleep，不会阻塞线程
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // 重新启动
    funasr_service::start_server(&app_handle, state.inner()).await?;

    Ok("FunASR 服务器已重启".to_string())
}

/// 停止 FunASR 服务器
///
/// 优雅地关闭 FunASR 服务。通常在应用退出前调用。
#[tauri::command]
pub async fn stop_funasr(
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    funasr_service::stop_server(state.inner()).await?;
    Ok("FunASR 服务器已停止".to_string())
}
