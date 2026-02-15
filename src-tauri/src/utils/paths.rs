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
                if engine == "whisper" || engine == "sensevoice" {
                    return engine;
                }
            }
        }
    }
    "sensevoice".to_string()
}

pub fn write_engine_config(engine: &str) -> Result<(), std::io::Error> {
    let config_path = get_engine_config_path();
    let content = serde_json::json!({ "engine": engine });
    let serialized = serde_json::to_string_pretty(&content).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("序列化引擎配置失败: {}", e),
        )
    })?;

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&config_path, serialized)
}
