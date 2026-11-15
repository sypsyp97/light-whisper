use crate::commands::funasr::lock_idle_asr;
use crate::services::funasr_service::{send_command_to_server_json, ServerCommand};
use crate::state::{user_profile::R2T2Config, AppState};
use crate::utils::{paths, AppError};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tauri::Manager;

const MAX_AUDIO_BYTES: u64 = 64 * 1024 * 1024;

struct OwnedQueryAudio(PathBuf);

impl OwnedQueryAudio {
    fn create(bytes: &[u8]) -> Result<Self, AppError> {
        let path = std::env::temp_dir().join(format!(
            "light_whisper_agent_{}_{}.wav",
            std::process::id(),
            rand::random::<u128>()
        ));
        let mut file = super::secure_files::create_private_file(&path)?;
        let owned = Self(path);
        let result = file.write_all(bytes);
        // Close the Windows file handle before cleanup on either result.
        drop(file);
        result?;
        Ok(owned)
    }
}

impl Drop for OwnedQueryAudio {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.0) {
            log::warn!("清理代理查询临时音频失败: {}", error);
        }
    }
}

fn read_audio(path: &Path) -> Result<Vec<u8>, AppError> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(AppError::Audio("音频路径必须是普通文件".into()));
    }
    if metadata.len() > MAX_AUDIO_BYTES {
        return Err(AppError::Audio("音频文件超过 64 MiB 上限".into()));
    }
    let mut bytes = Vec::new();
    // The extra byte detects growth after metadata without an unbounded read.
    file.take(MAX_AUDIO_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_AUDIO_BYTES {
        return Err(AppError::Audio("音频文件超过 64 MiB 上限".into()));
    }
    Ok(bytes)
}

fn transcribe_options(
    state: &AppState,
    request: &Value,
) -> Result<(R2T2Config, Vec<String>), AppError> {
    let (mut options, mut hot_words) =
        state.with_profile(|profile| (profile.r2t2.clone(), profile.get_hot_word_texts(100)));
    if let Some(overrides) = request.get("options") {
        let overrides = overrides
            .as_object()
            .ok_or_else(|| AppError::Other("options 必须是对象".into()))?;
        for (key, value) in overrides {
            let text = value
                .as_str()
                .ok_or_else(|| AppError::Other(format!("options.{} 必须是字符串", key)))?;
            match key.as_str() {
                "language" => options.language = Some(text.to_string()),
                "context" => options.context = text.to_string(),
                _ => return Err(AppError::Other(format!("不支持的转写选项: {}", key))),
            }
        }
    }
    if let Some(overrides) = request.get("hot_words") {
        hot_words = overrides
            .as_array()
            .ok_or_else(|| AppError::Other("hot_words 必须是字符串数组".into()))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| AppError::Other("hot_words 必须是字符串数组".into()))
            })
            .collect::<Result<_, _>>()?;
    }
    Ok((options, hot_words))
}

fn desktop_response(configured_engine: &str) -> Value {
    json!({
        "success": true,
        "surface": "desktop",
        "configured_engine": configured_engine,
        "runtime_observed": false,
        "runtime_engine": null,
        "busy": false,
    })
}

pub(crate) async fn query<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: Value,
) -> Result<Value, AppError> {
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .filter(|action| matches!(*action, "engine_get" | "status" | "stats" | "transcribe"))
        .ok_or_else(|| AppError::Other("不支持的代理查询操作".into()))?;
    let configured_engine = paths::read_engine_config();
    let mut response = desktop_response(&configured_engine);
    if action == "engine_get" {
        return Ok(response);
    }
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| AppError::Other("桌面应用状态不可用".into()))?;
    if action == "status" && state.engine.is_funasr_starting() {
        response["busy"] = json!(true);
        response["starting"] = json!(true);
        response["running"] = Value::Null;
        response["ready"] = json!(false);
        response["model_loaded"] = Value::Null;
        return Ok(response);
    }
    // Admission precedes process locks and file reads; keep this permit through IPC cleanup.
    let _owner = match lock_idle_asr(&state) {
        Ok(owner) => owner,
        Err(error) => {
            response["busy"] = json!(true);
            if action == "status" {
                response["running"] = Value::Null;
                response["ready"] = Value::Null;
                response["model_loaded"] = Value::Null;
            } else {
                response["success"] = json!(false);
                response["code"] = json!("busy");
                response["error"] = json!(error.to_string());
            }
            return Ok(response);
        }
    };

    if state.engine.funasr_process.lock().await.is_none() {
        if action != "status" {
            return Err(AppError::Asr("语音引擎进程未运行".into()));
        }
        response["running"] = json!(false);
        response["ready"] = json!(false);
        response["model_loaded"] = json!(false);
        return Ok(response);
    }

    match action {
        "status" => {
            let native = send_command_to_server_json(&state, &ServerCommand::Status, app).await?;
            let model_loaded = native["model_loaded"].as_bool().unwrap_or_else(|| {
                ["asr", "vad", "punc"]
                    .iter()
                    .all(|key| native["models"][key].as_bool() == Some(true))
            });
            response["running"] = json!(true);
            response["ready"] =
                json!(native["initialized"].as_bool() == Some(true) || model_loaded);
            response["model_loaded"] = json!(model_loaded);
            response["runtime_observed"] = json!(true);
            response["runtime_engine"] = native["engine"].clone();
            for key in ["device", "gpu_name", "gpu_memory_total", "message"] {
                response[key] = native[key].clone();
            }
        }
        "stats" => {
            let native = send_command_to_server_json(&state, &ServerCommand::Stats, app).await?;
            let stats = &native["stats"];
            if !stats.is_object()
                || stats["transcription_count"].as_u64().is_none()
                || !stats["total_audio_duration"]
                    .as_f64()
                    .is_some_and(|duration| duration.is_finite() && duration >= 0.0)
            {
                return Err(AppError::Asr("语音引擎返回的统计数据缺失或无效".into()));
            }
            response["stats"] = stats.clone();
        }
        "transcribe" => {
            if paths::is_online_engine(&configured_engine) {
                return Err(AppError::Asr("代理文件转写仅支持本地语音引擎".into()));
            }
            let path = request["audio_path"]
                .as_str()
                .ok_or_else(|| AppError::Audio("audio_path 必须是文件路径字符串".into()))?;
            let (options, hot_words) = transcribe_options(&state, &request)?;
            let bytes = read_audio(Path::new(path))?;
            // Preparation is bounded and synchronous, so future cancellation cannot
            // interrupt a partial write before its RAII guard has been installed.
            let audio = OwnedQueryAudio::create(&bytes)?;
            let native = send_command_to_server_json(
                &state,
                &ServerCommand::Transcribe {
                    options: Some(options),
                    audio_path: Some(audio.0.to_string_lossy().into_owned()),
                    audio_base64: None,
                    audio_format: None,
                    sample_rate: None,
                    hot_words: Some(hot_words),
                },
                app,
            )
            .await?;
            if !native["text"].is_string() {
                return Err(AppError::Asr("语音引擎返回的转写文本缺失或无效".into()));
            }
            for key in ["text", "duration", "language"] {
                response[key] = native[key].clone();
            }
        }
        _ => unreachable!("query action was validated before admission"),
    }
    Ok(response)
}

pub(crate) async fn dispatch(
    app: &tauri::AppHandle,
    request: Value,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> Value {
    if *shutdown.borrow() {
        return failure("owner_shutdown", "桌面应用正在退出");
    }
    tokio::select! {
        biased;
        _ = shutdown.changed() => failure("owner_shutdown", "桌面应用正在退出"),
        result = dispatch_request(app, request) => match result {
            Ok(response) => response,
            Err(error) => failure("operation_failed", &error.to_string()),
        },
    }
}

fn failure(code: &str, error: &str) -> Value {
    json!({"success": false, "surface": "desktop", "code": code, "error": error})
}

async fn dispatch_request(app: &tauri::AppHandle, request: Value) -> Result<Value, AppError> {
    match request["action"].as_str() {
        Some("engine_get" | "status" | "stats" | "transcribe") => query(app, request).await,
        Some("models") => {
            let models = crate::services::funasr_service::check_model_files().await?;
            let mut response = desktop_response(&paths::read_engine_config());
            response["models"] = serde_json::to_value(models)?;
            Ok(response)
        }
        Some("devices") => {
            let state = app.state::<AppState>();
            let devices = crate::services::audio_service::list_input_devices_sync(
                state.selected_input_device_name(),
            )?;
            let mut response = desktop_response(&paths::read_engine_config());
            response["devices"] = serde_json::to_value(devices.devices)?;
            Ok(response)
        }
        Some(action @ ("engine_start" | "engine_restart" | "engine_set")) => {
            let requested_engine = if action == "engine_set" {
                request["engine"].as_str().unwrap_or_default().to_string()
            } else {
                paths::read_engine_config()
            };
            if !matches!(
                requested_engine.as_str(),
                "qwen3-asr-0.6b" | "confucius4-r2t2"
            ) {
                return Ok(failure("invalid_request", "代理操作仅支持本地语音引擎"));
            }
            let state = app.state::<AppState>();
            let _lifecycle = state.engine.funasr_lifecycle_op.lock().await;
            if action != "engine_set"
                && !matches!(
                    paths::read_engine_config().as_str(),
                    "qwen3-asr-0.6b" | "confucius4-r2t2"
                )
            {
                return Ok(failure("invalid_request", "代理操作仅支持本地语音引擎"));
            }
            let _owner = match lock_idle_asr(&state) {
                Ok(owner) if !state.engine.is_funasr_starting() => owner,
                _ => return Ok(failure("busy", "录音、处理或语音引擎启动尚未结束")),
            };
            if action == "engine_set" {
                if state.engine.download_task.lock().await.is_some() {
                    return Ok(failure("busy", "模型下载尚未结束"));
                }
                paths::write_engine_config(&requested_engine)?;
                state.engine.block_funasr_starting();
                let result = crate::services::funasr_service::stop_server(state.inner()).await;
                state.engine.unblock_funasr_starting();
                result?;
                use tauri::Emitter;
                app.emit("engine-config-changed", &requested_engine)?;
            } else {
                if action == "engine_restart" {
                    crate::services::funasr_service::stop_server(state.inner()).await?;
                }
                drop(_lifecycle);
                crate::services::funasr_service::start_server(app, state.inner()).await?;
            }
            Ok(desktop_response(&paths::read_engine_config()))
        }
        _ => Ok(failure("invalid_request", "不支持的代理操作")),
    }
}
