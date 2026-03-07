use std::path::PathBuf;
use tauri::Manager;

const APP_IDENTIFIER: &str = "com.light-whisper.app";

pub fn get_data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".light-whisper"));

    let app_dir = base.join(APP_IDENTIFIER);
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir
}

fn get_resource_script_path(app: &tauri::AppHandle, filename: &str) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let script_path = resource_dir.join("resources").join(filename);
        if script_path.exists() {
            return script_path;
        }
    }

    PathBuf::from("resources").join(filename)
}

pub fn get_funasr_server_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "funasr_server.py")
}

pub fn get_whisper_server_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "whisper_server.py")
}

pub fn get_download_script_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "download_models.py")
}

pub fn strip_win_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

pub fn get_engine_config_path() -> PathBuf {
    get_data_dir().join("engine.json")
}

pub fn read_engine_config() -> String {
    let config_path = get_engine_config_path();
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(engine) = value.get("engine").and_then(|v| v.as_str()) {
                let engine = engine.to_string();
                if engine == "whisper" || engine == "sensevoice" || engine == "glm-asr" {
                    return engine;
                }
            }
        }
    }
    "sensevoice".to_string()
}

pub fn is_online_engine(engine: &str) -> bool {
    engine == "glm-asr"
}

const GLM_ENDPOINT_INTERNATIONAL: &str = "https://api.z.ai";
const GLM_ENDPOINT_DOMESTIC: &str = "https://open.bigmodel.cn";

fn read_engine_json() -> serde_json::Value {
    std::fs::read_to_string(get_engine_config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

fn write_engine_json(obj: &serde_json::Value) -> Result<(), std::io::Error> {
    let config_path = get_engine_config_path();
    let serialized = serde_json::to_string_pretty(obj).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("序列化配置失败: {}", e))
    })?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, serialized)
}

/// 返回当前配置的区域标识：`"international"` 或 `"domestic"`
pub fn read_online_asr_region() -> String {
    match read_engine_json().get("glm_endpoint").and_then(|v| v.as_str()) {
        Some("domestic") => "domestic".to_string(),
        _ => "international".to_string(),
    }
}

/// 返回 GLM-ASR 端点域名（不含路径）。
pub fn read_online_asr_endpoint() -> String {
    match read_online_asr_region().as_str() {
        "domestic" => GLM_ENDPOINT_DOMESTIC.to_string(),
        _ => GLM_ENDPOINT_INTERNATIONAL.to_string(),
    }
}

/// `region`: `"international"` 或 `"domestic"`
pub fn write_online_asr_endpoint(region: &str) -> Result<(), std::io::Error> {
    let mut obj = read_engine_json();
    obj.as_object_mut()
        .unwrap()
        .insert("glm_endpoint".to_string(), serde_json::Value::String(region.to_string()));
    write_engine_json(&obj)
}

pub fn write_engine_config(engine: &str) -> Result<(), std::io::Error> {
    let mut obj = read_engine_json();
    obj.as_object_mut()
        .unwrap()
        .insert("engine".to_string(), serde_json::Value::String(engine.to_string()));
    write_engine_json(&obj)
}
