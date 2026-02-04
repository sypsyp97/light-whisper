//! 跨平台路径工具模块
//!
//! 这个模块负责管理应用的各种文件路径，比如日志文件、模型缓存等。
//! 所有路径都基于操作系统的标准目录，确保跨平台兼容。
//!
//! # 路径说明
//! - Windows: `C:\Users\用户名\AppData\Roaming\com.ququ.app\`
//! - macOS: `~/Library/Application Support/com.ququ.app/`
//! - Linux: `~/.local/share/com.ququ.app/`

use std::path::PathBuf;

/// 应用的唯一标识符，用于确定数据存储目录
/// `const` 定义的是编译时常量，类似于其他语言中的 `final` 或 `readonly`
const APP_IDENTIFIER: &str = "com.ququ.app";

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
        // 如果获取系统数据目录失败，就用当前目录下的 .ququ 文件夹
        PathBuf::from(".ququ")
    });

    let app_dir = base.join(APP_IDENTIFIER);

    // 确保目录存在，如果不存在就创建
    // `let _ =` 表示我们忽略这个操作的返回值（创建失败也没关系，后续操作会报错）
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir
}

/// 获取 FunASR 服务器 Python 脚本的路径
///
/// # 参数
/// - `app`: Tauri 的 AppHandle，用于获取应用资源目录
///
/// # 什么是 AppHandle？
/// `AppHandle` 是 Tauri 提供的应用句柄，通过它可以访问应用的各种资源和功能。
/// 比如获取打包时附带的资源文件路径。
///
/// # 资源路径说明
/// 开发模式下，资源文件在项目的 `src-tauri/resources/` 目录中。
/// 打包后，资源文件会被嵌入到应用包中，通过 `resource_dir()` 获取。
pub fn get_funasr_server_path(app: &tauri::AppHandle) -> PathBuf {
    // 尝试从 Tauri 资源目录获取路径
    // `path()` 返回路径解析器，`resource_dir()` 获取资源目录
    if let Ok(resource_dir) = app.path().resource_dir() {
        let server_path = resource_dir.join("resources").join("funasr_server.py");
        if server_path.exists() {
            return server_path;
        }
    }

    // 备用方案：使用相对路径（开发模式下使用）
    PathBuf::from("resources").join("funasr_server.py")
}

/// 获取模型下载脚本的路径
///
/// 这个脚本负责从 HuggingFace 下载 FunASR 所需的语音识别模型。
pub fn get_download_script_path(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let script_path = resource_dir.join("resources").join("download_models.py");
        if script_path.exists() {
            return script_path;
        }
    }

    PathBuf::from("resources").join("download_models.py")
}

/// 清理 Windows 路径中的 `\\?\` 前缀
///
/// Windows 的 `std::fs::canonicalize()` 和某些 Tauri API 返回的路径
/// 可能带有 `\\?\` 前缀（UNC 扩展路径格式）。
/// 大多数程序（包括 Python）可能无法正确处理这个前缀，所以需要去掉它。
pub fn strip_win_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    if cfg!(target_os = "windows") {
        s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
    } else {
        s
    }
}

// 需要导入 tauri::Manager trait 才能使用 app.path() 方法
// `use` 语句用于引入其他模块的内容
use tauri::Manager;
