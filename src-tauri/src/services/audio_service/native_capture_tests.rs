use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{oneshot, Notify};

use super::native_capture::{run_capture, NativeCaptureControl, NativeCaptureSink};
use super::resample::ChunkedResampler;
use crate::services::funasr_service::native_stream::NativeStreamUpdate;
use crate::services::funasr_service::TranscriptionResult;
use crate::utils::AppError;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Feed(Vec<i16>),
    Finish,
    Cancel,
}

#[derive(Debug, Default)]
struct FakeLog {
    feed_inputs: Vec<Vec<i16>>,
    events: Vec<Event>,
    finish_count: usize,
    cancel_count: usize,
}

#[derive(Clone)]
struct FakeState {
    log: Arc<Mutex<FakeLog>>,
    feed_started: Arc<Notify>,
    release_feed: Arc<Notify>,
}

impl FakeState {
    fn new() -> Self {
        Self {
            log: Arc::new(Mutex::new(FakeLog::default())),
            feed_started: Arc::new(Notify::new()),
            release_feed: Arc::new(Notify::new()),
        }
    }
}

async fn wait_for_feed_start(state: &FakeState) {
    tokio::time::timeout(Duration::from_secs(5), state.feed_started.notified())
        .await
        .expect("capture task did not reach the feed barrier within 5 seconds");
}

struct FakeSink {
    state: FakeState,
    block_first_feed: bool,
    fail_first_feed: bool,
    finish_result: TranscriptionResult,
}

impl FakeSink {
    fn new(state: FakeState, block_first_feed: bool, fail_first_feed: bool) -> Self {
        Self {
            state,
            block_first_feed,
            fail_first_feed,
            finish_result: TranscriptionResult {
                text: "finished".to_string(),
                duration: Some(0.321),
                success: true,
                error: None,
                language: Some("en".to_string()),
            },
        }
    }
}

impl NativeCaptureSink for FakeSink {
    fn feed(
        &mut self,
        samples: &[i16],
    ) -> impl Future<Output = Result<NativeStreamUpdate, AppError>> + Send {
        let state = self.state.clone();
        let input = samples.to_vec();
        let block = self.block_first_feed;
        let fail = self.fail_first_feed;
        self.block_first_feed = false;
        self.fail_first_feed = false;

        async move {
            {
                let mut log = state.log.lock();
                log.events.push(Event::Feed(input.clone()));
                log.feed_inputs.push(input);
            }
            state.feed_started.notify_one();

            if block {
                state.release_feed.notified().await;
            }
            if fail {
                return Err(AppError::Asr("feed sentinel".to_string()));
            }

            let sample_count = state
                .log
                .lock()
                .feed_inputs
                .iter()
                .map(|chunk| chunk.len())
                .sum();
            Ok(NativeStreamUpdate {
                tentative_text: String::new(),
                text: format!("fed-{sample_count}"),
                language: None,
                sample_count,
                is_final: false,
            })
        }
    }

    fn finish(&mut self) -> impl Future<Output = Result<TranscriptionResult, AppError>> + Send {
        let state = self.state.clone();
        let result = self.finish_result.clone();
        async move {
            let mut log = state.log.lock();
            log.events.push(Event::Finish);
            log.finish_count += 1;
            Ok(result)
        }
    }

    fn cancel(&mut self) -> impl Future<Output = Result<(), AppError>> + Send {
        let state = self.state.clone();
        async move {
            let mut log = state.log.lock();
            log.events.push(Event::Cancel);
            log.cancel_count += 1;
            Ok(())
        }
    }
}

#[derive(Debug)]
struct UpdateObservation {
    text: String,
    language: Option<String>,
    sample_count: usize,
    is_final: bool,
}

fn update_collector() -> (
    Arc<Mutex<Vec<UpdateObservation>>>,
    impl Fn(NativeStreamUpdate) + Send,
) {
    let updates = Arc::new(Mutex::new(Vec::new()));
    let collected = updates.clone();
    let on_update = move |update: NativeStreamUpdate| {
        collected.lock().push(UpdateObservation {
            text: update.text,
            language: update.language,
            sample_count: update.sample_count,
            is_final: update.is_final,
        });
    };
    (updates, on_update)
}

fn samples(count: usize) -> Vec<i16> {
    (0..count)
        .map(|index| ((index % 2000) as i16).wrapping_sub(1000))
        .collect()
}

fn expected_resampled(input: &[i16], sample_rate: u32) -> Vec<i16> {
    let mut resampler = ChunkedResampler::new(sample_rate).expect("test sample rate is valid");
    let mut output = Vec::new();
    resampler
        .process_chunk(input, &mut output)
        .expect("one-shot resampling should succeed");
    resampler
        .finish(&mut output)
        .expect("one-shot resampling flush should succeed");
    output
}

#[tokio::test]
async fn finish_before_first_tick_drains_all_samples_in_bounded_chunks() {
    let input = samples(5121);
    let captured = Arc::new(Mutex::new(input.clone()));
    let state = FakeState::new();
    let sink = FakeSink::new(state.clone(), false, false);
    let (control_tx, control_rx) = oneshot::channel();
    let _ = control_tx.send(NativeCaptureControl::Finish);
    let (updates, on_update) = update_collector();

    let result = run_capture(sink, captured, 16_000, control_rx, on_update)
        .await
        .expect("finish should succeed")
        .expect("finish should return the native result");

    let log = state.log.lock();
    assert_eq!(
        log.feed_inputs,
        vec![input[..5120].to_vec(), input[5120..].to_vec()]
    );
    assert_eq!(log.finish_count, 1);
    assert_eq!(log.cancel_count, 0);
    assert_eq!(result.text, "finished");
    assert_eq!(result.language.as_deref(), Some("en"));

    let updates = updates.lock();
    assert_eq!(
        updates
            .iter()
            .map(|update| update.sample_count)
            .collect::<Vec<_>>(),
        vec![5120, 5121]
    );
    assert!(updates.iter().all(|update| !update.is_final));
    assert!(updates.iter().all(|update| update.language.is_none()));
    assert!(updates.iter().all(|update| update.text.starts_with("fed-")));
}

#[tokio::test]
async fn finish_flushes_persistent_resampler_tail_once() {
    let first = samples(480);
    let tail = [17_i16, -23, 41, -59, 73, -101, 127];
    let mut complete_input = first.clone();
    complete_input.extend_from_slice(&tail);
    let captured = Arc::new(Mutex::new(first));
    let state = FakeState::new();
    let sink = FakeSink::new(state.clone(), true, false);
    let (control_tx, control_rx) = oneshot::channel();
    let (updates, on_update) = update_collector();

    let task = tokio::spawn(run_capture(
        sink,
        captured.clone(),
        48_000,
        control_rx,
        on_update,
    ));
    wait_for_feed_start(&state).await;
    captured.lock().extend_from_slice(&tail);
    let _ = control_tx.send(NativeCaptureControl::Finish);
    state.release_feed.notify_one();

    let result = task
        .await
        .expect("capture task should not panic")
        .expect("finish should succeed")
        .expect("finish should return the native result");
    assert_eq!(result.text, "finished");

    let expected = expected_resampled(&complete_input, 48_000);
    let log = state.log.lock();
    let mut actual = Vec::new();
    for input in &log.feed_inputs {
        actual.extend_from_slice(input);
    }
    assert_eq!(actual, expected);
    assert!(!actual.is_empty());
    assert_eq!(log.finish_count, 1);
    assert!(matches!(log.events.last(), Some(Event::Finish)));

    let updates = updates.lock();
    assert_eq!(updates.len(), log.feed_inputs.len());
}

#[tokio::test]
async fn finish_waits_for_inflight_feed_then_drains_appended_tail_once() {
    let initial = samples(5120);
    let tail = vec![7001_i16, 7002, 7003, 7004];
    let captured = Arc::new(Mutex::new(initial.clone()));
    let state = FakeState::new();
    let sink = FakeSink::new(state.clone(), true, false);
    let (control_tx, control_rx) = oneshot::channel();
    let (updates, on_update) = update_collector();

    let task = tokio::spawn(run_capture(
        sink,
        captured.clone(),
        16_000,
        control_rx,
        on_update,
    ));
    wait_for_feed_start(&state).await;
    captured.lock().extend_from_slice(&tail);
    let _ = control_tx.send(NativeCaptureControl::Finish);
    state.release_feed.notify_one();

    let result = task
        .await
        .expect("capture task should not panic")
        .expect("finish should succeed")
        .expect("finish should return the native result");
    assert_eq!(result.text, "finished");

    let log = state.log.lock();
    assert_eq!(log.feed_inputs, vec![initial, tail]);
    assert_eq!(log.finish_count, 1);
    assert_eq!(log.cancel_count, 0);
    assert!(matches!(log.events.last(), Some(Event::Finish)));

    let updates = updates.lock();
    let counts: Vec<_> = updates.iter().map(|update| update.sample_count).collect();
    assert_eq!(counts, vec![5120, 5124]);
    assert!(counts.windows(2).all(|window| window[0] < window[1]));
}

#[tokio::test]
async fn cancel_waits_for_inflight_feed_and_dropped_control_cancels_without_finish() {
    let initial = samples(10240);
    let captured = Arc::new(Mutex::new(initial.clone()));
    let state = FakeState::new();
    let sink = FakeSink::new(state.clone(), true, false);
    let (control_tx, control_rx) = oneshot::channel();
    let (updates, on_update) = update_collector();

    let task = tokio::spawn(run_capture(
        sink,
        captured.clone(),
        16_000,
        control_rx,
        on_update,
    ));
    wait_for_feed_start(&state).await;
    let _ = control_tx.send(NativeCaptureControl::Cancel);
    state.release_feed.notify_one();

    let result = task
        .await
        .expect("capture task should not panic")
        .expect("cancel should complete");
    assert!(result.is_none());

    {
        let log = state.log.lock();
        assert_eq!(log.feed_inputs, vec![initial[..5120].to_vec()]);
        assert_eq!(log.finish_count, 0);
        assert_eq!(log.cancel_count, 1);
        assert!(matches!(log.events.last(), Some(Event::Cancel)));
        assert_eq!(updates.lock().len(), 1);
    }

    let dropped_state = FakeState::new();
    let dropped_sink = FakeSink::new(dropped_state.clone(), false, false);
    let dropped_samples = Arc::new(Mutex::new(Vec::new()));
    let (dropped_tx, dropped_rx) = oneshot::channel();
    drop(dropped_tx);
    let (_, dropped_update) = update_collector();
    let result = run_capture(
        dropped_sink,
        dropped_samples,
        16_000,
        dropped_rx,
        dropped_update,
    )
    .await
    .expect("dropped control should cancel cleanly");
    assert!(result.is_none());

    let dropped_log = dropped_state.log.lock();
    assert!(dropped_log.feed_inputs.is_empty());
    assert_eq!(dropped_log.finish_count, 0);
    assert_eq!(dropped_log.cancel_count, 1);
}

#[tokio::test]
async fn feed_error_propagates_without_retry_or_finish() {
    let input = samples(5121);
    let captured = Arc::new(Mutex::new(input.clone()));
    let state = FakeState::new();
    let sink = FakeSink::new(state.clone(), false, true);
    let (control_tx, control_rx) = oneshot::channel();
    let _ = control_tx.send(NativeCaptureControl::Finish);
    let (updates, on_update) = update_collector();

    let error = run_capture(sink, captured, 16_000, control_rx, on_update)
        .await
        .expect_err("feed error must propagate");
    assert!(error.to_string().contains("feed sentinel"));

    {
        let log = state.log.lock();
        assert_eq!(log.feed_inputs, vec![input[..5120].to_vec()]);
        assert_eq!(log.finish_count, 0);
        assert!(updates.lock().is_empty());
    }

    let zero_state = FakeState::new();
    let zero_sink = FakeSink::new(zero_state.clone(), false, false);
    let zero_samples = Arc::new(Mutex::new(vec![1_i16, -1, 2]));
    let (_zero_tx, zero_rx) = oneshot::channel();
    let (_, zero_update) = update_collector();
    let zero_error = run_capture(zero_sink, zero_samples, 0, zero_rx, zero_update)
        .await
        .expect_err("zero sample rate must fail");
    assert!(!zero_error.to_string().is_empty());

    let zero_log = zero_state.log.lock();
    assert!(zero_log.feed_inputs.is_empty());
    assert_eq!(zero_log.finish_count, 0);
    assert_eq!(zero_log.cancel_count, 1);
}
