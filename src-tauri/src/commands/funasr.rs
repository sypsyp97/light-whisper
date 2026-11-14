use tauri::Emitter;

use crate::services::funasr_service;
use crate::services::llm_provider;
use crate::state::AppState;
use crate::utils::{paths, AppError};

#[tauri::command]
pub async fn set_r2t2_config(
    state: tauri::State<'_, AppState>,
    context: String,
    language: Option<String>,
) -> Result<(), AppError> {
    const LANGUAGES: &[&str] = &[
        "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru", "th", "vi", "ja",
        "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs", "fil", "fa", "el", "hu", "mk", "ro",
    ];
    let language = language
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty());
    if language
        .as_deref()
        .is_some_and(|value| !LANGUAGES.contains(&value))
    {
        return Err(AppError::Other("R2T2 不支持所选语言".into()));
    }
    crate::services::profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.r2t2.context = context.trim().to_string();
        profile.r2t2.language = language;
    });
    Ok(())
}

pub(crate) fn lock_idle_asr(state: &AppState) -> Result<tokio::sync::MutexGuard<'_, ()>, AppError> {
    let recording = state.recording.recording.lock().is_some();
    let processing = state
        .recording
        .snapshot()
        .is_some_and(|snapshot| snapshot.phase == crate::state::RecordingPhase::Processing);
    if recording || processing {
        return Err(AppError::Other(
            "录音或处理尚未结束，请稍后修改语音引擎".into(),
        ));
    }
    state
        .engine
        .native_asr_owner
        .try_lock()
        .map_err(|_| AppError::Other("语音识别正在进行，请稍后修改语音引擎".into()))
}

#[tauri::command]
pub async fn start_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器启动成功".to_string())
}

const MAX_TRANSCRIBE_AUDIO_BYTES: usize = 64 * 1024 * 1024;
const MAX_TRANSCRIBE_AUDIO_BASE64_BYTES: usize =
    MAX_TRANSCRIBE_AUDIO_BYTES.saturating_mul(4) / 3 + 4;

#[tauri::command]
pub async fn transcribe_audio(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    audio_base64: String,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    use base64::Engine;
    if audio_base64.len() > MAX_TRANSCRIBE_AUDIO_BASE64_BYTES {
        return Err(AppError::Asr(format!(
            "音频过大：base64 {} 字节超过上限 {} 字节",
            audio_base64.len(),
            MAX_TRANSCRIBE_AUDIO_BASE64_BYTES
        )));
    }
    let audio_data = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| AppError::Asr(format!("Base64 解码失败: {}", e)))?;
    if audio_data.len() > MAX_TRANSCRIBE_AUDIO_BYTES {
        return Err(AppError::Asr(format!(
            "音频过大：解码后 {} 字节超过上限 {} 字节",
            audio_data.len(),
            MAX_TRANSCRIBE_AUDIO_BYTES
        )));
    }
    funasr_service::transcribe(state.inner(), audio_data, &app_handle).await
}

#[tauri::command]
pub async fn check_funasr_status(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<funasr_service::FunASRStatus, AppError> {
    funasr_service::check_status(state.inner(), &app_handle).await
}

#[tauri::command]
pub async fn check_model_files() -> Result<funasr_service::ModelCheckResult, AppError> {
    funasr_service::check_model_files().await
}

#[tauri::command]
pub async fn download_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request_id: Option<String>,
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner(), request_id).await
}

#[tauri::command]
pub async fn cancel_model_download(
    state: tauri::State<'_, AppState>,
    request_id: Option<String>,
) -> Result<String, AppError> {
    let cancellation = {
        let mut guard = state.engine.download_task.lock().await;
        guard
            .as_mut()
            .filter(|task| {
                request_id
                    .as_ref()
                    .is_none_or(|request_id| task.request_id.as_ref() == Some(request_id))
            })
            .map(|task| (task.cancel.take(), task.done.clone()))
    };

    if let Some((cancel, mut done)) = cancellation {
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
        while !*done.borrow() {
            if done.changed().await.is_err() {
                break;
            }
        }
        Ok("已取消模型下载".to_string())
    } else {
        Ok("当前没有下载任务".to_string())
    }
}

#[tauri::command]
pub async fn restart_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let asr_guard = lock_idle_asr(state.inner())?;
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        return Ok("在线引擎状态已刷新".to_string());
    }

    if state.engine.is_funasr_starting() {
        log::info!("FunASR 正在启动中，跳过本次重启请求");
        return Ok("FunASR 正在启动中，跳过重启".to_string());
    }

    log::info!("正在重启 FunASR 服务器...");
    funasr_service::stop_server(state.inner()).await?;
    drop(asr_guard);
    drop(lifecycle_guard);
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器已重启".to_string())
}

#[tauri::command]
pub async fn get_engine() -> Result<String, AppError> {
    Ok(paths::read_engine_config())
}

const GLM_ASR_KEYRING_USER: &str = "glm-asr-api-key";
const ALIBABA_ASR_INTL_KEYRING_USER: &str = "alibaba-asr-intl-api-key";
const ALIBABA_ASR_CN_KEYRING_USER: &str = "alibaba-asr-cn-api-key";

const VALID_ONLINE_KEYRING_USERS: &[&str] = &[
    GLM_ASR_KEYRING_USER,
    ALIBABA_ASR_INTL_KEYRING_USER,
    ALIBABA_ASR_CN_KEYRING_USER,
];

pub(crate) fn active_online_keyring_user() -> &'static str {
    let engine = paths::read_engine_config();
    if engine == "alibaba-asr" {
        match paths::read_alibaba_region().as_str() {
            "domestic" => ALIBABA_ASR_CN_KEYRING_USER,
            _ => ALIBABA_ASR_INTL_KEYRING_USER,
        }
    } else {
        GLM_ASR_KEYRING_USER
    }
}

pub(crate) fn reload_online_asr_key(app_handle: &tauri::AppHandle, state: &AppState) {
    use tauri_plugin_keyring::KeyringExt;
    let user = active_online_keyring_user();
    let key = app_handle
        .keyring()
        .get_password("light-whisper", user)
        .ok()
        .flatten()
        .unwrap_or_default();
    state.set_online_asr_api_key(&key);
}

fn online_status_payload(engine: &str, has_key: bool) -> serde_json::Value {
    let label = match engine {
        "alibaba-asr" => "Alibaba DashScope",
        _ => "GLM-ASR",
    };
    serde_json::json!({
        "status": if has_key { "ready" } else { "need_api_key" },
        "message": if has_key {
            format!("{} 在线服务就绪", label)
        } else {
            format!("请配置 {} API Key", label)
        },
        "device": "cloud",
        "gpu_name": serde_json::Value::Null,
        "models_present": true,
        "missing_models": [],
    })
}

#[tauri::command]
pub async fn set_engine(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    engine: String,
) -> Result<String, AppError> {
    const VALID: &[&str] = &[
        "qwen3-asr-0.6b",
        "confucius4-r2t2",
        "glm-asr",
        "alibaba-asr",
    ];
    if !VALID.contains(&engine.as_str()) {
        return Err(AppError::Other(format!(
            "不支持的引擎类型: {}，可选值: {}",
            engine,
            VALID.join(", ")
        )));
    }

    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let _asr_guard = lock_idle_asr(state.inner())?;
    if state.engine.download_task.lock().await.is_some() {
        return Err(AppError::Other(
            "模型正在下载，请等待完成或取消下载后再切换引擎".to_string(),
        ));
    }

    paths::write_engine_config(&engine)
        .map_err(|e| AppError::Other(format!("写入引擎配置失败: {}", e)))?;
    state.engine.block_funasr_starting();
    let switch_result: Result<(), AppError> = async {
        funasr_service::stop_server(state.inner()).await?;

        if paths::is_online_engine(&engine) {
            reload_online_asr_key(&app_handle, state.inner());
            let has_key = !state.read_online_asr_api_key().is_empty();
            state.set_funasr_ready(has_key);
            let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
        }
        Ok(())
    }
    .await;
    state.engine.unblock_funasr_starting();
    switch_result?;

    app_handle.emit("engine-config-changed", &engine)?;
    log::info!("引擎已切换为: {}", engine);
    Ok(engine)
}

#[tauri::command]
pub async fn get_gpu_idle_seconds(include_disabled: Option<bool>) -> Result<u64, AppError> {
    Ok(if include_disabled == Some(true) {
        paths::read_gpu_idle_timeout_seconds()
    } else {
        paths::read_gpu_idle_seconds()
    })
}

#[tauri::command]
pub async fn set_gpu_idle_seconds(
    state: tauri::State<'_, AppState>,
    seconds: u64,
    timeout_seconds: Option<u64>,
) -> Result<u64, AppError> {
    let requested_seconds = seconds.max(timeout_seconds.unwrap_or(0));
    if requested_seconds > paths::MAX_GPU_IDLE_SECONDS {
        return Err(AppError::Other(format!(
            "GPU 空闲时间过长: {}，最大 {} 秒",
            requested_seconds,
            paths::MAX_GPU_IDLE_SECONDS
        )));
    }
    paths::write_gpu_idle_seconds(seconds, timeout_seconds)
        .map_err(|e| AppError::Other(format!("写入 GPU 空闲设置失败: {}", e)))?;

    funasr_service::push_gpu_idle_seconds(state.inner(), seconds).await?;
    Ok(seconds)
}

#[tauri::command]
pub async fn get_gpu_backend() -> Result<String, AppError> {
    Ok(paths::read_gpu_backend())
}

#[tauri::command]
pub async fn set_gpu_backend(backend: String) -> Result<String, AppError> {
    let normalized = paths::parse_gpu_backend(Some(&serde_json::json!(backend)));
    paths::write_gpu_backend(&normalized)
        .map_err(|e| AppError::Other(format!("写入 GPU 后端设置失败: {}", e)))?;
    Ok(normalized)
}

#[tauri::command]
pub async fn list_gpu_backends() -> Result<Vec<String>, AppError> {
    Ok(paths::read_installed_gpu_backends())
}

#[tauri::command]
pub async fn get_audio_processing_enabled() -> Result<bool, AppError> {
    Ok(paths::read_audio_processing_enabled())
}

#[tauri::command]
pub async fn set_audio_processing_enabled(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, AppError> {
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let _asr_guard = lock_idle_asr(state.inner())?;
    let previous = paths::read_audio_processing_enabled();
    funasr_service::push_audio_processing_enabled(state.inner(), enabled).await?;
    if let Err(error) = paths::write_audio_processing_enabled(enabled) {
        if let Err(rollback) =
            funasr_service::push_audio_processing_enabled(state.inner(), previous).await
        {
            log::error!("音频处理设置回滚失败: {rollback}");
        }
        return Err(AppError::Other(format!("保存音频处理设置失败: {error}")));
    }
    Ok(enabled)
}

#[tauri::command]
pub async fn set_online_asr_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
    keyring_user: Option<String>,
) -> Result<(), AppError> {
    let active_user = active_online_keyring_user();
    let target_user: &str = match keyring_user.as_deref() {
        Some(user) if VALID_ONLINE_KEYRING_USERS.contains(&user) => user,
        _ => active_user,
    };

    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    llm_provider::save_or_delete_api_key(&app_handle, target_user, &api_key)
        .map_err(AppError::Other)?;
    let current_active_user = active_online_keyring_user();

    if target_user == current_active_user {
        state.set_online_asr_api_key(&api_key);
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            let has_key = !api_key.is_empty();
            state.set_funasr_ready(has_key);
            let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_online_asr_api_key(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    Ok(state.read_online_asr_api_key())
}

#[tauri::command]
pub async fn get_online_asr_endpoint() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn set_online_asr_endpoint(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    region: String,
) -> Result<serde_json::Value, AppError> {
    if region != "international" && region != "domestic" {
        return Err(AppError::Other(format!(
            "不支持的区域: {}，可选值: international, domestic",
            region
        )));
    }
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    paths::write_online_asr_endpoint(&region)
        .map_err(|e| AppError::Other(format!("写入端点配置失败: {}", e)))?;

    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        reload_online_asr_key(&app_handle, state.inner());
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
    }

    Ok(serde_json::json!({
        "region": paths::read_online_asr_region(),
        "url": paths::read_online_asr_endpoint(),
    }))
}

#[tauri::command]
pub async fn get_alibaba_asr_config() -> Result<serde_json::Value, AppError> {
    Ok(serde_json::json!({
        "region": paths::read_alibaba_region(),
        "url": paths::read_alibaba_endpoint(),
        "model": paths::read_alibaba_model(),


        "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
    }))
}

#[tauri::command]
pub async fn set_alibaba_asr_model(model: String) -> Result<serde_json::Value, AppError> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("模型 ID 不能为空".into()));
    }

    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(AppError::Other(format!(
            "非法模型 ID: {}，仅允许字母数字与 .-_",
            trimmed
        )));
    }
    paths::write_alibaba_model(trimmed)
        .map_err(|e| AppError::Other(format!("写入模型配置失败: {}", e)))?;
    Ok(serde_json::json!({
        "model": paths::read_alibaba_model(),
    }))
}

#[tauri::command]
pub async fn list_alibaba_asr_models(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, AppError> {
    #[derive(serde::Deserialize)]
    struct ModelsResponse {
        data: Option<Vec<ModelEntry>>,
    }
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: Option<String>,
    }

    let (api_key, base) = {
        let _configuration = state.engine.funasr_lifecycle_op.lock().await;
        use tauri_plugin_keyring::KeyringExt;
        let key_slot = if paths::read_alibaba_region() == "domestic" {
            ALIBABA_ASR_CN_KEYRING_USER
        } else {
            ALIBABA_ASR_INTL_KEYRING_USER
        };
        let api_key = app_handle
            .keyring()
            .get_password(llm_provider::KEYRING_SERVICE, key_slot)
            .map_err(|_| {
                AppError::Other("读取语音识别 API Key 失败，请检查系统凭据后重试。".into())
            })?
            .unwrap_or_default();
        (api_key, paths::read_alibaba_endpoint())
    };

    let fallback = |failed: bool| {
        serde_json::json!({
            "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
            "source": "fallback",
            "warning": if failed { Some("语音识别模型目录刷新失败，当前显示内置列表，请检查连接或 API Key 后重试。") } else { None },
        })
    };

    if api_key.is_empty() {
        return Ok(fallback(false));
    }

    let url = format!("{}/compatible-mode/v1/models", base);
    let resp_result = state
        .http_client
        .get(&url)
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let Ok(resp) = resp_result else {
        log::warn!(
            "抓取 DashScope 模型列表失败 (network)：{:?}",
            resp_result.err()
        );
        return Ok(fallback(true));
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!(
            "DashScope /v1/models HTTP {}（响应{}字符）",
            status,
            body.chars().count()
        );
        return Ok(fallback(true));
    }

    let parsed: ModelsResponse = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("解析 DashScope /v1/models 响应失败: {}", e);
            return Ok(fallback(true));
        }
    };

    let mut ids: Vec<String> = parsed
        .data
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e.id)
        .filter(|id| paths::is_asr_capable_model_id(id))
        .collect();

    if ids.is_empty() {
        return Ok(fallback(true));
    }

    ids.sort();
    ids.dedup();
    if let Some(pos) = ids.iter().position(|id| id == paths::ALIBABA_DEFAULT_MODEL) {
        let item = ids.remove(pos);
        ids.insert(0, item);
    }

    Ok(serde_json::json!({
        "models": ids,
        "source": "live",
    }))
}

#[tauri::command]
pub async fn get_models_dir() -> Result<serde_json::Value, AppError> {
    let effective = paths::strip_win_prefix(&paths::get_effective_models_dir());
    let is_custom = paths::read_models_dir().is_some();
    Ok(serde_json::json!({
        "path": effective,
        "is_custom": is_custom,
    }))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsDirUpdateResult {
    message: String,
    runtime_warning: Option<String>,
}

fn models_dir_update_result(
    restore_default: bool,
    runtime_warning: Option<String>,
) -> ModelsDirUpdateResult {
    ModelsDirUpdateResult {
        message: if restore_default {
            "已恢复默认模型目录".to_string()
        } else {
            "模型目录已更新".to_string()
        },
        runtime_warning,
    }
}

#[tauri::command]
pub async fn pick_folder(window: tauri::WebviewWindow) -> Result<Option<String>, AppError> {
    let result = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .pick_folder()
        .await;
    Ok(result.map(|folder| paths::strip_win_prefix(folder.path())))
}

#[tauri::command]
pub async fn set_models_dir(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: Option<String>,
    migrate: bool,
) -> Result<ModelsDirUpdateResult, AppError> {
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let _asr_guard = lock_idle_asr(state.inner())?;
    if state.engine.download_task.lock().await.is_some() {
        return Err(AppError::Other(
            "模型正在下载，请等待完成或取消下载后再切换目录".to_string(),
        ));
    }
    let old_dir = paths::get_effective_models_dir();
    let restore_default = path.as_deref().is_none_or(|value| value.trim().is_empty());
    let new_dir = match &path {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
        _ => paths::get_default_models_dir(),
    };

    let canon_old = std::fs::canonicalize(&old_dir).unwrap_or_else(|_| old_dir.clone());

    let canon_new = std::fs::canonicalize(&new_dir).unwrap_or_else(|_| new_dir.clone());
    if canon_old == canon_new {
        if restore_default {
            paths::write_models_dir(None)
                .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;
            return Ok(models_dir_update_result(true, None));
        }
        return Ok(ModelsDirUpdateResult {
            message: "路径未变化".to_string(),
            runtime_warning: None,
        });
    }

    let new_dir_clone = new_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&new_dir_clone))
        .await
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?;

    let engine = paths::read_engine_config();
    let local_engine = !paths::is_online_engine(&engine);
    let prepared_sources = if migrate && old_dir.is_dir() {
        let _ = app_handle.emit(
            "models-migrate-status",
            serde_json::json!({ "status": "migrating", "message": "正在复制模型文件..." }),
        );

        let old = old_dir.clone();
        let dest = new_dir.clone();
        let handle = app_handle.clone();
        tokio::task::spawn_blocking(move || migrate_model_dirs(&old, &dest, Some(&handle)))
            .await
            .map_err(|e| AppError::Other(format!("迁移任务失败: {}", e)))?
            .map_err(|e| AppError::Other(format!("迁移失败: {}", e)))?
    } else {
        Vec::new()
    };

    let dir_str = paths::strip_win_prefix(&new_dir);
    paths::write_models_dir((!restore_default).then_some(dir_str.as_str()))
        .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;

    state.engine.block_funasr_starting();
    let update_result: Result<bool, AppError> = async {
        if local_engine {
            funasr_service::stop_server(state.inner()).await?;
        }
        let should_start = if local_engine {
            let model_check = funasr_service::check_model_files().await?;
            if !model_check.all_present {
                state.set_funasr_ready(false);
            }
            model_check.all_present
        } else {
            false
        };

        Ok(should_start)
    }
    .await;
    state.engine.unblock_funasr_starting();
    let mut runtime_warning = None;
    let should_start = match update_result {
        Ok(should_start) => should_start,
        Err(err) => {
            state.set_funasr_ready(false);
            runtime_warning = Some(format!("目录配置已保存，但运行时更新失败: {}", err));
            false
        }
    };
    drop(lifecycle_guard);

    if should_start {
        if let Err(err) = funasr_service::start_server(&app_handle, state.inner()).await {
            state.set_funasr_ready(false);
            runtime_warning = Some(format!("目录配置已保存，但服务启动失败: {}", err));
        } else if !prepared_sources.is_empty() {
            let _cleanup_guard = state.engine.funasr_lifecycle_op.lock().await;
            let effective_dir = paths::get_effective_models_dir();
            let canonical_effective =
                std::fs::canonicalize(&effective_dir).unwrap_or(effective_dir.clone());
            let canonical_target = std::fs::canonicalize(&new_dir).unwrap_or(new_dir.clone());
            if canonical_effective == canonical_target {
                match tokio::task::spawn_blocking(move || {
                    cleanup_migrated_sources(&prepared_sources)
                })
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        runtime_warning =
                            Some(format!("模型目录已切换，但部分旧目录未能清理: {}", err));
                    }
                    Err(err) => {
                        runtime_warning =
                            Some(format!("模型目录已切换，但旧目录清理任务异常: {}", err));
                    }
                }
            } else {
                log::info!(
                    "模型目录已再次变更，跳过清理旧源: current={}, expected={}",
                    canonical_effective.display(),
                    canonical_target.display()
                );
            }
        }
    }

    let result = models_dir_update_result(restore_default, runtime_warning);
    let _ = app_handle.emit(
        "models-migrate-status",
        serde_json::json!({
            "status": "completed",
            "message": result.runtime_warning.as_deref().unwrap_or(&result.message),
        }),
    );

    Ok(result)
}

fn migrate_model_dirs(
    src: &std::path::Path,
    dst: &std::path::Path,
    handle: Option<&tauri::AppHandle>,
) -> Result<Vec<std::path::PathBuf>, String> {
    let canonical_src = std::fs::canonicalize(src)
        .map_err(|e| format!("解析源模型目录失败 {}: {}", src.display(), e))?;
    let canonical_dst = std::fs::canonicalize(dst)
        .map_err(|e| format!("解析目标模型目录失败 {}: {}", dst.display(), e))?;
    if canonical_dst.starts_with(&canonical_src) {
        return Err("目标模型目录不能位于当前模型目录内部".to_string());
    }

    let entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| format!("读取源目录失败: {}", e))?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("models--") && e.path().is_dir())
        .collect();

    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let total = entries.len();
    let mut copied_sources = Vec::with_capacity(total);

    for (i, entry) in entries.iter().enumerate() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        copy_dir_recursive(&src_path, &dst_path)
            .map_err(|e| format!("复制 {} 失败: {}", name.to_string_lossy(), e))?;
        copied_sources.push(src_path);
        log::info!("迁移准备（copy，保留源）: {}", name.to_string_lossy());

        if let Some(handle) = handle {
            let _ = handle.emit(
                "models-migrate-status",
                serde_json::json!({
                    "status": "migrating",
                    "message": format!("正在迁移 {}/{}...", i + 1, total),
                    "progress": ((i + 1) as f64 / total as f64 * 100.0).round(),
                }),
            );
        }
    }

    Ok(copied_sources)
}

fn cleanup_migrated_sources(sources: &[std::path::PathBuf]) -> Result<(), String> {
    let mut failures = Vec::new();
    for source in sources {
        if let Err(e) = std::fs::remove_dir_all(source) {
            log::warn!(
                "清理源目录失败（不影响迁移结果）: {} — {}",
                source.display(),
                e
            );
            failures.push(format!("{} — {}", source.display(), e));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod model_dir_migration_tests {
    use super::{cleanup_migrated_sources, migrate_model_dirs, models_dir_update_result};

    #[test]
    fn model_dir_result_distinguishes_committed_config_from_runtime_warning() {
        let result =
            models_dir_update_result(false, Some("目录配置已保存，但服务启动失败".to_string()));
        let value = serde_json::to_value(result).expect("serialize model directory result");

        assert_eq!(value["message"], "模型目录已更新");
        assert_eq!(value["runtimeWarning"], "目录配置已保存，但服务启动失败");
    }

    #[test]
    fn cleanup_failure_is_reported_to_the_committed_result() {
        let missing = std::env::temp_dir().join(format!(
            "light-whisper-missing-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));

        let error = cleanup_migrated_sources(std::slice::from_ref(&missing))
            .expect_err("a cleanup failure must reach the structured result");

        assert!(error.contains(&missing.display().to_string()));
    }

    #[test]
    fn migration_prepare_keeps_source_until_config_commit() {
        let unique = format!(
            "light-whisper-model-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let source = root.join("source");
        let target = root.join("target");
        let model_source = source.join("models--org--model");
        std::fs::create_dir_all(&model_source).expect("create source model directory");
        std::fs::write(model_source.join("weights.bin"), b"weights").expect("write source model");
        std::fs::create_dir_all(&target).expect("create target directory");

        migrate_model_dirs(&source, &target, None).expect("prepare migration");

        assert!(
            model_source.join("weights.bin").is_file(),
            "prepare must keep source data until the new config is committed"
        );
        assert_eq!(
            std::fs::read(target.join("models--org--model").join("weights.bin"))
                .expect("read copied model"),
            b"weights"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn migration_rejects_target_nested_inside_source() {
        let root = std::env::temp_dir().join(format!(
            "light-whisper-nested-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        let source = root.join("source");
        let target = source.join("nested-target");
        let model_source = source.join("models--org--model");
        std::fs::create_dir_all(&model_source).expect("create source model directory");
        std::fs::write(model_source.join("weights.bin"), b"weights").expect("write source model");
        std::fs::create_dir_all(&target).expect("create nested target");

        let error = migrate_model_dirs(&source, &target, None)
            .expect_err("nested migration target must be rejected");

        assert!(error.contains("不能位于"));
        assert!(model_source.join("weights.bin").is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
