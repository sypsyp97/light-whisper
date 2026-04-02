//! FunASR 语音识别服务模块
//!
//! 这是整个应用最核心、最复杂的模块。
//! 它负责管理 FunASR Python 后端服务器的完整生命周期：
//!
//! 1. **查找 Python 解释器**：按优先级搜索可用的 Python
//! 2. **启动服务器**：以子进程方式启动 Python 脚本
//! 3. **通信协议**：通过 stdin/stdout 以 JSON 格式与 Python 进程通信
//! 4. **语音转写**：发送音频数据，接收转写结果
//! 5. **状态管理**：检查服务器状态、模型状态
//! 6. **停止服务器**：优雅地关闭 Python 进程
//!
//! # 通信协议说明
//! Rust 进程（父进程）和 Python 进程（子进程）通过标准输入/输出通信：
//! - 父进程写入 JSON 命令到子进程的 stdin
//! - 子进程处理后，将 JSON 结果写入 stdout
//! - 每条消息占一行（以换行符分隔）
//!
//! ```text
//! [Rust/Tauri] --stdin--> [Python/FunASR]
//! [Rust/Tauri] <--stdout-- [Python/FunASR]
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::state::{AppState, FunasrProcess};
use crate::utils::paths;
use crate::utils::AppError;

/// 引擎运行模式
pub enum EngineRuntime {
    /// 生产模式：直接运行打包的 engine.exe
    Bundled { exe_path: String },
    /// 开发模式：使用系统 Python 解释器 + .py 脚本
    Development { python_path: String },
}

// ============================================================
// 数据结构定义
// ============================================================

/// 发送给 Python 服务器的命令
///
/// Python 端期望的 JSON 格式是扁平的：
/// - `{"action": "status"}`
/// - `{"action": "transcribe", "audio_path": "/path/to/file.wav"}`
/// - `{"action": "exit"}`
///
/// 使用 `#[serde(tag = "action")]` 生成带 `action` 字段的扁平 JSON，
/// `rename_all = "snake_case"` 将变体名转为小写下划线格式。
#[derive(Debug, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ServerCommand {
    /// 转写音频文件
    Transcribe {
        /// 音频文件的路径
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_path: Option<String>,
        /// 内存音频负载（Base64）
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_base64: Option<String>,
        /// 内存音频编码格式
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_format: Option<String>,
        /// 内存音频采样率
        #[serde(skip_serializing_if = "Option::is_none")]
        sample_rate: Option<u32>,
        /// 热词列表（可选）
        #[serde(skip_serializing_if = "Option::is_none")]
        hot_words: Option<Vec<String>>,
    },
    /// 查询服务器状态
    Status,
    /// 退出服务器
    Exit,
}

/// 语音转写的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// 转写得到的文本
    pub text: String,
    /// 音频时长（秒）
    pub duration: Option<f64>,
    /// 是否成功
    pub success: bool,
    /// 错误信息（如果失败）
    pub error: Option<String>,
    /// 检测到的语言
    pub language: Option<String>,
}

/// FunASR 服务器的状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunASRStatus {
    /// 服务器是否正在运行
    pub running: bool,
    /// 服务器是否已就绪（模型已加载）
    pub ready: bool,
    /// 模型是否已加载
    pub model_loaded: bool,
    /// 推理设备（cpu/cuda）
    pub device: Option<String>,
    /// GPU 名称（如果可用）
    pub gpu_name: Option<String>,
    /// GPU 总显存（GB）
    pub gpu_memory_total: Option<f64>,
    /// 状态描述信息
    pub message: String,
    /// 当前引擎
    pub engine: Option<String>,
    /// 模型文件是否齐备
    pub models_present: Option<bool>,
    /// 缺失模型列表
    pub missing_models: Option<Vec<String>>,
}

/// 模型文件检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCheckResult {
    /// 所有必需的模型是否都已就位
    pub all_present: bool,
    /// ASR（语音识别）模型是否存在
    pub asr_model: bool,
    /// VAD（语音活动检测）模型是否存在
    pub vad_model: bool,
    /// 标点符号模型是否存在
    pub punc_model: bool,
    /// 当前引擎
    pub engine: String,
    /// 模型缓存目录的路径
    pub cache_path: String,
    /// 缺失的模型列表
    pub missing_models: Vec<String>,
}

const ASR_REPO_ID: &str = "FunAudioLLM/SenseVoiceSmall";
const VAD_REPO_ID: &str = "funasr/fsmn-vad";
const WHISPER_REPO_ID: &str = "deepdml/faster-whisper-large-v3-turbo-ct2";

/// Python 服务器的 JSON 响应
///
/// 这个结构体对应 Python 服务器返回的 JSON 格式。
/// `Option<T>` 表示字段可能存在也可能不存在。
#[derive(Debug, Deserialize)]
struct ServerResponse {
    /// 操作是否成功
    success: Option<bool>,
    /// 状态标识
    status: Option<String>,
    /// 转写得到的文本
    text: Option<String>,
    /// 音频时长
    duration: Option<f64>,
    /// 错误信息
    error: Option<String>,
    /// 检测到的语言
    language: Option<String>,
    /// 附加消息
    message: Option<String>,
    /// 模型是否已加载
    model_loaded: Option<bool>,
    /// 模型是否已初始化（Python status 返回）
    initialized: Option<bool>,
    /// 模型加载状态
    models: Option<ServerModelStatus>,
    /// 设备信息
    device: Option<String>,
    /// GPU 名称
    gpu_name: Option<String>,
    /// GPU 总显存（GB）
    gpu_memory_total: Option<f64>,
    /// 当前引擎
    engine: Option<String>,
    /// 服务端实际采用的输入模式（memory/path）
    input_mode: Option<String>,
}

/// Python status 返回的模型状态
#[derive(Debug, Deserialize, Clone)]
struct ServerModelStatus {
    asr: Option<bool>,
    vad: Option<bool>,
    punc: Option<bool>,
}

impl ServerResponse {
    fn is_model_loaded(&self) -> bool {
        self.model_loaded.unwrap_or_else(|| {
            self.models
                .as_ref()
                .map(|m| {
                    m.asr.unwrap_or(false) && m.vad.unwrap_or(false) && m.punc.unwrap_or(false)
                })
                .unwrap_or(false)
        })
    }
}

/// 启动标志守卫，确保异常退出时重置 funasr_starting
struct StartingFlagGuard(Arc<std::sync::atomic::AtomicBool>);

impl Drop for StartingFlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

const SERVER_INIT_TIMEOUT_SECS: u64 = 120;
const SERVER_RESPONSE_TIMEOUT_SECS: u64 = 60;
const SERVER_EXIT_WRITE_TIMEOUT_MS: u64 = 300;
const SERVER_EXIT_WAIT_TIMEOUT_SECS: u64 = 2;
const INLINE_AUDIO_FORMAT_PCM_S16LE: &str = "pcm_s16le";
const ENGINE_ARCHIVE_FINGERPRINT: &str = env!("LIGHT_WHISPER_ENGINE_ARCHIVE_FINGERPRINT");

fn to_normalized_path(path: &std::path::Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    paths::strip_win_prefix(&canonical)
}

fn status_with_defaults(
    running: bool,
    ready: bool,
    model_loaded: bool,
    message: String,
) -> FunASRStatus {
    FunASRStatus {
        running,
        ready,
        model_loaded,
        device: None,
        gpu_name: None,
        gpu_memory_total: None,
        message,
        engine: None,
        models_present: None,
        missing_models: None,
    }
}

fn expected_engine_install_fingerprint(app_handle: &tauri::AppHandle) -> String {
    if paths::get_engine_archive_path(app_handle).is_some() {
        format!(
            "{}+{}",
            env!("CARGO_PKG_VERSION"),
            ENGINE_ARCHIVE_FINGERPRINT
        )
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

fn report_model_repo_state(
    repo_id: &str,
    description: &str,
    missing_models: &mut Vec<String>,
) -> bool {
    let present = is_hf_repo_ready(repo_id);
    if present {
        log::info!("模型文件已就位: {} ({})", description, repo_id);
    } else {
        log::warn!("模型文件缺失: {} ({})", description, repo_id);
        missing_models.push(description.to_string());
    }
    present
}

async fn read_json_response<T, R>(
    reader: &mut R,
    timeout: Duration,
    context: &str,
) -> Result<T, AppError>
where
    T: for<'de> Deserialize<'de>,
    R: AsyncBufRead + Unpin,
{
    let start_at = Instant::now();
    let mut line_bytes = Vec::new();

    loop {
        let remaining = timeout
            .checked_sub(start_at.elapsed())
            .ok_or_else(|| AppError::Asr(format!("{}超时", context)))?;

        line_bytes.clear();
        let read_result =
            tokio::time::timeout(remaining, reader.read_until(b'\n', &mut line_bytes)).await;

        match read_result {
            Ok(Ok(0)) => {
                return Err(AppError::Asr(format!("{}失败：stdout 已关闭", context)));
            }
            Ok(Ok(_)) => {
                let line = match std::str::from_utf8(&line_bytes) {
                    Ok(line) => std::borrow::Cow::Borrowed(line),
                    Err(err) => {
                        log::warn!(
                            "{}阶段收到非 UTF-8 输出，已按损坏文本容错处理: {}",
                            context,
                            err
                        );
                        String::from_utf8_lossy(&line_bytes)
                    }
                };

                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                if let Ok(value) = serde_json::from_str::<T>(trimmed) {
                    return Ok(value);
                }

                // 某些 Windows 机器上，第三方库会把噪音输出和 JSON 响应挤在同一行。
                // 尝试从首个 '{' 到末尾 '}' 提取有效 JSON，避免一次脏输出导致整次初始化失败。
                if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
                    if start < end {
                        if let Ok(value) = serde_json::from_str::<T>(&trimmed[start..=end]) {
                            log::warn!("{}阶段从混合输出中恢复了 JSON 响应", context);
                            return Ok(value);
                        }
                    }
                }

                log::warn!("{}阶段收到非JSON输出: {}", context, trimmed);
                continue;
            }
            Ok(Err(e)) => {
                return Err(AppError::Asr(format!("{}失败：{}", context, e)));
            }
            Err(_) => {
                return Err(AppError::Asr(format!("{}超时", context)));
            }
        }
    }
}

// ============================================================
// 核心功能实现
// ============================================================

/// 查找引擎运行时
///
/// 按优先级：
/// 1. 已解压到数据目录的 engine.exe（版本匹配时直接复用）
/// 2. 未解压的引擎归档（engine.tar.xz / 兼容 engine.zip）→ 解压后使用
/// 3. 资源目录中的 engine.exe（开发时直接放置 python-dist）
/// 4. 系统 Python（开发模式）
pub async fn find_engine(app_handle: &tauri::AppHandle) -> Result<EngineRuntime, AppError> {
    let expected_fingerprint = expected_engine_install_fingerprint(app_handle);

    // 策略1：已解压的 engine.exe（版本匹配时使用）
    if let Some(engine_path) = paths::get_engine_exe_path(app_handle) {
        let version_file = paths::get_engine_dir().join(".version");
        let installed_version = std::fs::read_to_string(&version_file).unwrap_or_default();

        if installed_version.trim() == expected_fingerprint {
            let path_str = paths::strip_win_prefix(&engine_path);
            log::info!("找到引擎: {} ({})", path_str, expected_fingerprint);
            return Ok(EngineRuntime::Bundled { exe_path: path_str });
        }

        log::info!(
            "引擎指纹不匹配 (已安装: {:?}, 当前: {}), 需要重新解压",
            installed_version.trim(),
            expected_fingerprint
        );
    }

    // 策略2：存在引擎归档，需要解压（首次启动或版本升级）
    if let Some(archive_path) = paths::get_engine_archive_path(app_handle) {
        log::info!("找到引擎压缩包，准备解压: {}", archive_path.display());
        let engine_exe = extract_engine_archive(&archive_path, app_handle).await?;
        let path_str = paths::strip_win_prefix(&engine_exe);
        log::info!("引擎解压完成: {}", path_str);
        return Ok(EngineRuntime::Bundled { exe_path: path_str });
    }

    // 策略3：资源目录中的 engine.exe（开发时直接 PyInstaller 输出）
    if let Some(engine_path) = paths::get_resource_engine_exe_path(app_handle) {
        let path_str = paths::strip_win_prefix(&engine_path);
        log::info!("找到资源目录引擎: {}", path_str);
        return Ok(EngineRuntime::Bundled { exe_path: path_str });
    }

    // 策略4：开发模式
    let python_path = find_python().await?;
    Ok(EngineRuntime::Development { python_path })
}

/// 解压引擎归档到数据目录
async fn extract_engine_archive(
    archive_path: &std::path::Path,
    app_handle: &tauri::AppHandle,
) -> Result<std::path::PathBuf, AppError> {
    let _ = app_handle.emit(
        "funasr-status",
        serde_json::json!({
            "status": "loading",
            "message": "首次启动，正在解压引擎文件..."
        }),
    );

    let engine_dir = paths::get_engine_dir();
    let archive = archive_path.to_path_buf();
    let handle = app_handle.clone();

    // 解压是 CPU 密集型 + IO 密集型，放到阻塞线程
    tokio::task::spawn_blocking(move || {
        // 清理可能残留的不完整解压
        if engine_dir.exists() {
            let _ = std::fs::remove_dir_all(&engine_dir);
        }
        std::fs::create_dir_all(&engine_dir)
            .map_err(|e| AppError::Asr(format!("创建引擎目录失败: {}", e)))?;

        let archive_name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let total = if archive_name.ends_with(".tar.xz") {
            extract_tar_xz_archive(&archive, &engine_dir, &handle)?
        } else {
            extract_zip_archive(&archive, &engine_dir, &handle)?
        };

        if total == 0 {
            return Err(AppError::Asr("引擎归档为空".to_string()));
        }

        // 写入版本标记，用于后续升级检测
        let fingerprint = expected_engine_install_fingerprint(&handle);
        let _ = std::fs::write(engine_dir.join(".version"), fingerprint);

        log::info!("引擎解压完成: {} 个条目", total);
        Ok(engine_dir.join("engine.exe"))
    })
    .await
    .map_err(|e| AppError::Asr(format!("解压任务异常: {}", e)))?
}

fn extract_zip_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
) -> Result<usize, AppError> {
    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?;

    let total = zip.len();
    log::info!("开始解压 ZIP 引擎归档: {} 个文件", total);

    for i in 0..total {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;

        let entry_path = engine_dir.join(
            entry
                .enclosed_name()
                .ok_or_else(|| AppError::Asr("压缩包含不安全路径".to_string()))?,
        );

        if entry.is_dir() {
            std::fs::create_dir_all(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建目录失败: {}", e)))?;
        } else {
            if let Some(parent) = entry_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| AppError::Asr(format!("创建父目录失败: {}", e)))?;
            }
            let mut outfile = std::fs::File::create(&entry_path)
                .map_err(|e| AppError::Asr(format!("创建文件失败: {}", e)))?;
            std::io::copy(&mut entry, &mut outfile)
                .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        }

        report_extract_progress(handle, i + 1, Some(total), false);
    }

    Ok(total)
}

fn extract_tar_xz_archive(
    archive: &std::path::Path,
    engine_dir: &std::path::Path,
    handle: &tauri::AppHandle,
) -> Result<usize, AppError> {
    log::info!("开始解压 TAR.XZ 引擎归档");

    let file = std::fs::File::open(archive)
        .map_err(|e| AppError::Asr(format!("打开引擎压缩包失败: {}", e)))?;
    let decoder = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut extracted = 0usize;

    for entry_result in tar
        .entries()
        .map_err(|e| AppError::Asr(format!("读取引擎压缩包失败: {}", e)))?
    {
        let mut entry =
            entry_result.map_err(|e| AppError::Asr(format!("读取压缩条目失败: {}", e)))?;
        entry
            .unpack_in(engine_dir)
            .map_err(|e| AppError::Asr(format!("写入文件失败: {}", e)))?;
        extracted += 1;

        report_extract_progress(handle, extracted, None, false);
    }

    if extracted > 0 && !extracted.is_multiple_of(200) {
        report_extract_progress(handle, extracted, None, true);
    }

    Ok(extracted)
}

fn report_extract_progress(
    handle: &tauri::AppHandle,
    current: usize,
    total: Option<usize>,
    force: bool,
) {
    let should_emit = force
        || current.is_multiple_of(200)
        || total.is_some_and(|t| current == t);

    if !should_emit {
        return;
    }
    if let Some(total) = total {
        if total == 0 {
            return;
        }
        let pct = current * 100 / total;
        let _ = handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": format!("正在解压引擎文件... {}%", pct)
            }),
        );
    } else {
        let _ = handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": format!("正在解压引擎文件... 已处理 {} 项", current)
            }),
        );
    }
}

/// 查找可用的 Python 解释器（开发模式回退）
async fn find_python() -> Result<String, AppError> {
    // ---- 策略1：检查项目 .venv 虚拟环境 ----
    let mut venv_candidates = vec![PathBuf::from(".venv"), PathBuf::from("..").join(".venv")];
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            venv_candidates.push(exe_dir.join("..").join("..").join("..").join(".venv"));
            venv_candidates.push(
                exe_dir
                    .join("..")
                    .join("..")
                    .join("..")
                    .join("..")
                    .join(".venv"),
            );
        }
    }

    for venv_dir in &venv_candidates {
        let venv_python = venv_dir.join("Scripts").join("python.exe");

        if tokio::fs::try_exists(&venv_python).await.unwrap_or(false) {
            let path_str = to_normalized_path(&venv_python);
            log::info!("找到虚拟环境 Python: {}", path_str);
            return Ok(path_str);
        }
    }

    // ---- 策略2：在系统 PATH 中搜索 ----
    // 尝试多个可能的 Python 命令名
    let python_names = vec!["python.exe", "python3.exe", "python"];

    for name in &python_names {
        let check_cmd = Command::new("where").arg(name).output().await;

        if let Ok(output) = check_cmd {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();

                if !path.is_empty() {
                    let version_check = Command::new(&path).arg("--version").output().await;

                    if let Ok(ver_output) = version_check {
                        if ver_output.status.success() {
                            let version = String::from_utf8_lossy(&ver_output.stdout);
                            log::info!("找到系统 Python: {} ({})", path, version.trim());
                            return Ok(path);
                        }
                    }
                }
            }
        }
    }

    // 所有策略都失败了
    Err(AppError::Asr(
        "未找到可用的 Python 解释器。请安装 Python 3.8+ 或在项目目录创建 .venv 虚拟环境（推荐使用 uv）。"
            .to_string(),
    ))
}

/// 启动 FunASR Python 服务器
pub async fn start_server(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), AppError> {
    // 在线引擎不需要 Python 子进程
    if paths::is_online_engine(&paths::read_engine_config()) {
        return Ok(());
    }

    // 先检查是否已经有运行中的服务器或正在启动中
    {
        let process_guard = state.funasr_process.lock().await;
        if process_guard.is_some() {
            log::warn!("FunASR 服务器已在运行中");
            return Ok(());
        }
    }

    // 原子标志防止并发启动（模型加载可能 25+ 秒，比 Mutex 更高效）
    if state
        .funasr_starting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("FunASR 服务器正在启动中，跳过重复启动");
        return Ok(());
    }

    // 记录启动时的代数，后续写入 state 前比对，防止被 stop_server 取消后仍写回旧进程
    let gen_at_start = state.funasr_generation.load(Ordering::SeqCst);

    // 确保无论成功还是失败，都要重置 starting 标志
    let _starting_guard = StartingFlagGuard(state.funasr_starting.clone());
    state.set_funasr_ready(false);
    state.set_inline_audio_transport(None);

    // 通知前端：正在查找引擎环境
    let _ = app_handle.emit(
        "funasr-status",
        serde_json::json!({
            "status": "loading",
            "message": "正在查找引擎环境..."
        }),
    );

    // 查找引擎运行时
    let runtime = find_engine(app_handle).await?;
    let engine = paths::read_engine_config();

    // 解压或查找运行时期间可能被 stop_server 取消（例如用户切换引擎）。
    if state.funasr_generation.load(Ordering::SeqCst) != gen_at_start {
        log::warn!("引擎运行时已就绪但代数已变更，取消本次启动");
        return Err(AppError::Asr("启动已被取消".to_string()));
    }

    // 构建子进程命令
    let data_dir = paths::strip_win_prefix(paths::get_data_dir());
    let mut cmd = match &runtime {
        EngineRuntime::Bundled { exe_path } => {
            log::info!("使用打包引擎: {} (engine={})", exe_path, engine);
            let mut c = Command::new(exe_path);
            c.arg("serve").arg("--engine").arg(&engine);
            c
        }
        EngineRuntime::Development { python_path } => {
            log::info!("使用开发模式 Python: {}", python_path);
            let server_script = if engine == "whisper" {
                paths::get_whisper_server_path(app_handle)
            } else {
                paths::get_funasr_server_path(app_handle)
            };
            let server_script_str = paths::strip_win_prefix(&server_script);
            log::info!(
                "语音识别脚本路径 (engine={}): {}",
                engine,
                server_script_str
            );

            if !server_script.exists() {
                return Err(AppError::Asr(format!(
                    "FunASR 服务器脚本不存在: {}",
                    server_script_str
                )));
            }

            let mut c = Command::new(python_path);
            c.arg("-X").arg("utf8").arg("-u").arg(&server_script_str);
            c
        }
    };

    let models_dir = paths::strip_win_prefix(&paths::get_effective_models_dir());
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .env("HF_HUB_CACHE", &models_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr({
            let log_path = paths::get_data_dir().join("funasr_stderr.log");
            match std::fs::File::create(&log_path) {
                Ok(file) => {
                    log::info!("Python stderr 重定向到: {}", log_path.display());
                    std::process::Stdio::from(file)
                }
                Err(e) => {
                    log::warn!("无法创建 stderr 日志文件: {}，丢弃 stderr", e);
                    std::process::Stdio::null()
                }
            }
        });

    // Windows 上隐藏控制台窗口
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Asr(format!("启动 FunASR 进程失败: {}", e)))?;

    log::info!("FunASR 子进程已启动，等待初始化...");

    // 通知前端：正在加载语音识别模型
    let _ = app_handle.emit(
        "funasr-status",
        serde_json::json!({
            "status": "loading",
            "message": "正在加载语音识别模型..."
        }),
    );

    // 取出 stdin/stdout 句柄（后续由 FunasrProcess 持有）
    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = child.kill().await;
            return Err(AppError::Asr("无法获取 FunASR 进程的标准输入".to_string()));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill().await;
            return Err(AppError::Asr("无法获取 FunASR 进程的标准输出".to_string()));
        }
    };

    // 读取子进程初始化输出，跳过非 JSON 行，直到拿到有效响应
    let mut stdout_reader = BufReader::new(stdout);
    let response: ServerResponse = match read_json_response(
        &mut stdout_reader,
        Duration::from_secs(SERVER_INIT_TIMEOUT_SECS),
        "FunASR 初始化",
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            let _ = child.kill().await;
            return Err(err);
        }
    };

    let model_loaded = response.is_model_loaded();
    let initialized = response.initialized.unwrap_or(false)
        || response.success.unwrap_or(false)
        || response.status.as_deref() == Some("ready")
        || model_loaded;

    let error_message = response
        .error
        .clone()
        .or_else(|| response.message.clone())
        .unwrap_or_else(|| "FunASR 初始化失败".to_string());

    if initialized {
        // 检查启动期间是否被 stop_server 取消（引擎切换 / 重启）
        if state.funasr_generation.load(Ordering::SeqCst) != gen_at_start {
            log::warn!("FunASR 初始化完成但代数已变更，丢弃旧进程");
            let _ = child.kill().await;
            return Err(AppError::Asr("启动已被取消".to_string()));
        }

        log::info!("FunASR 服务器初始化成功！");
        state.set_funasr_ready(true);

        // 只有初始化成功才把子进程存入 state
        {
            let mut process_guard = state.funasr_process.lock().await;
            *process_guard = Some(FunasrProcess {
                child,
                stdin,
                stdout: stdout_reader,
            });
        }
    } else {
        log::error!("FunASR 初始化失败: {}", error_message);
        state.set_funasr_ready(false);
        // 初始化失败，杀掉子进程，不存入 state，允许后续重试
        let _ = child.kill().await;
    }

    // 通过 Tauri 事件系统通知前端
    // `emit` 会向所有窗口广播事件
    let _ = app_handle.emit(
        "funasr-status",
        if initialized {
            serde_json::json!({
                "status": "ready",
                "message": "FunASR 服务器已就绪",
                "device": response.device,
                "gpu_name": response.gpu_name,
                "models_present": true,
                "missing_models": [],
            })
        } else {
            serde_json::json!({
                "status": "error",
                "message": &error_message
            })
        },
    );

    if initialized {
        Ok(())
    } else {
        Err(AppError::Asr(error_message))
    }
}

/// 执行语音转写
///
/// 将音频数据写入临时 WAV 文件，然后通过 stdin 发送转写命令给 Python 进程，
/// 并从 stdout 读取转写结果。
///
/// # 参数
/// - `state`：全局应用状态
/// - `audio_data`：WAV 格式的音频数据（字节数组）
///
/// # 流程
/// ```text
/// 音频数据 -> 临时文件 -> 发送命令给 Python -> 等待结果 -> 返回文本
/// ```
pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let hot_words = profile_hot_words(state);
    transcribe_wav_bytes_via_path(state, audio_data, hot_words, app_handle).await
}

pub async fn transcribe_pcm16(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    // 检查服务器是否就绪
    if !state.is_funasr_ready() {
        return Err(AppError::Asr(
            "FunASR 服务器尚未就绪，请等待初始化完成".to_string(),
        ));
    }

    let hot_words = profile_hot_words(state);

    if state.inline_audio_transport() == Some(false) {
        return transcribe_pcm16_via_path(state, samples, sample_rate, hot_words, app_handle).await;
    }

    let response = send_command_to_server(
        state,
        &ServerCommand::Transcribe {
            audio_path: None,
            audio_base64: Some(encode_pcm16_base64(samples)),
            audio_format: Some(INLINE_AUDIO_FORMAT_PCM_S16LE.to_string()),
            sample_rate: Some(sample_rate),
            hot_words: hot_words.clone(),
        },
        Some(app_handle),
    )
    .await?;

    if response.input_mode.as_deref() == Some("memory") {
        state.set_inline_audio_transport(Some(true));
        return Ok(server_response_to_transcription_result(response));
    }

    if response_indicates_inline_unsupported(&response) {
        log::info!("当前 FunASR 运行时不支持内存音频，回退到临时 WAV 文件");
        state.set_inline_audio_transport(Some(false));
        return transcribe_pcm16_via_path(state, samples, sample_rate, hot_words, app_handle).await;
    }

    state.set_inline_audio_transport(Some(true));
    Ok(server_response_to_transcription_result(response))
}

fn profile_hot_words(state: &AppState) -> Option<Vec<String>> {
    let words = state.with_profile(|p| p.get_hot_word_texts(100));
    (!words.is_empty()).then_some(words)
}

fn encode_pcm16_base64(samples: &[i16]) -> String {
    use base64::Engine;

    let mut audio_bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        audio_bytes.extend_from_slice(&sample.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(audio_bytes)
}

fn encode_wav_bytes(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AppError> {
    Ok(super::audio_service::encode_wav(samples, sample_rate))
}

fn create_temp_audio_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "light_whisper_audio_{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ))
}

fn response_indicates_inline_unsupported(response: &ServerResponse) -> bool {
    if response.input_mode.as_deref() == Some("memory") {
        return false;
    }

    if response.input_mode.is_none() || response.input_mode.as_deref() == Some("path") {
        return true;
    }

    response.error.as_deref().is_some_and(|error| {
        error.contains("音频文件不存在")
            || error.contains("path should be string")
            || error.contains("os.PathLike")
            || error.contains("NoneType")
    })
}

fn server_response_to_transcription_result(response: ServerResponse) -> TranscriptionResult {
    if response.success == Some(true) {
        TranscriptionResult {
            text: response.text.unwrap_or_default(),
            duration: response.duration,
            success: true,
            error: None,
            language: response.language,
        }
    } else {
        let error_msg = response
            .error
            .unwrap_or_else(|| "未知的转写错误".to_string());
        TranscriptionResult {
            text: String::new(),
            duration: None,
            success: false,
            error: Some(error_msg),
            language: None,
        }
    }
}

async fn transcribe_wav_bytes_via_path(
    state: &AppState,
    audio_data: Vec<u8>,
    hot_words: Option<Vec<String>>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let temp_file = create_temp_audio_path();

    tokio::fs::write(&temp_file, &audio_data)
        .await
        .map_err(|e| AppError::Asr(format!("写入临时音频文件失败: {}", e)))?;

    let response = send_command_to_server(
        state,
        &ServerCommand::Transcribe {
            audio_path: Some(temp_file.to_string_lossy().to_string()),
            audio_base64: None,
            audio_format: None,
            sample_rate: None,
            hot_words,
        },
        Some(app_handle),
    )
    .await;

    let _ = tokio::fs::remove_file(&temp_file).await;
    response.map(server_response_to_transcription_result)
}

async fn transcribe_pcm16_via_path(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    hot_words: Option<Vec<String>>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let wav_bytes = encode_wav_bytes(samples, sample_rate)?;
    transcribe_wav_bytes_via_path(state, wav_bytes, hot_words, app_handle).await
}

/// 向 Python 服务器发送命令并读取响应
///
/// 这是与 Python 进程通信的核心函数。
///
/// # 通信流程
/// 1. 从全局状态中取出子进程（释放锁）
/// 2. 将命令序列化为 JSON
/// 3. 通过 stdin 写入 JSON + 换行符
/// 4. 从 stdout 读取一行 JSON 响应
/// 5. 把子进程放回全局状态
/// 6. 反序列化响应并返回
///
/// # 注意事项
/// - 每条消息必须以换行符结尾
/// - 命令和响应都是单行 JSON
/// - 为了保证同一时间只有一个命令与子进程通信，
///   这里会在 I/O 完成前保持锁，避免并发读写导致协议错乱。
async fn send_command_to_server(
    state: &AppState,
    command: &ServerCommand,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<ServerResponse, AppError> {
    let mut guard = state.funasr_process.lock().await;

    let result = {
        let process = guard
            .as_mut()
            .ok_or_else(|| AppError::Asr("FunASR 进程未运行".to_string()))?;
        send_command_impl(process, command).await
    };

    if result.is_err() {
        if let Some(process) = guard.as_mut() {
            if let Ok(Some(status)) = process.child.try_wait() {
                log::warn!("FunASR 进程已退出，状态码: {}", status);
                state.set_funasr_ready(false);
                *guard = None;
                // 主动通知前端进程已崩溃
                if let Some(handle) = app_handle {
                    let _ = handle.emit(
                        "funasr-status",
                        serde_json::json!({
                            "status": "crashed",
                            "message": format!("FunASR 进程异常退出（状态码: {}），正在准备重启...", status)
                        }),
                    );
                }
            }
        }
    }

    result
}

/// 向子进程发送命令并读取响应的内部实现
///
/// 把实际的 I/O 操作分离出来，这样 `send_command_to_server` 可以
/// 在锁释放后安全地调用这个异步函数。
async fn send_command_impl(
    process: &mut FunasrProcess,
    command: &ServerCommand,
) -> Result<ServerResponse, AppError> {
    // 序列化命令为 Python 端期望的扁平 JSON 格式
    let command_json = serde_json::to_string(command)
        .map_err(|e| AppError::Asr(format!("序列化命令失败: {}", e)))?;

    // 写入命令到 stdin
    // `write_all` 确保所有字节都被写入
    process
        .stdin
        .write_all(format!("{}\n", command_json).as_bytes())
        .await
        .map_err(|e| AppError::Asr(format!("写入命令到 FunASR 失败: {}", e)))?;

    // `flush` 确保缓冲区的数据被立即发送
    process
        .stdin
        .flush()
        .await
        .map_err(|e| AppError::Asr(format!("刷新 stdin 缓冲区失败: {}", e)))?;

    // 从 stdout 读取响应（允许跳过非 JSON 行）
    read_json_response(
        &mut process.stdout,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
        "等待 FunASR 响应",
    )
    .await
}

async fn try_send_exit_command(process: &mut FunasrProcess) -> Result<(), AppError> {
    let command_json = serde_json::to_string(&ServerCommand::Exit)
        .map_err(|e| AppError::Asr(format!("序列化退出命令失败: {}", e)))?;
    let timeout = Duration::from_millis(SERVER_EXIT_WRITE_TIMEOUT_MS);

    tokio::time::timeout(
        timeout,
        process
            .stdin
            .write_all(format!("{}\n", command_json).as_bytes()),
    )
    .await
    .map_err(|_| AppError::Asr("写入退出命令超时".to_string()))?
    .map_err(|e| AppError::Asr(format!("写入退出命令失败: {}", e)))?;

    tokio::time::timeout(timeout, process.stdin.flush())
        .await
        .map_err(|_| AppError::Asr("刷新退出命令超时".to_string()))?
        .map_err(|e| AppError::Asr(format!("刷新退出命令失败: {}", e)))?;

    Ok(())
}

/// 检查 FunASR 服务器的状态
///
/// 发送 status 命令给 Python 服务器，获取当前的运行状态。
pub async fn check_status(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<FunASRStatus, AppError> {
    // 先检查进程是否存在
    let has_process = {
        let guard = state.funasr_process.lock().await;
        guard.is_some()
    };

    // 如果进程句柄不存在，检查是否正在启动中
    if !has_process {
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            let has_key = !state.read_online_asr_api_key().is_empty();
            return Ok(FunASRStatus {
                running: true,
                ready: has_key,
                model_loaded: true,
                device: Some("cloud".into()),
                gpu_name: None,
                gpu_memory_total: None,
                message: if has_key {
                    "GLM-ASR 在线服务就绪".into()
                } else {
                    "请配置 GLM-ASR API Key".into()
                },
                engine: Some(engine),
                models_present: Some(true),
                missing_models: Some(Vec::new()),
            });
        }

        use std::sync::atomic::Ordering;
        if state.funasr_starting.load(Ordering::SeqCst) {
            // 正在启动中（模型加载中），告诉前端"正在运行但还没准备好"
            return Ok(status_with_defaults(
                true,
                false,
                false,
                "FunASR 服务器正在启动，模型加载中...".to_string(),
            ));
        }
        let model_check = inspect_model_files_for_engine(&engine);
        return Ok(FunASRStatus {
            message: if model_check.all_present {
                "FunASR 服务器未运行".to_string()
            } else {
                "模型文件未下载，请先下载模型".to_string()
            },
            engine: Some(engine),
            models_present: Some(model_check.all_present),
            missing_models: Some(model_check.missing_models.clone()),
            ..status_with_defaults(false, false, false, String::new())
        });
    }

    // 发送状态查询命令
    match send_command_to_server(state, &ServerCommand::Status, Some(app_handle)).await {
        Ok(response) => {
            let model_loaded = response.is_model_loaded();

            let initialized = response.initialized.unwrap_or(false) || model_loaded;
            if initialized {
                state.set_funasr_ready(true);
            }

            let ready = state.is_funasr_ready() || initialized;
            let message = response
                .message
                .or(response.error)
                .unwrap_or_else(|| "服务器运行中".to_string());

            Ok(FunASRStatus {
                running: true,
                ready,
                model_loaded,
                device: response.device,
                gpu_name: response.gpu_name,
                gpu_memory_total: response.gpu_memory_total,
                message,
                engine: response.engine,
                models_present: Some(true),
                missing_models: Some(Vec::new()),
            })
        }
        Err(e) => {
            // 发送命令失败，可能进程已崩溃
            log::warn!("查询 FunASR 状态失败: {}", e);
            state.set_funasr_ready(false);
            Ok(status_with_defaults(
                false,
                false,
                false,
                format!("服务器通信失败: {}", e),
            ))
        }
    }
}

/// 停止 FunASR 服务器
///
/// 优雅关闭流程：
/// 1. 先发送 exit 命令让 Python 进程自行退出
/// 2. 等待一小段时间
/// 3. 如果进程仍在运行，强制杀死
///
pub async fn stop_server(state: &AppState) -> Result<(), AppError> {
    // 递增代数，使正在进行的 start_server 感知到取消
    state.funasr_generation.fetch_add(1, Ordering::SeqCst);

    // 先取出子进程句柄，避免关闭流程被常规请求超时拖住
    let mut process = {
        let mut guard = state.funasr_process.lock().await;
        guard.take()
    };

    // 如果有子进程，确保它被终止
    if let Some(ref mut child_process) = process {
        if let Err(err) = try_send_exit_command(child_process).await {
            log::debug!("发送 FunASR 退出命令失败，准备直接等待/终止进程: {}", err);
        }

        let wait_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(SERVER_EXIT_WAIT_TIMEOUT_SECS),
            child_process.child.wait(),
        )
        .await;

        match wait_result {
            Ok(Ok(status)) => {
                log::info!("FunASR 进程已退出，状态码: {}", status);
            }
            _ => {
                // 超时或出错，强制杀死进程
                log::warn!("FunASR 进程未响应退出命令，强制终止...");
                if let Err(e) = child_process.child.kill().await {
                    log::warn!("强制终止 FunASR 进程失败: {}", e);
                }
            }
        }
    }

    // 更新状态
    state.set_funasr_ready(false);
    state.set_inline_audio_transport(None);

    log::info!("FunASR 服务器已停止");
    Ok(())
}

/// 获取 HuggingFace 缓存根目录
///
/// 优先使用用户自定义模型目录，其次按 HuggingFace 标准规则。
fn get_hf_cache_root() -> PathBuf {
    paths::get_effective_models_dir()
}

/// 检查 HuggingFace 模型是否已缓存且包含实际模型权重文件
///
/// 仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshots 存在但无权重文件），
/// 导致后续加载卡死。这里额外验证 snapshots 中存在 >1MB 的模型权重文件（.pt/.bin/.safetensors/.onnx）。
fn is_hf_repo_ready(repo_id: &str) -> bool {
    let cache_root = get_hf_cache_root();
    let dir_name = format!("models--{}", repo_id.replace('/', "--"));
    let repo_dir = cache_root.join(&dir_name);

    log::info!(
        "模型检查: repo={}, cache_root={}, repo_dir={}, exists={}",
        repo_id,
        cache_root.display(),
        repo_dir.display(),
        repo_dir.is_dir()
    );

    if !repo_dir.is_dir() {
        log::warn!("模型目录不存在: {}", repo_dir.display());
        return false;
    }

    let snapshots_dir = repo_dir.join("snapshots");
    let entries = match std::fs::read_dir(&snapshots_dir) {
        Ok(e) => e,
        Err(err) => {
            log::warn!("无法读取 snapshots 目录: {} — {}", snapshots_dir.display(), err);
            return false;
        }
    };

    const MIN_SIZE: u64 = 1_000_000; // 1MB
    let weight_exts: &[&str] = &[".pt", ".bin", ".safetensors", ".onnx"];

    for entry in entries.filter_map(Result::ok) {
        let snapshot_path = entry.path();
        if !snapshot_path.is_dir() {
            continue;
        }
        log::info!("检查 snapshot: {}", snapshot_path.display());
        // 递归遍历 snapshot 目录查找模型权重文件
        if has_weight_file(&snapshot_path, weight_exts, MIN_SIZE) {
            log::info!("模型就绪: {} (在 {})", repo_id, snapshot_path.display());
            return true;
        }
    }

    log::warn!(
        "模型未就绪: {} — snapshots 中未找到 >1MB 的权重文件 ({:?})",
        repo_id,
        weight_exts
    );
    false
}

/// 递归检查目录中是否存在符合条件的模型权重文件
fn has_weight_file(dir: &std::path::Path, exts: &[&str], min_size: u64) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            if has_weight_file(&path, exts, min_size) {
                return true;
            }
        } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if exts.iter().any(|ext| name.ends_with(ext)) {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if meta.len() >= min_size {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 检查模型文件是否已下载
///
/// 检查 HuggingFace 缓存中是否存在 SenseVoiceSmall 相关模型：
/// - `FunAudioLLM/SenseVoiceSmall` + `funasr/fsmn-vad`
///
/// 注：SenseVoiceSmall 内置 ITN 标点恢复，不再需要独立的 ct-punc 模型
fn inspect_model_files_for_engine(engine: &str) -> ModelCheckResult {
    if paths::is_online_engine(engine) {
        return ModelCheckResult {
            all_present: true,
            asr_model: true,
            vad_model: true,
            punc_model: true,
            engine: engine.to_string(),
            cache_path: String::new(),
            missing_models: Vec::new(),
        };
    }

    let cache_root = get_hf_cache_root();
    let cache_path = cache_root.to_string_lossy().to_string();

    if engine == "whisper" {
        // Whisper 引擎：只需检查一个模型仓库，内置 VAD 和标点
        let mut missing_models = Vec::new();
        let asr_present =
            report_model_repo_state(WHISPER_REPO_ID, "Whisper ASR模型", &mut missing_models);

        ModelCheckResult {
            all_present: asr_present,
            asr_model: asr_present,
            vad_model: true,  // Whisper 内置 Silero VAD
            punc_model: true, // Whisper 内置标点
            engine: "whisper".to_string(),
            cache_path,
            missing_models,
        }
    } else {
        // SenseVoice 引擎：检查 ASR + VAD 模型
        let mut missing_models = Vec::new();
        let asr_present =
            report_model_repo_state(ASR_REPO_ID, "ASR语音识别模型", &mut missing_models);
        let vad_present =
            report_model_repo_state(VAD_REPO_ID, "VAD语音活动检测模型", &mut missing_models);

        let all_present = asr_present && vad_present;

        ModelCheckResult {
            all_present,
            asr_model: asr_present,
            vad_model: vad_present,
            punc_model: true, // SenseVoiceSmall 内置 ITN，无需独立标点模型
            engine: "sensevoice".to_string(),
            cache_path,
            missing_models,
        }
    }
}

pub async fn check_model_files() -> Result<ModelCheckResult, AppError> {
    Ok(inspect_model_files_for_engine(&paths::read_engine_config()))
}

// 需要引入 Emitter trait 才能使用 emit 方法
use tauri::Emitter;

#[cfg(test)]
mod tests {
    use super::{read_json_response, ServerResponse};
    use std::time::Duration;
    use tokio::io::{AsyncWriteExt, BufReader};

    async fn read_response_from_chunks(chunks: &[&[u8]]) -> ServerResponse {
        let (mut writer, reader) = tokio::io::duplex(1024);
        for chunk in chunks {
            writer.write_all(chunk).await.unwrap();
        }
        writer.shutdown().await.unwrap();

        let mut reader = BufReader::new(reader);
        read_json_response(&mut reader, Duration::from_secs(1), "test")
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn read_json_response_accepts_valid_json_line() {
        let response = read_response_from_chunks(&[br#"{"success":true,"message":"ok"}\n"#]).await;

        assert_eq!(response.success, Some(true));
        assert_eq!(response.message.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn read_json_response_recovers_from_non_utf8_prefix() {
        let response =
            read_response_from_chunks(&[b"\xff\xfe", br#"{"success":true,"message":"ok"}\n"#])
                .await;

        assert_eq!(response.success, Some(true));
        assert_eq!(response.message.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn read_json_response_recovers_json_from_mixed_line() {
        let response =
            read_response_from_chunks(&[br#"noise >>> {"success":true,"message":"ok"}\n"#]).await;

        assert_eq!(response.success, Some(true));
        assert_eq!(response.message.as_deref(), Some("ok"));
    }

    #[tokio::test]
    async fn read_json_response_skips_python_dict_noise_and_reads_next_json() {
        let response = read_response_from_chunks(&[
            b"{'load_data': '0.001'}\n",
            br#"{"success":true,"message":"ok"}\n"#,
        ])
        .await;

        assert_eq!(response.success, Some(true));
        assert_eq!(response.message.as_deref(), Some("ok"));
    }
}
