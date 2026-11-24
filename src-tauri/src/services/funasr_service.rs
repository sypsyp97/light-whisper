use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::state::{AppState, EngineState, FunasrProcess, StartingFunasrProcess};
use crate::utils::paths;
use crate::utils::AppError;

mod installation;
pub(crate) mod native_stream;
mod protocol;

#[cfg(test)]
mod native_stream_tests;

#[cfg(all(test, target_os = "windows"))]
mod native_stream_hotword_tests;

#[cfg(all(test, target_os = "windows"))]
pub(crate) mod ipc_recovery_tests;

#[cfg(all(test, target_os = "windows"))]
mod audio_processing_start_tests;

pub use installation::find_engine;
#[cfg(test)]
use installation::{engine_install_fingerprint_matches, replace_engine_dir};
pub use protocol::ServerCommand;
use protocol::{read_json_response, read_json_response_matching, ServerResponse};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,

    pub duration: Option<f64>,

    pub success: bool,

    pub error: Option<String>,

    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunASRStatus {
    pub running: bool,

    pub ready: bool,

    pub model_loaded: bool,

    pub device: Option<String>,

    pub gpu_name: Option<String>,

    pub gpu_memory_total: Option<f64>,

    pub message: String,

    pub engine: Option<String>,

    pub models_present: Option<bool>,

    pub missing_models: Option<Vec<String>>,
    pub model_state: Option<ModelState>,
    pub corrupt_models: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelState {
    Ready,
    Missing,
    Corrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCheckResult {
    pub all_present: bool,

    pub asr_model: bool,

    pub vad_model: bool,

    pub punc_model: bool,

    pub engine: String,

    pub cache_path: String,

    pub missing_models: Vec<String>,
    pub model_state: ModelState,
    pub corrupt_models: Vec<String>,
}

const QWEN3_ASR_06_REPO_ID: &str = "audio-cpp/audio.cpp-gguf";
const QWEN3_ASR_06_FILENAME: &str = "Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf";
const R2T2_REPO_ID: &str = "davidxifeng/Confucius4-R2T2-gguf";
const R2T2_FILENAME: &str = "r2t2-q8_0.gguf";
const HF_COMPLETE_MANIFEST_NAME: &str = ".light_whisper_complete.json";

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartTicket {
    owner: u64,
    generation: u64,
    engine: String,
}

#[derive(Clone)]
struct EngineProgressGate {
    generation: Arc<AtomicU64>,
    status_commit: Arc<parking_lot::Mutex<()>>,
    expected_generation: u64,
}

impl EngineProgressGate {
    fn is_current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.expected_generation
    }

    fn commit_if_current(&self, commit: impl FnOnce()) {
        let _status_guard = self.status_commit.lock();
        if self.is_current() {
            commit();
        }
    }
}

struct StartingFlagGuard<'a> {
    engine: &'a EngineState,
    owner: u64,
    child: Option<Arc<parking_lot::Mutex<Option<tokio::process::Child>>>>,
}

impl StartingFlagGuard<'_> {
    fn register_child(&mut self, child: Arc<parking_lot::Mutex<Option<tokio::process::Child>>>) {
        self.child = Some(child);
    }

    fn release_child(&mut self) {
        self.child = None;
    }
}

impl Drop for StartingFlagGuard<'_> {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            if let Some(mut child) = child.lock().take() {
                let _ = child.start_kill();
            }
        }
        let mut starting = self.engine.funasr_starting_process.lock();
        if starting
            .as_ref()
            .is_some_and(|entry| entry.owner == self.owner)
        {
            starting.take();
        }
        self.engine.finish_funasr_start(self.owner);
    }
}

const SERVER_INIT_TIMEOUT_SECS: u64 = 120;

const AUDIO_PROCESSING_BOOTSTRAP_ENABLED: bool = false;
const SERVER_RESPONSE_TIMEOUT_SECS: u64 = 60;
const SERVER_EXIT_WRITE_TIMEOUT_MS: u64 = 300;
const SERVER_EXIT_WAIT_TIMEOUT_SECS: u64 = 2;
const INLINE_AUDIO_FORMAT_PCM_S16LE: &str = "pcm_s16le";
const TARGET_SAMPLE_RATE_FOR_INLINE_AUDIO: u32 = 16_000;
const ENGINE_ARCHIVE_FINGERPRINT: &str = env!("LIGHT_WHISPER_ENGINE_ARCHIVE_FINGERPRINT");
static NEXT_SERVER_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SERVER_START_OWNER: AtomicU64 = AtomicU64::new(1);

fn next_server_start_owner() -> u64 {
    loop {
        let owner = NEXT_SERVER_START_OWNER.fetch_add(1, Ordering::Relaxed);
        if owner != 0 && owner != u64::MAX {
            return owner;
        }
    }
}

fn start_ticket_is_current(state: &AppState, ticket: &StartTicket) -> bool {
    state.engine.owns_funasr_start(ticket.owner)
        && state.engine.funasr_generation.load(Ordering::SeqCst) == ticket.generation
        && paths::read_engine_config() == ticket.engine
}

fn take_starting_child(
    state: &AppState,
    ticket: &StartTicket,
    expected_handle: &Arc<parking_lot::Mutex<Option<tokio::process::Child>>>,
) -> Option<tokio::process::Child> {
    let entry = {
        let mut slot = state.engine.funasr_starting_process.lock();
        let matches = slot.as_ref().is_some_and(|entry| {
            entry.owner == ticket.owner
                && entry.generation == ticket.generation
                && Arc::ptr_eq(&entry.child, expected_handle)
        });
        matches.then(|| slot.take()).flatten()
    }?;
    let child = entry.child.lock().take();
    child
}

fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
        model_state: None,
        corrupt_models: None,
    }
}

fn report_model_repo_file_state(
    repo_id: &str,
    required_file: Option<&str>,
    description: &str,
    missing_models: &mut Vec<String>,
    corrupt_models: &mut Vec<String>,
) -> ModelState {
    let model_state = hf_repo_model_state(repo_id, required_file);
    match model_state {
        ModelState::Ready => log::info!("模型文件已就位: {} ({})", description, repo_id),
        ModelState::Missing => missing_models.push(description.to_string()),
        ModelState::Corrupt => corrupt_models.push(description.to_string()),
    }
    model_state
}

fn configure_server_environment(cmd: &mut Command, engine: &str, gpu_idle_seconds: u64) {
    let data_dir = paths::strip_win_prefix(paths::get_data_dir());
    let models_dir = paths::strip_win_prefix(&paths::get_effective_models_dir());
    cmd.env("LIGHT_WHISPER_DATA_DIR", &data_dir)
        .env("LIGHT_WHISPER_ASR_ENGINE", engine)
        .env(
            "LIGHT_WHISPER_AUDIO_PROCESSING",
            if AUDIO_PROCESSING_BOOTSTRAP_ENABLED {
                "1"
            } else {
                "0"
            },
        )
        .env("HF_HUB_CACHE", &models_dir);
    if gpu_idle_seconds > 0 {
        cmd.env(
            "LIGHT_WHISPER_GPU_IDLE_SECONDS",
            gpu_idle_seconds.to_string(),
        );
    }
}

pub async fn start_server(app_handle: &tauri::AppHandle, state: &AppState) -> Result<(), AppError> {
    let (ticket, mut starting_guard) = {
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            return Ok(());
        }

        let process_guard = state.engine.funasr_process.lock().await;
        if process_guard.is_some() {
            log::warn!("FunASR 服务器已在运行中");
            return Ok(());
        }
        drop(process_guard);
        if state.engine.funasr_starting_process.lock().is_some() {
            log::info!("FunASR 子进程正在初始化中，跳过重复启动");
            return Ok(());
        }

        let start_owner = next_server_start_owner();
        if !state.engine.try_begin_funasr_start(start_owner) {
            log::info!("FunASR 服务器正在启动中，跳过重复启动");
            return Ok(());
        }

        let ticket = StartTicket {
            owner: start_owner,
            generation: state.engine.funasr_generation.load(Ordering::SeqCst),
            engine,
        };
        let starting_guard = StartingFlagGuard {
            engine: &state.engine,
            owner: start_owner,
            child: None,
        };

        state.set_funasr_ready(false);
        state.set_inline_audio_transport(None);
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "正在查找引擎环境..."
            }),
        );
        (ticket, starting_guard)
    };

    let runtime_result = find_engine(app_handle, state, ticket.generation).await;
    {
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("引擎运行时已就绪但启动票据已失效，取消本次启动");
            return Err(AppError::Asr("启动已被取消".to_string()));
        }
    }
    let runtime = runtime_result?;

    if !start_ticket_is_current(state, &ticket) {
        log::warn!("引擎运行时已就绪但代数已变更，取消本次启动");
        return Err(AppError::Asr("启动已被取消".to_string()));
    }

    let gpu_idle_at_spawn = paths::read_gpu_idle_seconds();
    log::info!("使用本地语音引擎: {} (engine={})", runtime, ticket.engine);
    let mut cmd = Command::new(runtime);
    cmd.arg("serve").arg("--engine").arg(&ticket.engine);
    configure_server_environment(&mut cmd, &ticket.engine, gpu_idle_at_spawn);
    cmd.stdin(std::process::Stdio::piped())
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

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let (stdin, stdout, child_handle) = {
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            return Err(AppError::Asr("启动已被取消".to_string()));
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| AppError::Asr(format!("启动 FunASR 进程失败: {}", e)))?;
        let stdin = child.stdin.take().ok_or_else(|| {
            let _ = child.start_kill();
            AppError::Asr("无法获取 FunASR 进程的标准输入".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            let _ = child.start_kill();
            AppError::Asr("无法获取 FunASR 进程的标准输出".to_string())
        })?;
        let child_handle = Arc::new(parking_lot::Mutex::new(Some(child)));
        *state.engine.funasr_starting_process.lock() = Some(StartingFunasrProcess {
            owner: ticket.owner,
            generation: ticket.generation,
            child: child_handle.clone(),
        });
        starting_guard.register_child(child_handle.clone());

        log::info!("FunASR 子进程已启动，等待初始化...");
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "loading",
                "message": "正在加载语音识别模型..."
            }),
        );
        (stdin, stdout, child_handle)
    };

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
            return if start_ticket_is_current(state, &ticket) {
                Err(err)
            } else {
                Err(AppError::Asr("启动已被取消".to_string()))
            };
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
        let ready_status = serde_json::json!({
            "status": "ready",
            "message": "FunASR 服务器已就绪",
            "device": response.device,
            "gpu_name": response.gpu_name,
            "models_present": true,
            "missing_models": [],
            "model_state": "ready",
            "corrupt_models": [],
        });
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("FunASR 初始化完成但启动票据已失效，丢弃旧进程");
            return Err(AppError::Asr("启动已被取消".to_string()));
        }

        let Some(child) = take_starting_child(state, &ticket, &child_handle) else {
            return Err(AppError::Asr("启动已被取消".to_string()));
        };
        let mut process = FunasrProcess {
            child,
            stdin,
            stdout: stdout_reader,
        };

        sync_audio_processing_at_start(&mut process, AUDIO_PROCESSING_BOOTSTRAP_ENABLED).await?;
        if !start_ticket_is_current(state, &ticket) {
            return Err(AppError::Asr("启动已被取消".to_string()));
        }
        let mut process_guard = state.engine.funasr_process.lock().await;
        if process_guard.is_some() {
            return Err(AppError::Asr("FunASR 服务器已由其他任务启动".to_string()));
        }
        *process_guard = Some(process);
        starting_guard.release_child();
        log::info!("FunASR 服务器初始化成功！");
        state.set_funasr_ready(true);
        let _ = app_handle.emit("funasr-status", ready_status);
    } else {
        log::error!("FunASR 初始化失败: {}", error_message);
        let _lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        if !start_ticket_is_current(state, &ticket) {
            log::warn!("FunASR 初始化失败结果已过期，忽略旧错误状态");
            return Err(AppError::Asr("启动已被取消".to_string()));
        }
        let Some(mut child) = take_starting_child(state, &ticket, &child_handle) else {
            return Err(AppError::Asr("启动已被取消".to_string()));
        };
        let _ = child.start_kill();
        starting_guard.release_child();
        state.set_funasr_ready(false);
        let _ = app_handle.emit(
            "funasr-status",
            serde_json::json!({
                "status": "error",
                "message": &error_message
            }),
        );
    }

    if initialized {
        let gpu_idle_now = paths::read_gpu_idle_seconds();

        if gpu_idle_now != gpu_idle_at_spawn {
            if let Err(error) = push_gpu_idle_seconds(state, gpu_idle_now).await {
                log::warn!("同步 GPU 空闲卸载设置失败: {error}");
            }
        }
        Ok(())
    } else {
        Err(AppError::Asr(error_message))
    }
}

pub async fn transcribe(
    state: &AppState,
    audio_data: Vec<u8>,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let _asr_guard = state.engine.native_asr_owner.lock().await;
    let hot_words = profile_hot_words(state);
    transcribe_wav_bytes_via_path(state, audio_data, hot_words, app_handle).await
}

pub(crate) async fn probe_model(
    state: &AppState,
    engine: &str,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let _asr_guard = state
        .engine
        .native_asr_owner
        .try_lock()
        .map_err(|_| AppError::Asr("语音识别正在进行，请稍后测试。".into()))?;
    if state.recording.recording.lock().is_some()
        || state
            .recording
            .snapshot()
            .is_some_and(|snapshot| snapshot.phase == crate::state::RecordingPhase::Processing)
    {
        return Err(AppError::Asr("语音识别正在进行，请稍后测试。".into()));
    }
    {
        let _configuration = state.engine.funasr_lifecycle_op.lock().await;
        if paths::read_engine_config() != engine {
            return Err(AppError::Asr("语音引擎已切换，请重新测试。".into()));
        }
    }
    transcribe_wav_bytes_via_path(
        state,
        super::model_connection::TEST_AUDIO.to_vec(),
        None,
        app_handle,
    )
    .await
}

pub async fn transcribe_pcm16(
    state: &AppState,
    samples: &[i16],
    sample_rate: u32,
    app_handle: &tauri::AppHandle,
) -> Result<TranscriptionResult, AppError> {
    let _asr_guard = state.engine.native_asr_owner.lock().await;

    if !state.is_funasr_ready() {
        return Err(AppError::Asr(
            "语音识别引擎尚未就绪，请等待初始化完成".to_string(),
        ));
    }

    const MIN_IPC_DURATION_SEC: f64 = 0.5;
    let min_samples_for_ipc = (sample_rate as f64 * MIN_IPC_DURATION_SEC) as usize;
    let padded_storage: Vec<i16>;
    let samples: &[i16] = if paths::read_engine_config() != "confucius4-r2t2"
        && samples.len() < min_samples_for_ipc
        && !samples.is_empty()
    {
        let mut v = Vec::with_capacity(min_samples_for_ipc);
        v.extend_from_slice(samples);
        v.resize(min_samples_for_ipc, 0);
        padded_storage = v;
        &padded_storage
    } else {
        samples
    };

    let hot_words = profile_hot_words(state);

    if sample_rate != TARGET_SAMPLE_RATE_FOR_INLINE_AUDIO {
        return transcribe_pcm16_via_path(state, samples, sample_rate, hot_words, app_handle).await;
    }

    if state.inline_audio_transport() == Some(false) {
        return transcribe_pcm16_via_path(state, samples, sample_rate, hot_words, app_handle).await;
    }

    let response = send_command_to_server(
        state,
        &ServerCommand::Transcribe {
            options: profile_r2t2_options(state),
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
        return Ok(server_response_to_transcription_result(state, response));
    }

    if response_indicates_inline_unsupported(&response) {
        log::info!("当前 FunASR 运行时不支持内存音频，回退到临时 WAV 文件");
        state.set_inline_audio_transport(Some(false));
        return transcribe_pcm16_via_path(state, samples, sample_rate, hot_words, app_handle).await;
    }

    state.set_inline_audio_transport(Some(true));
    Ok(server_response_to_transcription_result(state, response))
}

fn profile_hot_words(state: &AppState) -> Option<Vec<String>> {
    let words = state.with_profile(|p| p.get_hot_word_texts(100));
    (!words.is_empty()).then_some(words)
}

fn profile_r2t2_options(state: &AppState) -> Option<crate::state::user_profile::R2T2Config> {
    (paths::read_engine_config() == "confucius4-r2t2")
        .then(|| state.with_profile(|profile| profile.r2t2.clone()))
}

fn encode_pcm16_base64(samples: &[i16]) -> String {
    use base64::Engine;

    let byte_slice: &[u8] =
        unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 2) };
    base64::engine::general_purpose::STANDARD.encode(byte_slice)
}

fn encode_wav_bytes(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>, AppError> {
    super::audio_service::encode_wav(samples, sample_rate)
}

fn create_temp_audio_path() -> std::path::PathBuf {
    static TEMP_AUDIO_COUNTER: AtomicU64 = AtomicU64::new(1);
    let counter = TEMP_AUDIO_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "light_whisper_audio_{}_{}_{}.wav",
        std::process::id(),
        now_unix_ms(),
        counter,
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

fn server_response_to_transcription_result(
    state: &AppState,
    mut response: ServerResponse,
) -> TranscriptionResult {
    if response.success == Some(true) {
        if response
            .engine
            .as_deref()
            .is_some_and(|engine| engine.starts_with("qwen3-asr-"))
        {
            let text = response.text.as_deref().unwrap_or_default();
            let started = Instant::now();
            let correction = state.with_profile(|profile| {
                super::qwen_hotword_service::correct_qwen_profile_terms(text, profile)
            });
            if correction.replacements > 0 {
                log::info!(
                    "Qwen3-ASR 本地热词纠偏完成: {} 处, {} 微秒",
                    correction.replacements,
                    started.elapsed().as_micros()
                );
                response.text = Some(correction.text);
            }
        }

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
            options: profile_r2t2_options(state),
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
    response.map(|response| server_response_to_transcription_result(state, response))
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

async fn send_command_to_server<R: tauri::Runtime>(
    state: &AppState,
    command: &ServerCommand,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<ServerResponse, AppError> {
    send_command_to_server_with_timeout(
        state,
        command,
        app_handle,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
    )
    .await
}

pub(crate) async fn send_command_to_server_json<R: tauri::Runtime>(
    state: &AppState,
    command: &ServerCommand,
    app_handle: &tauri::AppHandle<R>,
) -> Result<serde_json::Value, AppError> {
    let response = send_command_to_server(state, command, Some(app_handle)).await?;
    if response.success != Some(true) {
        return Err(AppError::Asr(
            response
                .error
                .unwrap_or_else(|| "语音引擎未返回成功响应".into()),
        ));
    }
    Ok(serde_json::to_value(response)?)
}

struct CommandProcessGuard<'a, R: tauri::Runtime> {
    state: &'a AppState,
    app_handle: Option<&'a tauri::AppHandle<R>>,
    process: Option<FunasrProcess>,
}

impl<R: tauri::Runtime> Drop for CommandProcessGuard<'_, R> {
    fn drop(&mut self) {
        if self.process.is_none() {
            return;
        }
        log::warn!("FunASR 通信中断，释放进程并准备重启");
        self.state.set_funasr_ready(false);
        self.state.set_inline_audio_transport(None);
        if let Some(handle) = self.app_handle {
            let _ = handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "crashed",
                    "message": "语音识别引擎连接已断开，正在准备重启..."
                }),
            );
        }
    }
}

async fn send_command_to_server_with_timeout<R: tauri::Runtime>(
    state: &AppState,
    command: &ServerCommand,
    app_handle: Option<&tauri::AppHandle<R>>,
    timeout: Duration,
) -> Result<ServerResponse, AppError> {
    tokio::time::timeout(timeout, async {
        let mut guard = state.engine.funasr_process.lock().await;
        let process = guard
            .take()
            .ok_or_else(|| AppError::Asr("FunASR 进程未运行".to_string()))?;
        let mut owned = CommandProcessGuard {
            state,
            app_handle,
            process: Some(process),
        };
        let result = send_command_impl(owned.process.as_mut().unwrap(), command).await;
        if result.is_ok() {
            *guard = owned.process.take();
        }
        result
    })
    .await
    .map_err(|_| AppError::Asr("等待 FunASR 命令完成超时".to_string()))?
}

async fn send_command_impl(
    process: &mut FunasrProcess,
    command: &ServerCommand,
) -> Result<ServerResponse, AppError> {
    let request_id = NEXT_SERVER_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let mut command_value = serde_json::to_value(command)
        .map_err(|e| AppError::Asr(format!("序列化命令失败: {}", e)))?;
    if let Some(map) = command_value.as_object_mut() {
        map.insert(
            "request_id".to_string(),
            serde_json::Value::Number(request_id.into()),
        );
    }

    let command_json = serde_json::to_string(&command_value)
        .map_err(|e| AppError::Asr(format!("序列化命令失败: {}", e)))?;

    process
        .stdin
        .write_all(format!("{}\n", command_json).as_bytes())
        .await
        .map_err(|e| AppError::Asr(format!("写入命令到 FunASR 失败: {}", e)))?;

    process
        .stdin
        .flush()
        .await
        .map_err(|e| AppError::Asr(format!("刷新 stdin 缓冲区失败: {}", e)))?;

    read_json_response_matching(
        &mut process.stdout,
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
        "等待 FunASR 响应",
        |response: &ServerResponse| {
            server_response_matches_request(response.request_id, request_id)
        },
    )
    .await
}

fn server_response_matches_request(actual: Option<u64>, expected: u64) -> bool {
    actual == Some(expected)
}

#[cfg(test)]
mod request_identity_tests {
    use super::server_response_matches_request;

    #[test]
    fn untagged_late_reply_cannot_satisfy_a_new_request() {
        assert!(!server_response_matches_request(None, 2));
        assert!(!server_response_matches_request(Some(1), 2));
        assert!(server_response_matches_request(Some(2), 2));
    }
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

pub async fn push_gpu_idle_seconds(state: &AppState, seconds: u64) -> Result<(), AppError> {
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };
    if !has_process {
        return Ok(());
    }
    let response =
        send_command_to_server::<tauri::Wry>(state, &ServerCommand::SetGpuIdle { seconds }, None)
            .await?;
    if response.success == Some(false) {
        return Err(AppError::Asr(
            response
                .error
                .unwrap_or_else(|| "设置 GPU 空闲卸载失败".to_string()),
        ));
    }
    Ok(())
}

async fn sync_audio_processing_at_start(
    process: &mut FunasrProcess,
    enabled_at_spawn: bool,
) -> Result<(), AppError> {
    let enabled = paths::read_audio_processing_enabled();
    if enabled == enabled_at_spawn {
        return Ok(());
    }
    let response = tokio::time::timeout(
        Duration::from_secs(SERVER_RESPONSE_TIMEOUT_SECS),
        send_command_impl(process, &ServerCommand::SetAudioProcessing { enabled }),
    )
    .await
    .map_err(|_| AppError::Asr("同步音频处理设置超时".into()))??;
    if response.success == Some(true) {
        Ok(())
    } else {
        Err(AppError::Asr(
            response.error.unwrap_or_else(|| "音频处理设置失败".into()),
        ))
    }
}

pub async fn push_audio_processing_enabled(
    state: &AppState,
    enabled: bool,
) -> Result<(), AppError> {
    if state.engine.funasr_process.lock().await.is_none() {
        return Ok(());
    }
    let response = send_command_to_server::<tauri::Wry>(
        state,
        &ServerCommand::SetAudioProcessing { enabled },
        None,
    )
    .await?;
    if response.success != Some(true) {
        return Err(AppError::Asr(
            response.error.unwrap_or_else(|| "音频处理设置失败".into()),
        ));
    }
    Ok(())
}

pub async fn check_status(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<FunASRStatus, AppError> {
    let has_process = {
        let guard = state.engine.funasr_process.lock().await;
        guard.is_some()
    };

    if !has_process {
        let engine = paths::read_engine_config();
        if paths::is_online_engine(&engine) {
            let has_key = !state.read_online_asr_api_key().is_empty();
            let label = match engine.as_str() {
                "alibaba-asr" => "Alibaba DashScope",
                _ => "GLM-ASR",
            };
            return Ok(FunASRStatus {
                running: true,
                ready: has_key,
                model_loaded: true,
                device: Some("cloud".into()),
                gpu_name: None,
                gpu_memory_total: None,
                message: if has_key {
                    format!("{} 在线服务就绪", label)
                } else {
                    format!("请配置 {} API Key", label)
                },
                engine: Some(engine),
                models_present: Some(true),
                missing_models: Some(Vec::new()),
                model_state: Some(ModelState::Ready),
                corrupt_models: Some(Vec::new()),
            });
        }

        if state.engine.is_funasr_starting() {
            return Ok(status_with_defaults(
                true,
                false,
                false,
                "FunASR 服务器正在启动，模型加载中...".to_string(),
            ));
        }
        let model_check = inspect_model_files_for_engine(&engine);
        return Ok(FunASRStatus {
            message: match model_check.model_state {
                ModelState::Ready => "FunASR 服务器未运行".to_string(),
                ModelState::Missing => "模型文件未下载，请先下载模型".to_string(),
                ModelState::Corrupt => "模型文件校验失败，请修复模型".to_string(),
            },
            engine: Some(engine),
            models_present: Some(model_check.all_present),
            missing_models: Some(model_check.missing_models.clone()),
            model_state: Some(model_check.model_state),
            corrupt_models: Some(model_check.corrupt_models.clone()),
            ..status_with_defaults(false, false, false, String::new())
        });
    }

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
                model_state: Some(ModelState::Ready),
                corrupt_models: Some(Vec::new()),
            })
        }
        Err(e) => {
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

pub async fn stop_server(state: &AppState) -> Result<(), AppError> {
    {
        let _status_guard = state.engine.funasr_status_commit.lock();
        state
            .engine
            .funasr_generation
            .fetch_add(1, Ordering::SeqCst);
    }

    if let Some(starting) = state.engine.funasr_starting_process.lock().take() {
        if let Some(mut child) = starting.child.lock().take() {
            let _ = child.start_kill();
        }
    }

    let mut process = {
        let mut guard = state.engine.funasr_process.lock().await;
        guard.take()
    };

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
                log::warn!("FunASR 进程未响应退出命令，强制终止...");
                if let Err(e) = child_process.child.kill().await {
                    log::warn!("强制终止 FunASR 进程失败: {}", e);
                }
            }
        }
    }

    state.set_funasr_ready(false);
    state.set_inline_audio_transport(None);

    log::info!("FunASR 服务器已停止");
    Ok(())
}

fn get_hf_cache_root() -> PathBuf {
    paths::get_effective_models_dir()
}

#[cfg(test)]
fn is_hf_repo_ready(repo_id: &str, required_file: Option<&str>) -> bool {
    hf_repo_model_state(repo_id, required_file) == ModelState::Ready
}

fn hf_repo_model_state(repo_id: &str, required_file: Option<&str>) -> ModelState {
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

    match std::fs::metadata(&repo_dir) {
        Ok(metadata) if metadata.is_dir() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ModelState::Missing,
        _ => return ModelState::Corrupt,
    }

    let snapshots_dir = repo_dir.join("snapshots");
    let entries = match std::fs::read_dir(&snapshots_dir) {
        Ok(e) => e,
        Err(err) => {
            log::warn!(
                "无法读取 snapshots 目录: {} — {}",
                snapshots_dir.display(),
                err
            );
            return if snapshots_dir.exists() || err.kind() != std::io::ErrorKind::NotFound {
                ModelState::Corrupt
            } else {
                ModelState::Missing
            };
        }
    };

    let mut snapshots = match entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(snapshots) => snapshots,
        Err(_) => return ModelState::Corrupt,
    };
    snapshots.sort();
    if let Ok(revision) = std::fs::read_to_string(repo_dir.join("refs/main")) {
        if let Some(index) = snapshots.iter().position(|path| {
            path.file_name().and_then(|name| name.to_str()) == Some(revision.trim())
        }) {
            let selected = snapshots.remove(index);
            snapshots.insert(0, selected);
        }
    }
    let mut model_state = ModelState::Missing;
    for snapshot_path in snapshots {
        model_state = ModelState::Corrupt;
        if !snapshot_path.is_dir() {
            continue;
        }
        if required_file.is_some_and(|filename| {
            !std::fs::metadata(snapshot_path.join(filename))
                .is_ok_and(|meta| meta.is_file() && meta.len() >= 1_000_000)
        }) {
            continue;
        }
        log::info!("检查 snapshot: {}", snapshot_path.display());
        let manifest_present = !matches!(
            std::fs::symlink_metadata(snapshot_path.join(HF_COMPLETE_MANIFEST_NAME)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        );
        let ready = if manifest_present {
            snapshot_matches_completion_manifest_for_file(&snapshot_path, required_file)
        } else {
            snapshot_matches_legacy_weight_check(&snapshot_path)
        };
        if ready {
            log::info!("模型就绪: {} (在 {})", repo_id, snapshot_path.display());
            return ModelState::Ready;
        }
    }

    log::warn!("模型未就绪: {} — 未找到完整下载清单或文件校验失败", repo_id);
    model_state
}

#[cfg(test)]
fn snapshot_matches_completion_manifest(snapshot_path: &std::path::Path) -> bool {
    snapshot_matches_completion_manifest_for_file(snapshot_path, None)
}

fn snapshot_matches_completion_manifest_for_file(
    snapshot_path: &std::path::Path,
    required_file: Option<&str>,
) -> bool {
    const MIN_SIZE: u64 = 1_000_000;
    let manifest_path = snapshot_path.join(HF_COMPLETE_MANIFEST_NAME);
    let data = match std::fs::read_to_string(&manifest_path) {
        Ok(data) => data,
        Err(err) => {
            log::warn!("缺少模型完整性清单 {}: {}", manifest_path.display(), err);
            return false;
        }
    };
    let manifest: serde_json::Value = match serde_json::from_str(&data) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "模型完整性清单解析失败 {}: {}",
                manifest_path.display(),
                err
            );
            return false;
        }
    };
    let Some(files) = manifest.get("files").and_then(|value| value.as_array()) else {
        return false;
    };
    let mut has_weight = false;
    let mut has_required_file = required_file.is_none();
    for item in files {
        let Some(rel_path) = item.get("path").and_then(|value| value.as_str()) else {
            return false;
        };
        let Some(expected_size) = item.get("size").and_then(|value| value.as_u64()) else {
            return false;
        };
        let normalized = rel_path.replace('\\', "/");
        if normalized.is_empty()
            || !std::path::Path::new(&normalized)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return false;
        }
        let path = snapshot_path.join(normalized);
        let actual_size = match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => meta.len(),
            _ => return false,
        };
        if actual_size != expected_size {
            return false;
        }
        match item.get("sha256") {
            None | Some(serde_json::Value::Null) => {}
            Some(serde_json::Value::String(expected_hash)) => {
                if expected_hash.len() != 64
                    || !expected_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return false;
                }
                let Ok(mut file) = std::fs::File::open(&path) else {
                    return false;
                };
                let mut hash = Sha256::new();
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => hash.update(&buffer[..count]),
                        Err(_) => return false,
                    }
                }
                if !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected_hash) {
                    return false;
                }
            }
            _ => return false,
        }
        if required_file == Some(rel_path) {
            has_required_file = true;
        }
        if [".pt", ".bin", ".safetensors", ".onnx", ".gguf"]
            .iter()
            .any(|ext| rel_path.ends_with(ext))
            && actual_size >= MIN_SIZE
        {
            has_weight = true;
        }
    }
    has_weight && has_required_file
}

fn snapshot_matches_legacy_weight_check(snapshot_path: &std::path::Path) -> bool {
    const MIN_SIZE: u64 = 1_000_000;
    const WEIGHT_EXTS: &[&str] = &[".pt", ".bin", ".safetensors", ".onnx", ".gguf"];

    fn visit(dir: &std::path::Path, exts: &[&str], min_size: u64) -> Option<bool> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut has_weight = false;
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".incomplete") {
                return Some(false);
            }
            if path.is_dir() {
                match visit(&path, exts, min_size) {
                    Some(true) => has_weight = true,
                    Some(false) | None => return Some(false),
                }
            } else if exts.iter().any(|ext| name.ends_with(ext)) {
                match std::fs::metadata(&path) {
                    Ok(meta) if meta.len() >= min_size => has_weight = true,
                    Ok(_) => {}
                    Err(_) => return Some(false),
                }
            }
        }
        Some(has_weight)
    }

    let ready = visit(snapshot_path, WEIGHT_EXTS, MIN_SIZE).unwrap_or(false);
    if ready {
        log::warn!(
            "模型 snapshot 缺少完整性清单，按旧版权重文件检查兼容放行: {}",
            snapshot_path.display()
        );
    }
    ready
}

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
            model_state: ModelState::Ready,
            corrupt_models: Vec::new(),
        };
    }

    let cache_root = get_hf_cache_root();
    let cache_path = cache_root.to_string_lossy().to_string();

    let normalized_engine = if matches!(engine, "confucius4-r2t2" | "qwen3-asr-1.7b") {
        "confucius4-r2t2"
    } else {
        "qwen3-asr-0.6b"
    };
    let (repo_id, filename, description) = if normalized_engine == "confucius4-r2t2" {
        (R2T2_REPO_ID, R2T2_FILENAME, "Confucius4-R2T2 Q8 模型")
    } else {
        (
            QWEN3_ASR_06_REPO_ID,
            QWEN3_ASR_06_FILENAME,
            "Qwen3-ASR 0.6B Q8模型",
        )
    };
    let mut missing_models = Vec::new();
    let mut corrupt_models = Vec::new();
    let model_state = report_model_repo_file_state(
        repo_id,
        Some(filename),
        description,
        &mut missing_models,
        &mut corrupt_models,
    );
    let asr_present = model_state == ModelState::Ready;

    ModelCheckResult {
        all_present: asr_present,
        asr_model: asr_present,
        vad_model: true,
        punc_model: true,
        engine: normalized_engine.to_string(),
        cache_path,
        missing_models,
        model_state,
        corrupt_models,
    }
}

pub async fn check_model_files() -> Result<ModelCheckResult, AppError> {
    Ok(inspect_model_files_for_engine(&paths::read_engine_config()))
}

use tauri::Emitter;

#[cfg(test)]
mod tests {
    use super::{
        engine_install_fingerprint_matches, read_json_response, read_json_response_matching,
        server_response_to_transcription_result, EngineProgressGate, ServerCommand, ServerResponse,
        StartingFlagGuard, ENGINE_ARCHIVE_FINGERPRINT,
    };
    use crate::state::user_profile::{CorrectionPattern, CorrectionSource, HotWord, HotWordSource};
    use crate::state::AppState;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
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

    #[test]
    fn server_command_serializes_flat_optional_audio_fields() {
        let inline = serde_json::to_value(ServerCommand::Transcribe {
            options: Some(crate::state::user_profile::R2T2Config {
                context: "domain context".into(),
                language: Some("de".into()),
            }),
            audio_path: None,
            audio_base64: Some("AQI=".to_string()),
            audio_format: Some("pcm_s16le".to_string()),
            sample_rate: Some(16_000),
            hot_words: Some(vec!["light-whisper".to_string()]),
        })
        .unwrap();

        assert_eq!(inline["action"], serde_json::json!("transcribe"));
        assert_eq!(inline["options"]["context"], "domain context");
        assert_eq!(inline["options"]["language"], "de");
        assert_eq!(inline["audio_base64"], serde_json::json!("AQI="));
        assert_eq!(inline["audio_format"], serde_json::json!("pcm_s16le"));
        assert_eq!(inline["sample_rate"], serde_json::json!(16_000));
        assert_eq!(inline["hot_words"], serde_json::json!(["light-whisper"]));
        assert!(inline.get("audio_path").is_none());

        let idle = serde_json::to_value(ServerCommand::SetGpuIdle { seconds: 0 }).unwrap();
        assert_eq!(idle["action"], serde_json::json!("set_gpu_idle"));
        assert_eq!(idle["seconds"], serde_json::json!(0));
        let idle = serde_json::to_value(ServerCommand::SetGpuIdle { seconds: 180 }).unwrap();
        assert_eq!(idle["seconds"], serde_json::json!(180));

        let path = serde_json::to_value(ServerCommand::Transcribe {
            options: None,
            audio_path: Some("C:/tmp/clip.wav".to_string()),
            audio_base64: None,
            audio_format: None,
            sample_rate: None,
            hot_words: None,
        })
        .unwrap();

        assert_eq!(path["action"], serde_json::json!("transcribe"));
        assert!(path.get("options").is_none());
        assert_eq!(path["audio_path"], serde_json::json!("C:/tmp/clip.wav"));
        assert!(path.get("audio_base64").is_none());
        assert!(path.get("audio_format").is_none());
        assert!(path.get("sample_rate").is_none());
        assert!(path.get("hot_words").is_none());
    }

    #[test]
    fn engine_install_fingerprint_reuses_legacy_versioned_archive_marker() {
        let legacy = format!("1.3.13+{}", ENGINE_ARCHIVE_FINGERPRINT);

        assert!(engine_install_fingerprint_matches(
            &legacy,
            ENGINE_ARCHIVE_FINGERPRINT
        ));
        assert!(engine_install_fingerprint_matches(
            ENGINE_ARCHIVE_FINGERPRINT,
            ENGINE_ARCHIVE_FINGERPRINT
        ));
    }

    #[test]
    fn engine_install_fingerprint_rejects_a_different_archive() {
        assert!(!engine_install_fingerprint_matches(
            "1.3.13+0000000000000001-0000000000000002",
            ENGINE_ARCHIVE_FINGERPRINT
        ));
    }

    #[test]
    fn qwen_response_uses_profile_hotwords_without_touching_other_engines() {
        let state = AppState::new();
        state.update_profile_mut(|profile| {
            profile.hot_words.push(HotWord {
                text: "github".to_string(),
                weight: 3,
                source: HotWordSource::User,
                use_count: 10,
                last_used: 0,
            });
            profile.hot_words.push(HotWord {
                text: "openclaw".to_string(),
                weight: 3,
                source: HotWordSource::User,
                use_count: 10,
                last_used: 0,
            });
            profile.correction_patterns.push(CorrectionPattern {
                original: "open cloud".to_string(),
                corrected: "openclaw".to_string(),
                count: 33,
                last_seen: 0,
                source: CorrectionSource::User,
            });
        });

        let qwen_response: ServerResponse = serde_json::from_value(serde_json::json!({
            "success": true,
            "text": "请用 get hub 打开 open cloud。",
            "engine": "qwen3-asr-0.6b"
        }))
        .unwrap();
        let qwen_result = server_response_to_transcription_result(&state, qwen_response);
        assert_eq!(qwen_result.text, "请用 github 打开 openclaw。");
    }

    #[test]
    fn stale_starting_guard_does_not_clear_a_new_start_owner() {
        let engine = crate::state::EngineState::default();
        assert!(engine.try_begin_funasr_start(1));
        let stale_guard = StartingFlagGuard {
            engine: &engine,
            owner: 1,
            child: None,
        };

        engine.block_funasr_starting();
        engine.unblock_funasr_starting();
        assert!(engine.try_begin_funasr_start(2));

        drop(stale_guard);

        assert!(
            engine.is_funasr_starting(),
            "旧启动 guard 不得清除新启动持有的标志"
        );
    }

    #[test]
    fn stale_engine_extraction_progress_is_suppressed_after_stop() {
        let generation = Arc::new(AtomicU64::new(4));
        let gate = EngineProgressGate {
            generation: generation.clone(),
            status_commit: Arc::new(parking_lot::Mutex::new(())),
            expected_generation: 4,
        };
        assert!(gate.is_current());

        generation.fetch_add(1, Ordering::SeqCst);

        assert!(
            !gate.is_current(),
            "old extraction progress must not emit after an engine switch changes generation"
        );
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

    #[tokio::test]
    async fn read_json_response_reports_stdout_eof_without_a_response() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        writer.shutdown().await.unwrap();
        let mut reader = BufReader::new(reader);

        let error =
            read_json_response::<ServerResponse, _>(&mut reader, Duration::from_secs(1), "test")
                .await
                .expect_err("closed stdout must fail instead of waiting for a response");

        assert_eq!(error.to_string(), "ASR错误: test失败：stdout 已关闭");
    }

    #[tokio::test]
    async fn read_json_response_times_out_when_stdout_stays_open_without_data() {
        let (_writer, reader) = tokio::io::duplex(1024);
        let mut reader = BufReader::new(reader);

        let error = tokio::time::timeout(
            Duration::from_secs(1),
            read_json_response::<ServerResponse, _>(&mut reader, Duration::from_millis(20), "test"),
        )
        .await
        .expect("response timeout path must not hang the test")
        .expect_err("silent stdout must respect the response timeout");

        assert_eq!(error.to_string(), "ASR错误: test超时");
    }

    #[tokio::test]
    async fn read_json_response_ignores_stale_response_before_matching_request() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        writer
            .write_all(
                br#"{"request_id":1,"success":true,"message":"stale"}
{"request_id":2,"success":true,"message":"fresh"}
"#,
            )
            .await
            .unwrap();
        writer.shutdown().await.unwrap();
        let mut reader = BufReader::new(reader);
        let response = read_json_response_matching(
            &mut reader,
            Duration::from_secs(1),
            "test",
            |response: &ServerResponse| response.request_id == Some(2),
        )
        .await
        .unwrap();

        assert_eq!(
            response.message.as_deref(),
            Some("fresh"),
            "ASR IPC must correlate responses by request_id and skip stale responses left behind by a cancelled request"
        );
    }

    #[tokio::test]
    async fn read_json_response_accepts_legacy_response_without_request_id() {
        let (mut writer, reader) = tokio::io::duplex(1024);
        writer
            .write_all(
                br#"{"success":true,"message":"legacy"}
{"request_id":7,"success":true,"message":"matched"}
"#,
            )
            .await
            .unwrap();
        writer.shutdown().await.unwrap();
        let mut reader = BufReader::new(reader);
        let response = read_json_response_matching(
            &mut reader,
            Duration::from_secs(1),
            "test",
            |response: &ServerResponse| match response.request_id {
                Some(id) => id == 7,
                None => true,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.message.as_deref(), Some("legacy"));
    }

    #[test]
    fn engine_extraction_preserves_existing_engine_until_archive_succeeds() {
        let root = std::env::temp_dir().join(format!(
            "light_whisper_engine_replace_test_{}_{}",
            std::process::id(),
            super::now_unix_ms()
        ));
        let engine_dir = root.join("engine");
        let missing_staging_dir = root.join("missing-staging");
        let backup_dir = root.join("engine.backup");
        std::fs::create_dir_all(&engine_dir).unwrap();
        std::fs::write(engine_dir.join("engine.exe"), b"old-engine").unwrap();

        let result = super::replace_engine_dir(&engine_dir, &missing_staging_dir, &backup_dir);

        assert!(result.is_err());
        assert_eq!(
            std::fs::read(engine_dir.join("engine.exe")).unwrap(),
            b"old-engine"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod model_readiness_tests {
    use sha2::{Digest, Sha256};

    fn run_in_isolated_cache(test_name: &str) -> bool {
        const CHILD_ENV: &str = "LIGHT_WHISPER_MODEL_READINESS_TEST_CHILD";
        if std::env::var(CHILD_ENV).as_deref() == Ok(test_name) {
            let cache = crate::utils::paths::get_data_dir().join("models");
            crate::utils::paths::write_models_dir(Some(cache.to_str().unwrap())).unwrap();
            return false;
        }
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "light-whisper-model-readiness-{}-{nonce}",
            std::process::id()
        ));
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test_name, "--nocapture"])
            .env("LIGHT_WHISPER_DATA_DIR", &directory)
            .env(CHILD_ENV, test_name)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&directory);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    fn snapshot(repo_id: &str) -> std::path::PathBuf {
        let path = super::get_hf_cache_root()
            .join(format!("models--{}", repo_id.replace('/', "--")))
            .join("snapshots/test-revision");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("model.gguf"), vec![0_u8; 1_000_001]).unwrap();
        path
    }

    #[test]
    fn model_repo_invalid_manifest_cannot_fall_back_to_large_legacy_weight() {
        if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::model_repo_invalid_manifest_cannot_fall_back_to_large_legacy_weight",
        ) {
            return;
        }
        let digest = format!("{:x}", Sha256::digest(vec![0_u8; 1_000_001]));
        let valid_weight = serde_json::json!({
            "path": "model.gguf", "size": 1_000_001, "sha256": digest,
        });
        let cases = [
            ("malformed", "{".to_string(), false),
            (
                "size-mismatch",
                serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_002}]}).to_string(),
                false,
            ),
            (
                "hash-mismatch",
                serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_001, "sha256": "0".repeat(64)}]}).to_string(),
                false,
            ),
            (
                "missing-listed-file",
                serde_json::json!({"files": [valid_weight, {"path": "config.json", "size": 2}]}).to_string(),
                false,
            ),
            (
                "unlisted-required-file",
                serde_json::json!({"files": [{"path": "other.gguf", "size": 1_000_001, "sha256": digest}]}).to_string(),
                true,
            ),
        ];
        let mut actual = Vec::new();
        let mut expected = Vec::new();
        for (case, manifest, manifest_valid) in cases {
            let repo_id = format!("test/{case}");
            let path = snapshot(&repo_id);
            if case == "unlisted-required-file" {
                std::fs::copy(path.join("model.gguf"), path.join("other.gguf")).unwrap();
            }
            std::fs::write(path.join(super::HF_COMPLETE_MANIFEST_NAME), manifest).unwrap();
            actual.push((
                case,
                super::snapshot_matches_completion_manifest(&path),
                super::is_hf_repo_ready(&repo_id, Some("model.gguf")),
            ));
            expected.push((case, manifest_valid, false));
        }
        assert_eq!(
            actual, expected,
            "a present manifest is authoritative, including SHA256 and the selected file"
        );
    }

    #[test]
    fn model_repo_preserves_valid_manifest_and_no_manifest_legacy_cache() {
        if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::model_repo_preserves_valid_manifest_and_no_manifest_legacy_cache",
        ) {
            return;
        }
        let legacy = snapshot("test/legacy");
        assert!(!super::snapshot_matches_completion_manifest(&legacy));
        assert!(super::is_hf_repo_ready("test/legacy", Some("model.gguf")));

        let valid = snapshot("test/valid");
        let digest = format!(
            "{:x}",
            Sha256::digest(std::fs::read(valid.join("model.gguf")).unwrap())
        );
        std::fs::write(
            valid.join(super::HF_COMPLETE_MANIFEST_NAME),
            serde_json::json!({"files": [{"path": "model.gguf", "size": 1_000_001, "sha256": digest}]}).to_string(),
        )
        .unwrap();
        assert!(super::snapshot_matches_completion_manifest(&valid));
        assert!(super::is_hf_repo_ready("test/valid", Some("model.gguf")));
    }

    #[test]
    fn selected_model_status_distinguishes_missing_ready_and_corrupt() {
        if run_in_isolated_cache(
            "services::funasr_service::model_readiness_tests::selected_model_status_distinguishes_missing_ready_and_corrupt",
        ) {
            return;
        }
        let inspect = || {
            serde_json::to_value(super::inspect_model_files_for_engine("qwen3-asr-0.6b")).unwrap()
        };
        let missing = inspect();
        let selected = snapshot(super::QWEN3_ASR_06_REPO_ID);
        let selected_weight = selected.join(super::QWEN3_ASR_06_FILENAME);
        std::fs::create_dir_all(selected_weight.parent().unwrap()).unwrap();
        std::fs::rename(selected.join("model.gguf"), selected_weight).unwrap();
        let ready = inspect();
        std::fs::write(selected.join(super::HF_COMPLETE_MANIFEST_NAME), "{").unwrap();
        let corrupt = inspect();

        assert_eq!(
            [
                missing["model_state"].as_str(),
                ready["model_state"].as_str(),
                corrupt["model_state"].as_str(),
            ],
            [Some("missing"), Some("ready"), Some("corrupt")],
            "serialized model state must preserve the reason recognition cannot start"
        );
        assert_eq!(missing["missing_models"].as_array().unwrap().len(), 1);
        assert!(missing["corrupt_models"].as_array().unwrap().is_empty());
        assert_eq!(ready["all_present"], true);
        assert!(ready["missing_models"].as_array().unwrap().is_empty());
        assert!(ready["corrupt_models"].as_array().unwrap().is_empty());
        assert_eq!(corrupt["all_present"], false);
        assert!(corrupt["missing_models"].as_array().unwrap().is_empty());
        let corrupt_models = corrupt["corrupt_models"].as_array().unwrap();
        assert_eq!(corrupt_models.len(), 1);
        assert!(corrupt_models[0]
            .as_str()
            .is_some_and(|name| !name.is_empty()));
    }
}
