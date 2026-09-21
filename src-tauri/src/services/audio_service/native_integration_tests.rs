use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdout, Command};
use tokio::sync::oneshot;

use super::native_capture::{run_capture, NativeCaptureControl, NativeCaptureSink};
use crate::services::funasr_service::native_stream::{NativeStreamClient, NativeStreamUpdate};
use crate::services::funasr_service::{stop_server, TranscriptionResult};
use crate::state::{AppState, FunasrProcess};
use crate::utils::AppError;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(120);

fn test_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
    Box::new(std::io::Error::other(message.into()))
}

struct IntegrationEnv {
    python: PathBuf,
    resources: PathBuf,
    cache: PathBuf,
    audio: PathBuf,
    data: PathBuf,
}

impl IntegrationEnv {
    fn from_process() -> TestResult<Self> {
        let python = required_path("LIGHT_WHISPER_R2T2_TEST_PYTHON")?;
        let resources = required_path("LIGHT_WHISPER_R2T2_TEST_RESOURCES")?;
        let cache = required_path("LIGHT_WHISPER_R2T2_TEST_CACHE")?;
        let audio = required_path("LIGHT_WHISPER_R2T2_TEST_AUDIO")?;
        let data = required_path("LIGHT_WHISPER_R2T2_TEST_DATA")?;

        require_file("LIGHT_WHISPER_R2T2_TEST_PYTHON", &python)?;
        require_dir("LIGHT_WHISPER_R2T2_TEST_RESOURCES", &resources)?;
        require_dir("LIGHT_WHISPER_R2T2_TEST_CACHE", &cache)?;
        require_file("LIGHT_WHISPER_R2T2_TEST_AUDIO", &audio)?;
        require_dir("LIGHT_WHISPER_R2T2_TEST_DATA", &data)?;
        require_file("engine.py", &resources.join("engine.py"))?;

        Ok(Self {
            python,
            resources,
            cache,
            audio,
            data,
        })
    }
}

fn required_path(name: &str) -> TestResult<PathBuf> {
    std::env::var_os(name).map(PathBuf::from).ok_or_else(|| {
        test_error(format!(
            "{name} is required for the ignored integration test"
        ))
    })
}

fn require_file(name: &str, path: &Path) -> TestResult<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(test_error(format!(
            "{name} must point to a file: {}",
            path.display()
        )))
    }
}

fn require_dir(name: &str, path: &Path) -> TestResult<()> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(test_error(format!(
            "{name} must point to a directory: {}",
            path.display()
        )))
    }
}

async fn read_json_line(
    reader: &mut BufReader<ChildStdout>,
    timeout: Duration,
    label: &str,
) -> TestResult<serde_json::Value> {
    let mut line = String::new();
    let bytes = tokio::time::timeout(timeout, reader.read_line(&mut line))
        .await
        .map_err(|_| test_error(format!("{label} response timed out")))??;
    if bytes == 0 {
        return Err(test_error(format!(
            "{label} stdout closed before JSON response"
        )));
    }
    serde_json::from_str(line.trim())
        .map_err(|error| test_error(format!("{label} returned invalid JSON: {error}")))
}

struct RunningServer {
    state: Arc<AppState>,
    stopped: bool,
}

impl RunningServer {
    async fn start(config: &IntegrationEnv) -> TestResult<Self> {
        let mut command = Command::new(&config.python);
        command
            .arg("-X")
            .arg("utf8")
            .arg("-u")
            .arg(config.resources.join("engine.py"))
            .arg("serve")
            .arg("--engine")
            .arg("confucius4-r2t2")
            .current_dir(&config.resources)
            .env("HF_HUB_CACHE", &config.cache)
            .env("HF_HUB_OFFLINE", "1")
            .env("LIGHT_WHISPER_DATA_DIR", &config.data)
            .env("LIGHT_WHISPER_ASR_ENGINE", "confucius4-r2t2")
            .env("PYTHONUTF8", "1")
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        command.creation_flags(0x08000000);
        command.kill_on_drop(true);

        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| test_error("R2T2 test server did not expose stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| test_error("R2T2 test server did not expose stdout"))?;
        let mut stdout = BufReader::new(stdout);
        let ready = read_json_line(&mut stdout, STARTUP_TIMEOUT, "R2T2 startup").await?;
        if ready.get("success").and_then(serde_json::Value::as_bool) != Some(true)
            || ready
                .get("model_loaded")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            || ready.get("backend").and_then(serde_json::Value::as_str) != Some("cuda")
        {
            return Err(test_error(format!(
                "R2T2 startup was not CUDA-ready: {ready}"
            )));
        }

        let state = Arc::new(AppState::default());
        *state.engine.funasr_process.lock().await = Some(FunasrProcess {
            child,
            stdin,
            stdout,
        });
        state.set_funasr_ready(true);
        Ok(Self {
            state,
            stopped: false,
        })
    }

    async fn status(&self) -> TestResult<serde_json::Value> {
        let mut guard = self.state.engine.funasr_process.lock().await;
        let process = guard
            .as_mut()
            .ok_or_else(|| test_error("R2T2 test server process is missing"))?;
        process
            .stdin
            .write_all(
                br#"{"action":"status","request_id":0}
"#,
            )
            .await?;
        process.stdin.flush().await?;
        read_json_line(&mut process.stdout, IPC_TIMEOUT, "R2T2 status").await
    }

    async fn stop(&mut self) -> TestResult<()> {
        if !self.stopped {
            stop_server(self.state.as_ref()).await?;
            self.stopped = true;
        }
        Ok(())
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        if let Ok(mut guard) = self.state.engine.funasr_process.try_lock() {
            if let Some(mut process) = guard.take() {
                let _ = process.child.start_kill();
            }
        }
    }
}

#[derive(Debug, Default)]
struct CaptureObservation {
    feed_sample_counts: Vec<usize>,
    final_sample_count: Option<usize>,
    cancel_count: usize,
}

#[derive(Debug, Clone)]
struct UpdateObservation {
    text: String,
    tentative_text: String,
    sample_count: usize,
    is_final: bool,
}

fn observe_update(update: &NativeStreamUpdate) -> UpdateObservation {
    UpdateObservation {
        text: update.text.clone(),
        tentative_text: update.tentative_text.clone(),
        sample_count: update.sample_count,
        is_final: update.is_final,
    }
}

struct ClientSink {
    state: Arc<AppState>,
    client: NativeStreamClient,
    observation: Arc<parking_lot::Mutex<CaptureObservation>>,
}

impl NativeCaptureSink for ClientSink {
    async fn feed(&mut self, samples: &[i16]) -> Result<NativeStreamUpdate, AppError> {
        let update = self.client.feed(self.state.as_ref(), None, samples).await?;
        self.observation
            .lock()
            .feed_sample_counts
            .push(update.sample_count);
        Ok(update)
    }

    async fn finish(&mut self) -> Result<TranscriptionResult, AppError> {
        let result = self.client.finish(self.state.as_ref(), None).await?;
        self.observation.lock().final_sample_count = Some(self.client.sample_count);
        Ok(result)
    }

    async fn cancel(&mut self) -> Result<(), AppError> {
        self.client.cancel(self.state.as_ref(), None).await?;
        self.observation.lock().cancel_count += 1;
        Ok(())
    }
}

async fn run_wav_capture(
    state: Arc<AppState>,
    samples: Vec<i16>,
    session_id: u64,
) -> TestResult<(
    TranscriptionResult,
    Arc<parking_lot::Mutex<CaptureObservation>>,
    Arc<parking_lot::Mutex<Vec<UpdateObservation>>>,
)> {
    let client = NativeStreamClient::start(
        state.as_ref(),
        None,
        session_id,
        "顾客 酒水".to_string(),
        Some("Chinese".to_string()),
    )
    .await?;
    let observation = Arc::new(parking_lot::Mutex::new(CaptureObservation::default()));
    let sink = ClientSink {
        state: state.clone(),
        client,
        observation: observation.clone(),
    };
    let emitted = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let emitted_for_callback = emitted.clone();
    let on_update = move |update: NativeStreamUpdate| {
        emitted_for_callback.lock().push(observe_update(&update));
    };
    let captured = Arc::new(parking_lot::Mutex::new(samples));
    let (control_tx, control_rx) = oneshot::channel();
    control_tx
        .send(NativeCaptureControl::Finish)
        .map_err(|_| test_error("R2T2 finish control receiver disappeared"))?;

    let result = tokio::time::timeout(
        CAPTURE_TIMEOUT,
        run_capture(sink, captured, 16_000, control_rx, on_update),
    )
    .await
    .map_err(|_| test_error("R2T2 native capture timed out"))??
    .ok_or_else(|| test_error("R2T2 native capture was cancelled"))?;

    Ok((result, observation, emitted))
}

fn read_pcm16(path: &Path) -> TestResult<Vec<i16>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 || spec.bits_per_sample != 16 {
        return Err(test_error(format!(
            "integration audio must be mono 16 kHz PCM16, got channels={} rate={} bits={}",
            spec.channels, spec.sample_rate, spec.bits_per_sample
        )));
    }
    Ok(reader.samples::<i16>().collect::<Result<Vec<_>, _>>()?)
}

fn assert_ready_status(status: &serde_json::Value) {
    assert_eq!(
        status.get("success").and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status
            .get("initialized")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status.get("backend").and_then(serde_json::Value::as_str),
        Some("cuda")
    );
    assert_eq!(
        status
            .get("model_loaded")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status
            .get("native_streaming")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        status
            .get("inline_audio")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
#[ignore = "requires explicit CUDA R2T2 integration environment"]
async fn real_native_capture_streams_public_chinese_wav() -> TestResult {
    let config = IntegrationEnv::from_process()?;
    let mut server = RunningServer::start(&config).await?;
    assert_ready_status(&server.status().await?);

    let samples = read_pcm16(&config.audio)?;
    let sample_count = samples.len();
    let (result, observation, emitted) =
        run_wav_capture(server.state.clone(), samples, 70_001).await?;

    assert!(result.success);
    assert!(
        result.text.contains("顾客自己带酒水"),
        "transcript: {}",
        result.text
    );
    {
        let emitted = emitted.lock();
        let last_update = emitted
            .last()
            .ok_or_else(|| test_error("native capture emitted no successful feed update"))?;
        assert!(result.text.starts_with(&last_update.text));
        assert_eq!(last_update.sample_count, sample_count);
        assert!(!last_update.is_final);
        let first_visible = emitted
            .iter()
            .find(|update| !update.text.is_empty() || !update.tentative_text.is_empty())
            .expect("capture must expose a model hypothesis");
        let first_committed = emitted
            .iter()
            .find(|update| !update.text.is_empty())
            .expect("fixture must eventually commit text");
        assert!(
            first_visible.sample_count < first_committed.sample_count,
            "tentative captions should reach the capture callback before stable text"
        );
    }
    {
        let observation = observation.lock();
        assert_eq!(observation.final_sample_count, Some(sample_count));
        assert_eq!(
            observation.feed_sample_counts.last().copied(),
            Some(sample_count)
        );
        assert!(observation
            .feed_sample_counts
            .windows(2)
            .all(|window| window[0] < window[1]));
        assert_eq!(observation.cancel_count, 0);
    }

    server.stop().await
}

#[tokio::test]
#[ignore = "requires explicit CUDA R2T2 integration environment"]
async fn real_native_stream_cancel_restart_and_empty_finish_leave_server_usable() -> TestResult {
    let config = IntegrationEnv::from_process()?;
    let mut server = RunningServer::start(&config).await?;
    assert_ready_status(&server.status().await?);
    let state = server.state.clone();
    let samples = read_pcm16(&config.audio)?;
    let prefix_len = 48_000.min(samples.len());
    assert!(prefix_len > 0, "integration WAV must contain audio samples");

    let mut cancelled = NativeStreamClient::start(
        state.as_ref(),
        None,
        70_101,
        String::new(),
        Some("Chinese".to_string()),
    )
    .await?;
    let update = cancelled
        .feed(state.as_ref(), None, &samples[..prefix_len])
        .await?;
    assert_eq!(update.sample_count, prefix_len);
    cancelled.cancel(state.as_ref(), None).await?;
    assert!(cancelled
        .feed(state.as_ref(), None, &[0_i16])
        .await
        .is_err());
    assert!(state.engine.funasr_process.lock().await.is_some());

    let mut restarted = NativeStreamClient::start(
        state.as_ref(),
        None,
        70_102,
        String::new(),
        Some("Chinese".to_string()),
    )
    .await?;
    let empty = restarted.finish(state.as_ref(), None).await?;
    assert!(empty.success);
    assert_eq!(empty.text, "");
    assert_eq!(empty.duration, Some(0.0));
    assert_eq!(restarted.sample_count, 0);
    assert!(restarted.finish(state.as_ref(), None).await.is_err());
    assert!(state.engine.funasr_process.lock().await.is_some());

    server.stop().await
}
