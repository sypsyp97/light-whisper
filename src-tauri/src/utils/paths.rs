//! 路径工具模块
//!
//! 管理应用的各种文件路径，比如日志文件、模型缓存等。
//!
//! # 路径说明
//! - `C:\Users\用户名\AppData\Roaming\com.light-whisper.app\`

use std::path::PathBuf;

/// 应用的唯一标识符，用于确定数据存储目录
/// `const` 定义的是编译时常量，类似于其他语言中的 `final` 或 `readonly`
const APP_IDENTIFIER: &str = "com.light-whisper.app";

/// 获取应用的数据目录
///
/// # 什么是 PathBuf？
/// `PathBuf` 是 Rust 中表示文件路径的类型，类似于 `String` 之于字符串。
/// 它会自动处理不同操作系统的路径分隔符（Windows 用 `\`，Linux/macOS 用 `/`）。
///
/// # 返回值
/// 返回应用专属的数据存储目录路径。如果目录不存在会自动创建。
pub fn get_data_dir() -> PathBuf {
    // `dirs` crate 提供了跨平台获取系统目录的功能
    // `data_dir()` 返回 Option<PathBuf>：
    //   - Some(路径) 表示成功获取
    //   - None 表示获取失败
    // `.unwrap_or_else(|| ...)` 的意思是：如果是 None，就用后面的默认值
    let base = dirs::data_dir().unwrap_or_else(|| {
        // 如果获取系统数据目录失败，就用当前目录下的 .light-whisper 文件夹
        PathBuf::from(".light-whisper")
    });

    let app_dir = base.join(APP_IDENTIFIER);

    // 确保目录存在，如果不存在就创建
    // `let _ =` 表示我们忽略这个操作的返回值（创建失败也没关系，后续操作会报错）
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir
}

/// 获取 resources 目录下某个脚本的路径
///
/// 优先从 Tauri 打包资源目录查找，找不到则回退到相对路径（开发模式）。
fn get_resource_script_path(app: &tauri::AppHandle, filename: &str) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let script_path = resource_dir.join("resources").join(filename);
        if script_path.exists() {
            return script_path;
        }
    }

    PathBuf::from("resources").join(filename)
}

/// 获取 FunASR 服务器 Python 脚本的路径
pub fn get_funasr_server_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "funasr_server.py")
}

/// 获取 Whisper 服务器 Python 脚本的路径
pub fn get_whisper_server_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "whisper_server.py")
}

/// 获取模型下载脚本的路径
pub fn get_download_script_path(app: &tauri::AppHandle) -> PathBuf {
    get_resource_script_path(app, "download_models.py")
}

/// 清理 Windows 路径中的 `\\?\` 前缀
///
/// Windows 的 `std::fs::canonicalize()` 和某些 Tauri API 返回的路径
/// 可能带有 `\\?\` 前缀（UNC 扩展路径格式）。
/// 大多数程序（包括 Python）可能无法正确处理这个前缀，所以需要去掉它。
pub fn strip_win_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// 获取引擎配置文件路径（{app_data_dir}/engine.json）
pub fn get_engine_config_path() -> PathBuf {
    get_data_dir().join("engine.json")
}

/// 读取当前引擎配置，默认返回 "sensevoice"
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

/// 写入引擎配置
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

// 需要导入 tauri::Manager trait 才能使用 app.path() 方法
// `use` 语句用于引入其他模块的内容
use tauri::Manager;
