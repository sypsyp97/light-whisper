use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::Manager;

const APP_IDENTIFIER: &str = "com.light-whisper.app";

pub fn get_data_dir() -> &'static PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".light-whisper"));
        let app_dir = base.join(APP_IDENTIFIER);
        let _ = std::fs::create_dir_all(&app_dir);
        app_dir
    })
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
    if let Some(engine) = read_engine_json().get("engine").and_then(|v| v.as_str()) {
        match engine {
            "whisper" | "sensevoice" | "glm-asr" => return engine.to_string(),
            _ => {}
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
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("序列化配置失败: {}", e),
        )
    })?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, serialized)
}

/// 返回当前配置的区域标识：`"international"` 或 `"domestic"`
pub fn read_online_asr_region() -> String {
    match read_engine_json()
        .get("glm_endpoint")
        .and_then(|v| v.as_str())
    {
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

fn update_engine_json_field(key: &str, value: &str) -> Result<(), std::io::Error> {
    let mut obj = read_engine_json();
    obj.as_object_mut().unwrap().insert(
        key.to_string(),
        serde_json::Value::String(value.to_string()),
    );
    write_engine_json(&obj)
}

/// `region`: `"international"` 或 `"domestic"`
pub fn write_online_asr_endpoint(region: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("glm_endpoint", region)
}

/// 查找已解压的 engine.exe
///
/// 仅检查数据目录（从引擎归档解压后的位置）。
pub fn get_engine_exe_path(_app: &tauri::AppHandle) -> Option<PathBuf> {
    let data_engine = get_data_dir().join("engine").join("engine.exe");
    if data_engine.exists() {
        return Some(data_engine);
    }
    None
}

/// 查找资源目录中的 engine.exe（开发时直接放置 python-dist 的情况）。
pub fn get_resource_engine_exe_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let resource_engine = resource_dir
            .join("resources")
            .join("python-dist")
            .join("engine")
            .join("engine.exe");
        if resource_engine.exists() {
            return Some(resource_engine);
        }
    }
    None
}

/// 查找打包的引擎归档（NSIS 安装后存在于资源目录）
///
/// 跳过空文件（build.rs 为 dev 模式创建的占位文件）。
pub fn get_engine_archive_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let resources_dir = resource_dir.join("resources");
        for filename in ["engine.tar.xz", "engine.zip"] {
            let archive = resources_dir.join(filename);
            if archive.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                return Some(archive);
            }
        }
    }
    None
}

/// 获取 engine 解压目标目录
pub fn get_engine_dir() -> PathBuf {
    get_data_dir().join("engine")
}

pub fn write_engine_config(engine: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("engine", engine)
}

/// 读取用户自定义模型目录（None 表示使用默认 HF 缓存）
pub fn read_models_dir() -> Option<String> {
    read_engine_json()
        .get("models_dir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 写入自定义模型目录（None 表示恢复默认）
pub fn write_models_dir(dir: Option<&str>) -> Result<(), std::io::Error> {
    let mut obj = read_engine_json();
    let map = obj.as_object_mut().unwrap();
    match dir.filter(|s| !s.is_empty()) {
        Some(d) => {
            map.insert(
                "models_dir".to_string(),
                serde_json::Value::String(d.to_string()),
            );
        }
        None => {
            map.remove("models_dir");
        }
    }
    write_engine_json(&obj)
}

/// 默认 HF 缓存根目录（不考虑自定义配置）
fn default_hf_cache_root() -> PathBuf {
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        return PathBuf::from(hf_home).join("hub");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".cache").join("huggingface").join("hub");
    }
    PathBuf::from(".cache").join("huggingface").join("hub")
}

/// 获取生效的模型缓存目录：自定义路径 > HF_HOME > 默认
pub fn get_effective_models_dir() -> PathBuf {
    if let Some(custom) = read_models_dir() {
        return PathBuf::from(custom);
    }
    default_hf_cache_root()
}
