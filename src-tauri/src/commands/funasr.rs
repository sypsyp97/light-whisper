use std::sync::atomic::Ordering;
use tauri::Emitter;

use crate::services::funasr_service;
use crate::services::llm_provider;
use crate::state::AppState;
use crate::utils::{paths, AppError};

#[tauri::command]
pub async fn start_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    funasr_service::start_server(&app_handle, state.inner()).await?;
    Ok("FunASR 服务器启动成功".to_string())
}

#[tauri::command]
pub async fn transcribe_audio(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    audio_base64: String,
) -> Result<funasr_service::TranscriptionResult, AppError> {
    use base64::Engine;
    let audio_data = base64::engine::general_purpose::STANDARD
        .decode(&audio_base64)
        .map_err(|e| AppError::Asr(format!("Base64 解码失败: {}", e)))?;
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
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn cancel_model_download(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
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

#[tauri::command]
pub async fn restart_funasr(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, AppError> {
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        // 在线引擎无需重启 Python，仅刷新就绪状态
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        return Ok("在线引擎状态已刷新".to_string());
    }

    // 首次启动可能仍在解压 engine.zip 或加载模型，自动重启不应打断这一过程。
    if state.funasr_starting.load(Ordering::SeqCst) {
        log::info!("FunASR 正在启动中，跳过本次重启请求");
        return Ok("FunASR 正在启动中，跳过重启".to_string());
    }

    log::info!("正在重启 FunASR 服务器...");
    funasr_service::stop_server(state.inner()).await?;
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

/// 计算当前活跃在线引擎对应的 keyring user 名称。
///
/// GLM 全区共享一个 entry（沿用历史行为），Alibaba 按 CN / Intl 分开存，
/// 因为 Alibaba Cloud 域外与域内是两个独立控制台、两份独立的 DashScope API Key。
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

/// 从密钥环加载 active online provider 的 API Key 到运行时缓存。
pub(crate) fn reload_online_asr_key(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) {
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
    const VALID: &[&str] = &["sensevoice", "whisper", "glm-asr", "alibaba-asr"];
    if !VALID.contains(&engine.as_str()) {
        return Err(AppError::Other(format!(
            "不支持的引擎类型: {}，可选值: {}",
            engine,
            VALID.join(", ")
        )));
    }

    paths::write_engine_config(&engine)
        .map_err(|e| AppError::Other(format!("写入引擎配置失败: {}", e)))?;
    funasr_service::stop_server(state.inner()).await?;

    // 强制重置启动标志，确保新引擎可以立即启动。
    state
        .funasr_starting
        .store(false, std::sync::atomic::Ordering::SeqCst);

    // 在线引擎：切换后从密钥环重新加载对应的 API Key，然后刷新就绪状态。
    if paths::is_online_engine(&engine) {
        reload_online_asr_key(&app_handle, state.inner());
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        let _ = app_handle.emit("funasr-status", online_status_payload(&engine, has_key));
    }

    log::info!("引擎已切换为: {}", engine);
    Ok(engine)
}

#[tauri::command]
pub async fn set_online_asr_api_key(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_key: String,
    keyring_user: Option<String>,
) -> Result<(), AppError> {
    // 调用方可以显式指定 keyring 槽，用于避免 debounce 与 set_engine 的 race：
    // 用户在 GLM 输入框里打字→切到 Alibaba，debounced save 就在切换之后落地。
    // 此时按"当前活跃引擎"算槽会把 GLM 的 key 写进 Alibaba 的 entry。把槽由
    // 发起方传入，就能锁定键入瞬间的槽位。
    let active_user = active_online_keyring_user();
    let target_user: &str = match keyring_user.as_deref() {
        Some(user) if VALID_ONLINE_KEYRING_USERS.contains(&user) => user,
        _ => active_user,
    };

    llm_provider::save_or_delete_api_key(&app_handle, target_user, &api_key);

    // 只有当目标槽和当前活跃槽一致时，才更新运行时缓存与就绪状态，
    // 否则会把一个不相关的 key 写进 state 造成状态与 UI 不一致。
    if target_user == active_user {
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
    paths::write_online_asr_endpoint(&region)
        .map_err(|e| AppError::Other(format!("写入端点配置失败: {}", e)))?;

    // 区域切换对 Alibaba 意味着换了 API Key 来源；重新加载缓存并刷新就绪状态。
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
        // 兜底模型列表：用户没填 key 时也能看到一份可选集。拿到 key 后前端会调
        // list_alibaba_asr_models 刷新为运行时抓取结果。
        "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
    }))
}

#[tauri::command]
pub async fn set_alibaba_asr_model(model: String) -> Result<serde_json::Value, AppError> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err(AppError::Other("模型 ID 不能为空".into()));
    }
    // 轻量白名单：模型 ID 只允许 ascii 字母数字以及 `.-_`，防止被注入到 JSON body
    // 里产生奇怪的 URL / 字段。DashScope 的模型命名规范本身就在这个范围内。
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

/// 调 DashScope `/compatible-mode/v1/models` 拉取账号可见模型列表，过滤出
/// 能做语音转文字的 ASR / Omni 家族；抓取失败则回退到静态白名单。
///
/// 返回：`{ models: [...], source: "live" | "fallback" }`。`source = "live"`
/// 表示这份列表来自真实 API 响应，`"fallback"` 表示网络或鉴权失败，前端可据此
/// 决定是否要提示用户。
#[tauri::command]
pub async fn list_alibaba_asr_models(
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

    let api_key = state.read_online_asr_api_key();
    let base = paths::read_alibaba_endpoint();

    let fallback = || {
        serde_json::json!({
            "models": paths::ALIBABA_FALLBACK_MODEL_IDS,
            "source": "fallback",
        })
    };

    if api_key.is_empty() {
        return Ok(fallback());
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
        log::warn!("抓取 DashScope 模型列表失败 (network)：{:?}", resp_result.err());
        return Ok(fallback());
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!("DashScope /v1/models HTTP {}: {}", status, body);
        return Ok(fallback());
    }

    let parsed: ModelsResponse = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("解析 DashScope /v1/models 响应失败: {}", e);
            return Ok(fallback());
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
        return Ok(fallback());
    }

    // 稳定排序 + 去重，并把默认模型 qwen3-asr-flash 顶到第一位，其它按字典序。
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

#[tauri::command]
pub async fn pick_folder() -> Result<Option<String>, AppError> {
    let result = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder())
        .await
        .map_err(|e| AppError::Other(format!("文件夹选择失败: {}", e)))?;
    Ok(result.map(|p| paths::strip_win_prefix(&p)))
}

#[tauri::command]
pub async fn set_models_dir(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: Option<String>,
    migrate: bool,
) -> Result<String, AppError> {
    let old_dir = paths::get_effective_models_dir();
    let new_dir = match &path {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p.trim()),
        _ => {
            // 恢复默认
            paths::write_models_dir(None)
                .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;
            return Ok("已恢复默认模型目录".to_string());
        }
    };

    // canonicalize 后比较，避免 Windows 大小写/尾部斜杠差异
    let canon_old = std::fs::canonicalize(&old_dir).unwrap_or_else(|_| old_dir.clone());
    // 目标目录可能还不存在，canonicalize 会失败，回退到原始路径比较
    let canon_new = std::fs::canonicalize(&new_dir).unwrap_or_else(|_| new_dir.clone());
    if canon_old == canon_new {
        return Ok("路径未变化".to_string());
    }

    // 确保目标目录存在（放 spawn_blocking 避免网络驱动器阻塞 async 线程）
    let new_dir_clone = new_dir.clone();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&new_dir_clone))
        .await
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?
        .map_err(|e| AppError::Other(format!("创建目录失败: {}", e)))?;

    if migrate && old_dir.is_dir() {
        // 设置 starting 标志，阻止前端轮询触发的自动重启
        state
            .funasr_starting
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let migration_result: Result<(), AppError> = async {
            funasr_service::stop_server(state.inner()).await?;

            let _ = app_handle.emit(
                "models-migrate-status",
                serde_json::json!({ "status": "migrating", "message": "正在迁移模型文件..." }),
            );

            let old = old_dir.clone();
            let dest = new_dir.clone();
            let handle = app_handle.clone();
            tokio::task::spawn_blocking(move || migrate_model_dirs(&old, &dest, &handle))
                .await
                .map_err(|e| AppError::Other(format!("迁移任务失败: {}", e)))?
                .map_err(|e| AppError::Other(format!("迁移失败: {}", e)))?;

            Ok(())
        }
        .await;

        // 无论成功失败都解除阻止，让后续 retryModel 能正常启动
        state
            .funasr_starting
            .store(false, std::sync::atomic::Ordering::SeqCst);

        migration_result?;
    }

    // 写入配置
    let dir_str = paths::strip_win_prefix(&new_dir);
    paths::write_models_dir(Some(&dir_str))
        .map_err(|e| AppError::Other(format!("写入配置失败: {}", e)))?;

    let _ = app_handle.emit(
        "models-migrate-status",
        serde_json::json!({ "status": "completed", "message": "模型目录已更新" }),
    );

    Ok("模型目录已更新".to_string())
}

/// 迁移 models--* 目录从 src 到 dst
///
/// 策略：先全量复制/rename，全部成功后再统一删除源目录。
/// 任何一步失败则中止，已完成的复制保留在目标，源文件不删除，保证数据不丢失。
fn migrate_model_dirs(
    src: &std::path::Path,
    dst: &std::path::Path,
    handle: &tauri::AppHandle,
) -> Result<(), String> {
    let entries: Vec<_> = std::fs::read_dir(src)
        .map_err(|e| format!("读取源目录失败: {}", e))?
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("models--")
                && e.path().is_dir()
        })
        .collect();

    if entries.is_empty() {
        return Ok(());
    }

    let total = entries.len();
    // 记录需要跨盘复制（而非 rename）的目录，稍后统一删除源
    let mut copied_sources: Vec<std::path::PathBuf> = Vec::new();

    // 第一阶段：全量迁移（rename 或 copy）
    for (i, entry) in entries.iter().enumerate() {
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        if dst_path.exists() {
            log::info!("迁移跳过（已存在）: {}", name.to_string_lossy());
        } else if std::fs::rename(&src_path, &dst_path).is_ok() {
            log::info!("迁移（rename）: {}", name.to_string_lossy());
        } else {
            // 跨盘：先复制，不立即删源
            copy_dir_recursive(&src_path, &dst_path)
                .map_err(|e| format!("复制 {} 失败: {}", name.to_string_lossy(), e))?;
            copied_sources.push(src_path);
            log::info!("迁移（copy）: {}", name.to_string_lossy());
        }

        let _ = handle.emit(
            "models-migrate-status",
            serde_json::json!({
                "status": "migrating",
                "message": format!("正在迁移 {}/{}...", i + 1, total),
                "progress": ((i + 1) as f64 / total as f64 * 100.0).round(),
            }),
        );
    }

    // 第二阶段：全部复制成功后，统一清理源目录
    for source in &copied_sources {
        if let Err(e) = std::fs::remove_dir_all(source) {
            log::warn!("清理源目录失败（不影响迁移结果）: {} — {}", source.display(), e);
        }
    }

    Ok(())
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
