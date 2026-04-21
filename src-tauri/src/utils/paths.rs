use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::Manager;

const APP_IDENTIFIER: &str = "com.light-whisper.desktop";
const LEGACY_APP_IDENTIFIER: &str = "com.light-whisper.app";
const DEFAULT_ENGINE: &str = "alibaba-asr";

pub fn get_data_dir() -> &'static PathBuf {
    static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from(".light-whisper"));
        let app_dir = base.join(APP_IDENTIFIER);
        let legacy_dir = base.join(LEGACY_APP_IDENTIFIER);

        if !app_dir.exists() && legacy_dir.exists() {
            match std::fs::rename(&legacy_dir, &app_dir) {
                Ok(()) => {
                    log::info!(
                        "已将历史数据目录迁移到新的应用标识目录: {} -> {}",
                        legacy_dir.display(),
                        app_dir.display()
                    );
                }
                Err(err) => {
                    log::warn!(
                        "迁移历史数据目录失败，继续使用新目录: {} -> {} ({})",
                        legacy_dir.display(),
                        app_dir.display(),
                        err
                    );
                }
            }
        }

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

pub fn strip_win_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

pub fn get_engine_config_path() -> PathBuf {
    get_data_dir().join("engine.json")
}

fn normalize_engine(engine: &str) -> &'static str {
    match engine {
        "glm-asr" => "glm-asr",
        "alibaba-asr" | "local" | "sensevoice" | "whisper" => DEFAULT_ENGINE,
        _ => DEFAULT_ENGINE,
    }
}

pub fn read_engine_config() -> String {
    if let Some(engine) = read_engine_json().get("engine").and_then(|v| v.as_str()) {
        return normalize_engine(engine).to_string();
    }
    DEFAULT_ENGINE.to_string()
}

pub fn is_online_engine(engine: &str) -> bool {
    matches!(engine, "glm-asr" | "alibaba-asr")
}

const GLM_ENDPOINT_INTERNATIONAL: &str = "https://api.z.ai";
const GLM_ENDPOINT_DOMESTIC: &str = "https://open.bigmodel.cn";

const ALIBABA_ENDPOINT_INTERNATIONAL: &str = "https://dashscope-intl.aliyuncs.com";
const ALIBABA_ENDPOINT_DOMESTIC: &str = "https://dashscope.aliyuncs.com";

pub const ALIBABA_DEFAULT_MODEL: &str = "qwen3-asr-flash";

/// 运行时抓取失败时使用的静态兜底列表。包含 2026-04 已知在 DashScope 上架的
/// 全部 Qwen ASR / Omni 家族模型；抓取成功后以实际结果为准，不受此列表限制。
///
/// 走哪条 HTTP 路径由 `alibaba_model_uses_omni_chat` 运行时决定：
/// - `qwen3-asr-*` → `/api/v1/services/aigc/multimodal-generation/generation`
/// - `*omni*` → `/compatible-mode/v1/chat/completions`
pub const ALIBABA_FALLBACK_MODEL_IDS: &[&str] = &[
    "qwen3-asr-flash",
    "qwen3-omni-flash",
    "qwen3-omni-plus",
    "qwen3.5-omni-flash",
    "qwen3.5-omni-plus",
    "qwen-omni-turbo",
];

/// 模型 ID 是否可能胜任 ASR（作为语音转文字使用）。
///
/// 用于过滤 DashScope `/v1/models` 返回的完整模型清单：包含 asr/omni/audio 关键词
/// 的模型入围，同时排除明确不做转写的 realtime / tts / vl / coder 等家族。
pub fn is_asr_capable_model_id(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let looks_asr =
        id.contains("asr") || id.contains("omni") || id.contains("audio") || id.contains("stt");
    if !looks_asr {
        return false;
    }
    // 精确匹配：`-vl-` / `-vl` 尾部（避免误伤 "novel"/"evaluation" 等普通词）
    const BLOCK_SUBSTR: &[&str] = &[
        "realtime",
        "tts",
        "embedding",
        "embed",
        "rerank",
        "caption",
        "coder",
        "math",
        "thinking",
        "image",
        "video-gen",
    ];
    if BLOCK_SUBSTR.iter().any(|b| id.contains(b)) {
        return false;
    }
    // vision-language 模型（qwen2.5-vl-*, qwen3-vl-*）走段边界匹配
    if id.contains("-vl-") || id.ends_with("-vl") {
        return false;
    }
    true
}

/// Omni 家族模型走 OpenAI-compat chat.completions 路径；
/// 其它走 qwen3-asr-flash 专用的 multimodal-generation 路径。
pub fn alibaba_model_uses_omni_chat(model: &str) -> bool {
    model.contains("omni")
}

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

fn read_region_field(field: &str) -> String {
    match read_engine_json().get(field).and_then(|v| v.as_str()) {
        Some("domestic") => "domestic".to_string(),
        _ => "international".to_string(),
    }
}

/// 返回 GLM-ASR 区域标识：`"international"` 或 `"domestic"`
pub fn read_glm_region() -> String {
    read_region_field("glm_endpoint")
}

/// 返回 Alibaba ASR 区域标识：`"international"` 或 `"domestic"`
pub fn read_alibaba_region() -> String {
    read_region_field("alibaba_region")
}

/// 返回 Alibaba ASR 当前选择的模型 ID。
///
/// 这里不再对值做白名单校验——DashScope 上架速度快于硬编码列表的更新频率，
/// 运行时抓取回来的新模型应该能直接用。非法字符由 write_alibaba_model 入口过滤。
pub fn read_alibaba_model() -> String {
    read_engine_json()
        .get("alibaba_model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| ALIBABA_DEFAULT_MODEL.to_string())
}

/// 返回当前活跃在线引擎的区域标识。
pub fn read_online_asr_region() -> String {
    match read_engine_config().as_str() {
        "alibaba-asr" => read_alibaba_region(),
        _ => read_glm_region(),
    }
}

/// 返回当前活跃在线引擎的端点域名（不含路径）。
pub fn read_online_asr_endpoint() -> String {
    match read_engine_config().as_str() {
        "alibaba-asr" => read_alibaba_endpoint(),
        _ => read_glm_endpoint(),
    }
}

/// 返回 GLM-ASR 端点域名（不含路径）。
pub fn read_glm_endpoint() -> String {
    match read_glm_region().as_str() {
        "domestic" => GLM_ENDPOINT_DOMESTIC.to_string(),
        _ => GLM_ENDPOINT_INTERNATIONAL.to_string(),
    }
}

/// 返回 Alibaba DashScope 端点域名（不含路径）。
pub fn read_alibaba_endpoint() -> String {
    match read_alibaba_region().as_str() {
        "domestic" => ALIBABA_ENDPOINT_DOMESTIC.to_string(),
        _ => ALIBABA_ENDPOINT_INTERNATIONAL.to_string(),
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

/// 写入 GLM-ASR 区域。`region`: `"international"` 或 `"domestic"`
pub fn write_glm_region(region: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("glm_endpoint", region)
}

/// 写入 Alibaba ASR 区域。`region`: `"international"` 或 `"domestic"`
pub fn write_alibaba_region(region: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("alibaba_region", region)
}

/// 写入 Alibaba ASR 当前选择的模型 ID。
pub fn write_alibaba_model(model: &str) -> Result<(), std::io::Error> {
    update_engine_json_field("alibaba_model", model)
}

/// 根据当前引擎写入对应区域配置。
pub fn write_online_asr_endpoint(region: &str) -> Result<(), std::io::Error> {
    match read_engine_config().as_str() {
        "alibaba-asr" => write_alibaba_region(region),
        _ => write_glm_region(region),
    }
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
    update_engine_json_field("engine", normalize_engine(engine))
}

/// 读取用户自定义模型目录（None 表示使用默认 HF 缓存）
pub fn read_models_dir() -> Option<String> {
    read_engine_json()
        .get("models_dir")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
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
