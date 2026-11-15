use std::future::{poll_fn, Future};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::native_capture::{run_capture, NativeCaptureControl, NativeCaptureSink};
use super::native_recording::stream_config;
use crate::services::funasr_service::native_stream::NativeStreamUpdate;
use crate::services::funasr_service::TranscriptionResult;
use crate::state::user_profile::R2T2Config;
use crate::utils::AppError;

#[test]
fn only_qwen_06_and_r2t2_use_the_shared_native_route() {
    for engine in ["qwen3-asr-0.6b", "confucius4-r2t2"] {
        assert!(super::uses_native_stream(engine), "engine: {engine}");
    }
    for engine in ["qwen3-asr-1.7b", "glm-asr", "alibaba-asr", "unknown", ""] {
        assert!(!super::uses_native_stream(engine), "engine: {engine}");
    }
}

#[test]
fn qwen_stream_does_not_inherit_r2t2_context_or_forced_language() {
    let r2_config = R2T2Config {
        context: "R2T2-only recognition context".into(),
        language: Some("Chinese".into()),
    };

    let actual = stream_config("qwen3-asr-0.6b", &r2_config);
    let default = R2T2Config::default();
    assert_eq!(actual.context, default.context);
    assert_eq!(actual.language, default.language);
}

#[test]
fn r2t2_stream_keeps_its_saved_context_and_language() {
    let saved = R2T2Config {
        context: "R2T2 recognition context".into(),
        language: Some("English".into()),
    };

    let actual = stream_config("confucius4-r2t2", &saved);
    assert_eq!(actual.context, saved.context);
    assert_eq!(actual.language, saved.language);
}

#[derive(Default)]
struct CaptureLog {
    feeds: Vec<Vec<i16>>,
    finishes: usize,
    cancels: usize,
}

struct QwenSink(Arc<Mutex<CaptureLog>>);

impl NativeCaptureSink for QwenSink {
    fn chunk_samples(&self) -> usize {
        640
    }

    async fn feed(&mut self, samples: &[i16]) -> Result<NativeStreamUpdate, AppError> {
        let mut log = self.0.lock();
        log.feeds.push(samples.to_vec());
        Ok(NativeStreamUpdate {
            text: String::new(),
            tentative_text: String::new(),
            language: None,
            sample_count: log.feeds.iter().map(Vec::len).sum(),
            is_final: false,
        })
    }

    async fn finish(&mut self) -> Result<TranscriptionResult, AppError> {
        self.0.lock().finishes += 1;
        Ok(TranscriptionResult {
            text: "finished".into(),
            duration: None,
            success: true,
            error: None,
            language: None,
        })
    }

    async fn cancel(&mut self) -> Result<(), AppError> {
        self.0.lock().cancels += 1;
        Ok(())
    }
}

async fn poll_capture_once<F: Future>(mut capture: Pin<&mut F>) {
    poll_fn(|cx| {
        assert!(capture.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
}

#[tokio::test(start_paused = true)]
async fn qwen_feeds_first_40ms_chunk_then_finishes_with_every_sample_once() {
    let input: Vec<i16> = (0..1453).collect();
    let captured = Arc::new(Mutex::new(input[..639].to_vec()));
    let log = Arc::new(Mutex::new(CaptureLog::default()));
    let updates = Arc::new(Mutex::new(Vec::new()));
    let collected = updates.clone();
    let (control_tx, control_rx) = oneshot::channel();
    let capture = run_capture(
        QwenSink(log.clone()),
        captured.clone(),
        16_000,
        control_rx,
        move |update| collected.lock().push(update.sample_count),
    );
    tokio::pin!(capture);

    poll_capture_once(capture.as_mut()).await;
    assert!(log.lock().feeds.is_empty());

    captured.lock().push(input[639]);
    tokio::time::advance(Duration::from_millis(40)).await;
    poll_capture_once(capture.as_mut()).await;
    assert_eq!(log.lock().feeds, vec![input[..640].to_vec()]);

    captured.lock().extend_from_slice(&input[640..]);
    assert!(control_tx.send(NativeCaptureControl::Finish).is_ok());
    assert!(capture.await.unwrap().unwrap().success);

    let log = log.lock();
    assert_eq!(
        log.feeds.iter().map(Vec::len).collect::<Vec<_>>(),
        [640, 640, 173]
    );
    assert_eq!(log.feeds.concat(), input);
    assert_eq!(log.finishes, 1);
    assert_eq!(log.cancels, 0);
    assert_eq!(*updates.lock(), [640, 1280, 1453]);
}

#[tokio::test]
async fn qwen_finish_before_first_poll_uses_40ms_chunks_and_keeps_short_tail() {
    let input: Vec<i16> = (0..1281).collect();
    let log = Arc::new(Mutex::new(CaptureLog::default()));
    let (control_tx, control_rx) = oneshot::channel();
    assert!(control_tx.send(NativeCaptureControl::Finish).is_ok());

    let result = run_capture(
        QwenSink(log.clone()),
        Arc::new(Mutex::new(input.clone())),
        16_000,
        control_rx,
        |_| {},
    )
    .await
    .unwrap()
    .unwrap();

    assert!(result.success);
    let log = log.lock();
    assert_eq!(
        log.feeds.iter().map(Vec::len).collect::<Vec<_>>(),
        [640, 640, 1]
    );
    assert_eq!(log.feeds.concat(), input);
    assert_eq!(log.finishes, 1);
    assert_eq!(log.cancels, 0);
}
