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

/// IPC 音频上限。前端是受信任的（同进程渲染器），但仍统一卡上限：
/// 1) 防御异常路径（漏网录音、bug、未来其他调用方）一次性吃光内存。
/// 2) 在线引擎本身就有更严的上限（DashScope 10MB、GLM 类似），统一卡口
///    让本地引擎也享受同一份保护。base64 比原始字节大 4/3，按 64MB 原始
///    对应 ~85MB base64，对 16kHz/16bit 单声道相当于约 33 分钟，足够覆盖
///    常规交互式录音；超长音频应走分段方案。
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
) -> Result<String, AppError> {
    crate::services::download_service::run_download(&app_handle, state.inner()).await
}

#[tauri::command]
pub async fn cancel_model_download(state: tauri::State<'_, AppState>) -> Result<String, AppError> {
    let cancellation = {
        let mut guard = state.engine.download_task.lock().await;
        guard.as_mut().and_then(|task| task.cancel.take())
    };

    if let Some(cancel) = cancellation {
        let _ = cancel.send(());
        Ok("已取消模型下载".to_string())
    } else if state.engine.download_task.lock().await.is_some() {
        Ok("模型下载正在取消".to_string())
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
    let engine = paths::read_engine_config();
    if paths::is_online_engine(&engine) {
        // 在线引擎无需重启 Python，仅刷新就绪状态
        let has_key = !state.read_online_asr_api_key().is_empty();
        state.set_funasr_ready(has_key);
        return Ok("在线引擎状态已刷新".to_string());
    }

    // 首次启动可能仍在解压 engine.zip 或加载模型，自动重启不应打断这一过程。
    if state.engine.is_funasr_starting() {
        log::info!("FunASR 正在启动中，跳过本次重启请求");
        return Ok("FunASR 正在启动中，跳过重启".to_string());
    }

    log::info!("正在重启 FunASR 服务器...");
    funasr_service::stop_server(state.inner()).await?;
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
    const VALID: &[&str] = &["sensevoice", "whisper", "glm-asr", "alibaba-asr"];
    if !VALID.contains(&engine.as_str()) {
        return Err(AppError::Other(format!(
            "不支持的引擎类型: {}，可选值: {}",
            engine,
            VALID.join(", ")
        )));
    }

    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    if state.engine.download_task.lock().await.is_some() {
        return Err(AppError::Other(
            "模型正在下载，请等待完成或取消下载后再切换引擎".to_string(),
        ));
    }
    // 配置文件使用原子替换；只有提交成功后才停止旧服务。这样磁盘/权限错误
    // 不会把仍然有效的旧引擎留在“配置未变但进程已停”的状态。
    paths::write_engine_config(&engine)
        .map_err(|e| AppError::Other(format!("写入引擎配置失败: {}", e)))?;
    state.engine.block_funasr_starting();
    let switch_result: Result<(), AppError> = async {
        funasr_service::stop_server(state.inner()).await?;

        // 在线引擎：切换后从密钥环重新加载对应的 API Key，然后刷新就绪状态。
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

    // keyring IO 不占生命周期锁；保存完成后再用生命周期锁把“活跃槽重读→
    // runtime cache/ready/event 更新”组成原子提交，避免引擎或区域切换后旧 key
    // 覆盖新 provider 的运行时凭据。
    let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
    let current_active_user = active_online_keyring_user();

    // 只有当目标槽和当前活跃槽一致时，才更新运行时缓存与就绪状态，
    // 否则会把一个不相关的 key 写进 state 造成状态与 UI 不一致。
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
        log::warn!(
            "抓取 DashScope 模型列表失败 (network)：{:?}",
            resp_result.err()
        );
        return Ok(fallback());
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        log::warn!(
            "DashScope /v1/models HTTP {}（响应{}字符）",
            status,
            body.chars().count()
        );
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
) -> Result<ModelsDirUpdateResult, AppError> {
    let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
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

    // canonicalize 后比较，避免 Windows 大小写/尾部斜杠差异
    let canon_old = std::fs::canonicalize(&old_dir).unwrap_or_else(|_| old_dir.clone());
    // 目标目录可能还不存在，canonicalize 会失败，回退到原始路径比较
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

    // 确保目标目录存在（放 spawn_blocking 避免网络驱动器阻塞 async 线程）
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

    // prepare 失败只留下可安全覆盖的目标副本；源和旧运行时都保持不变。
    // 配置原子提交成功之后才取消旧启动并停止旧服务。
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
            // start 完成到 cleanup 之间可能有另一次目录切换。重新进入 lifecycle
            // 并确认当前配置仍指向本次目标，避免删除已经被切回使用的旧目录。
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

/// 迁移 models--* 目录从 src 到 dst
///
/// 准备阶段只复制，不 rename/删除源目录。调用方必须先原子提交新配置，
/// 并确认新运行时可用后，才能 best-effort 清理返回的源目录。
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

    // prepare 阶段：即使目标已存在也递归补齐/覆盖源文件；源始终保留。
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
