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
        audio_path: String,
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
    let mut line = String::new();

    loop {
        let remaining = timeout
            .checked_sub(start_at.elapsed())
            .ok_or_else(|| AppError::FunASR(format!("{}超时", context)))?;

        line.clear();
        let read_result = tokio::time::timeout(remaining, reader.read_line(&mut line)).await;

        match read_result {
            Ok(Ok(0)) => {
                return Err(AppError::FunASR(format!("{}失败：stdout 已关闭", context)));
            }
            Ok(Ok(_)) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                match serde_json::from_str::<T>(trimmed) {
                    Ok(value) => return Ok(value),
                    Err(_) => {
                        log::warn!("{}阶段收到非JSON输出: {}", context, trimmed);
                        continue;
                    }
                }
            }
            Ok(Err(e)) => {
                return Err(AppError::FunASR(format!("{}失败：{}", context, e)));
            }
            Err(_) => {
                return Err(AppError::FunASR(format!("{}超时", context)));
            }
        }
    }
}

// ============================================================
// 核心功能实现
// ============================================================

/// 查找可用的 Python 解释器
///
/// 按以下优先级搜索 Python：
/// 1. 项目虚拟环境中的 Python（.venv/Scripts/python.exe）
/// 2. 系统 PATH 中的 python / python3
///
/// # 返回值
/// - `Ok(String)`：找到的 Python 可执行文件路径
/// - `Err(AppError)`：没有找到任何可用的 Python
pub async fn find_python() -> Result<String, AppError> {
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
    Err(AppError::FunASR(
        "未找到可用的 Python 解释器。请安装 Python 3.8+ 或在项目目录创建 .venv 虚拟环境（推荐使用 uv）。"
            .to_string(),
    ))
}

/// 启动 FunASR Python 服务器
///
/// 这个函数做以下事情：
/// 1. 查找 Python 解释器
/// 2. 以子进程方式启动 funasr_server.py
/// 3. 等待服务器初始化完成（读取第一行 JSON 输出）
/// 4. 把子进程句柄存储到全局状态中
///
/// # 参数
/// - `app_handle`：Tauri 应用句柄，用于获取资源路径和发送事件
/// - `state`：全局应用状态的引用
///
/// # Rust 知识点：async/await
/// `async fn` 定义一个异步函数。异步函数不会阻塞当前线程，
/// 而是在等待 IO 操作时让出执行权给其他任务。
/// `await` 关键字用于等待异步操作完成。
///
/// 为什么要用异步？因为启动进程和等待初始化可能需要几秒钟，
/// 如果用同步方式，整个 UI 线程会被阻塞，导致界面卡死。
pub async fn start_server(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), AppError> {
    // 先检查是否已经有运行中的服务器或正在启动中
    {
        let process_guard = state.funasr_process.lock().await;
        if process_guard.is_some() {
            log::warn!("FunASR 服务器已在运行中");
            return Ok(());
        }
    }

    // 使用原子标志防止并发启动
    // `compare_exchange` 是原子操作：如果当前值是 false，就设为 true 并返回 Ok；
    // 如果已经是 true（说明另一个启动流程正在进行），就返回 Err。
    // 这比持有 Mutex 锁更高效，因为模型加载可能需要 25+ 秒。
    if state
        .funasr_starting
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::info!("FunASR 服务器正在启动中，跳过重复启动");
        return Ok(());
    }

    // 确保无论成功还是失败，都要重置 starting 标志
    let _starting_guard = StartingFlagGuard(state.funasr_starting.clone());
    state.set_funasr_ready(false);

    // 通知前端：正在启动 Python 服务
    let _ = app_handle.emit(
        "funasr-status",
        serde_json::json!({
            "status": "loading",
            "message": "正在查找 Python 环境..."
        }),
    );

    // 查找 Python 解释器
    let python_path = find_python().await?;
    log::info!("使用 Python: {}", python_path);

    // 根据引擎配置选择对应的 Python 脚本
    let engine = paths::read_engine_config();
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
        return Err(AppError::FunASR(format!(
            "FunASR 服务器脚本不存在: {}",
            server_script_str
        )));
    }

    // 构建子进程命令
    let data_dir = paths::strip_win_prefix(&paths::get_data_dir());
    let mut cmd = Command::new(&python_path);
    cmd.arg("-u")
        .arg(&server_script_str)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr({
            // 将 stderr 重定向到日志文件，便于调试 Python 错误
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
        .map_err(|e| AppError::FunASR(format!("启动 FunASR 进程失败: {}", e)))?;

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
            return Err(AppError::FunASR(
                "无法获取 FunASR 进程的标准输入".to_string(),
            ));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill().await;
            return Err(AppError::FunASR(
                "无法获取 FunASR 进程的标准输出".to_string(),
            ));
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
        log::info!("FunASR 服务器初始化成功！");
        state.set_funasr_ready(true);
    } else {
        log::error!("FunASR 初始化失败: {}", error_message);
        state.set_funasr_ready(false);
    }

    // 把子进程句柄存储到全局状态中
    {
        let mut process_guard = state.funasr_process.lock().await;
        *process_guard = Some(FunasrProcess {
            child,
            stdin,
            stdout: stdout_reader,
        });
    }

    // 通过 Tauri 事件系统通知前端
    // `emit` 会向所有窗口广播事件
    let event_payload = if initialized {
        serde_json::json!({
            "status": "ready",
            "message": "FunASR 服务器已就绪"
        })
    } else {
        serde_json::json!({
            "status": "error",
            "message": error_message
        })
    };
    let _ = app_handle.emit("funasr-status", event_payload);

    Ok(())
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
///
/// # Rust 知识点：Vec<u8>
/// `Vec<u8>` 是一个字节数组，用于存储二进制数据（如音频文件内容）。
/// `u8` 是无符号 8 位整数（0-255），一个字节。
pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    // 检查服务器是否就绪
    if !state.is_funasr_ready() {
        return Err(AppError::FunASR(
            "FunASR 服务器尚未就绪，请等待初始化完成".to_string(),
        ));
    }

    // 将音频数据写入临时文件
    //
    // 为什么要用临时文件？因为通过 stdin 传递大量二进制数据比较复杂，
    // 而文件路径是一个简单的字符串，通过 JSON 传递很方便。
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!(
        "light_whisper_audio_{}.wav",
        // 使用时间戳作为文件名的一部分，避免文件名冲突
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));

    // 写入音频数据到临时文件
    tokio::fs::write(&temp_file, &audio_data)
        .await
        .map_err(|e| AppError::FunASR(format!("写入临时音频文件失败: {}", e)))?;

    // 构建转写命令
    let command = ServerCommand::Transcribe {
        audio_path: temp_file.to_string_lossy().to_string(),
    };

    // 发送命令并获取响应（无论成功与否都清理临时文件）
    let response = send_command_to_server(state, &command, Some(app_handle)).await;
    let _ = tokio::fs::remove_file(&temp_file).await;
    let response = response?;

    // 解析响应
    if response.success == Some(true) {
        Ok(TranscriptionResult {
            text: response.text.unwrap_or_default(),
            duration: response.duration,
            success: true,
            error: None,
        })
    } else {
        let error_msg = response
            .error
            .unwrap_or_else(|| "未知的转写错误".to_string());
        Ok(TranscriptionResult {
            text: String::new(),
            duration: None,
            success: false,
            error: Some(error_msg),
        })
    }
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
            .ok_or_else(|| AppError::FunASR("FunASR 进程未运行".to_string()))?;
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
        .map_err(|e| AppError::FunASR(format!("序列化命令失败: {}", e)))?;

    // 写入命令到 stdin
    // `write_all` 确保所有字节都被写入
    process
        .stdin
        .write_all(format!("{}\n", command_json).as_bytes())
        .await
        .map_err(|e| AppError::FunASR(format!("写入命令到 FunASR 失败: {}", e)))?;

    // `flush` 确保缓冲区的数据被立即发送
    process
        .stdin
        .flush()
        .await
        .map_err(|e| AppError::FunASR(format!("刷新 stdin 缓冲区失败: {}", e)))?;

    // 从 stdout 读取响应（允许跳过非 JSON 行）
    read_json_response(
        &mut process.stdout,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
        "等待 FunASR 响应",
    )
    .await
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
        return Ok(status_with_defaults(
            false,
            false,
            false,
            "FunASR 服务器未运行".to_string(),
        ));
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
/// # Rust 知识点：Option 的 take 方法
/// `take()` 把 Option 中的值取出来，原位置变成 None。
/// 这在需要获取所有权时很有用。
pub async fn stop_server(state: &AppState) -> Result<(), AppError> {
    // 先尝试发送退出命令
    let _ = send_command_to_server(state, &ServerCommand::Exit, None).await;

    // 取出子进程句柄
    let mut child = {
        let mut guard = state.funasr_process.lock().await;
        guard.take() // 取出 Option 中的值，留下 None
    };

    // 如果有子进程，确保它被终止
    if let Some(ref mut child_process) = child {
        // 等待 2 秒让进程自然退出
        let wait_result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
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

    log::info!("FunASR 服务器已停止");
    Ok(())
}

/// 获取 HuggingFace 缓存根目录
///
/// 按照 HuggingFace 的标准缓存路径规则：
/// 1. `HF_HOME` 环境变量 + `/hub/`
/// 2. `~/.cache/huggingface/hub/`
fn get_hf_cache_root() -> PathBuf {
    if let Ok(hf_home) = std::env::var("HF_HOME") {
        return PathBuf::from(hf_home).join("hub");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".cache").join("huggingface").join("hub");
    }
    PathBuf::from(".cache").join("huggingface").join("hub")
}

/// 检查 HuggingFace 模型是否已缓存且包含实际模型权重文件
///
/// 仅检查目录结构不够——下载中途取消会留下空壳目录（refs/snapshots 存在但无权重文件），
/// 导致后续加载卡死。这里额外验证 snapshots 中存在 >1MB 的模型权重文件（.pt/.bin/.safetensors/.onnx）。
fn is_hf_repo_ready(repo_id: &str) -> bool {
    let cache_root = get_hf_cache_root();
    let dir_name = format!("models--{}", repo_id.replace('/', "--"));
    let repo_dir = cache_root.join(&dir_name);
    if !repo_dir.is_dir() {
        return false;
    }

    let snapshots_dir = repo_dir.join("snapshots");
    let entries = match std::fs::read_dir(&snapshots_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    const MIN_SIZE: u64 = 1_000_000; // 1MB
    let weight_exts: &[&str] = &[".pt", ".bin", ".safetensors", ".onnx"];

    for entry in entries.filter_map(Result::ok) {
        let snapshot_path = entry.path();
        if !snapshot_path.is_dir() {
            continue;
        }
        // 递归遍历 snapshot 目录查找模型权重文件
        if has_weight_file(&snapshot_path, weight_exts, MIN_SIZE) {
            return true;
        }
    }

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
/// 注：SenseVoiceSmall 内置 ITN 标点恢复，不再需要独立的 ct-punc 模型
pub async fn check_model_files() -> Result<ModelCheckResult, AppError> {
    let engine = paths::read_engine_config();
    let cache_root = get_hf_cache_root();
    let cache_path = cache_root.to_string_lossy().to_string();

    if engine == "whisper" {
        // Whisper 引擎：只需检查一个模型仓库，内置 VAD 和标点
        let mut missing_models = Vec::new();
        let asr_present =
            report_model_repo_state(WHISPER_REPO_ID, "Whisper ASR模型", &mut missing_models);

        Ok(ModelCheckResult {
            all_present: asr_present,
            asr_model: asr_present,
            vad_model: true,  // Whisper 内置 Silero VAD
            punc_model: true, // Whisper 内置标点
            engine: "whisper".to_string(),
            cache_path,
            missing_models,
        })
    } else {
        // SenseVoice 引擎：检查 ASR + VAD 模型
        let mut missing_models = Vec::new();
        let asr_present =
            report_model_repo_state(ASR_REPO_ID, "ASR语音识别模型", &mut missing_models);
        let vad_present =
            report_model_repo_state(VAD_REPO_ID, "VAD语音活动检测模型", &mut missing_models);

        let all_present = asr_present && vad_present;

        Ok(ModelCheckResult {
            all_present,
            asr_model: asr_present,
            vad_model: vad_present,
            punc_model: true, // SenseVoiceSmall 内置 ITN，无需独立标点模型
            engine: "sensevoice".to_string(),
            cache_path,
            missing_models,
        })
    }
}

// 需要引入 Emitter trait 才能使用 emit 方法
use tauri::Emitter;
