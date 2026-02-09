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
/// - `audio_base64`：WAV 格式的音频数据（Base64 编码的字符串）
///
/// # 前端调用示例
/// ```javascript
/// const audioBlob = await recorder.stop();
/// const arrayBuffer = await audioBlob.arrayBuffer();
/// const base64 = btoa(String.fromCharCode(...new Uint8Array(arrayBuffer)));
/// const result = await invoke('transcribe_audio', { audioBase64: base64 });
/// console.log('转写结果:', result.text);
/// ```
#[tauri::command]
pub async fn transcribe_audio(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    audio_base64: String,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    use base64::Engine;
    let audio_data = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| AppError::FunASR(format!("Base64 解码失败: {}", e)))?;
    funasr_service::transcribe(state.inner(), audio_data, &app_handle).await
}

/// 检查 FunASR 服务器的状态
///
/// 返回服务器是否正在运行、是否就绪、模型是否已加载等信息。
#[tauri::command]
pub async fn check_funasr_status(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<funasr_service::FunASRStatus, AppError> {
    funasr_service::check_status(state.inner(), &app_handle).await
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
#[tauri::command]
pub async fn download_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner()).await
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
