use std::future::{pending, poll_fn, Future};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::{sleep, Instant};

use super::ai_polish_service::{
    run_cancellable_polish, run_polish_operation, POLISH_CANCELLED_ERROR,
};
use crate::state::AppState;

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[tokio::test(start_paused = true)]
async fn operation_deadline_is_shared_by_preparation_and_all_retries() {
    let state = AppState::new();
    let stages = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let operation = {
        let stages = stages.clone();
        let guard = DropFlag(dropped.clone());
        async move {
            let _guard = guard;
            // Preparation/auth and each transport attempt share one budget.
            for _ in 0..4 {
                stages.fetch_add(1, Ordering::AcqRel);
                sleep(Duration::from_secs(50)).await;
            }
            Ok("finished")
        }
    };

    let result = run_polish_operation(&state, 41, Duration::from_secs(120), operation).await;

    assert!(result.is_err());
    assert_eq!(start.elapsed(), Duration::from_secs(120));
    assert_eq!(stages.load(Ordering::Acquire), 3);
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test(start_paused = true)]
async fn operation_deadline_also_bounds_a_stalled_pre_transport_stage() {
    let state = AppState::new();
    let start = Instant::now();

    let result = run_polish_operation(
        &state,
        42,
        Duration::from_secs(120),
        pending::<Result<(), String>>(),
    )
    .await;

    assert!(result.is_err());
    assert_eq!(start.elapsed(), Duration::from_secs(120));
}

#[tokio::test(start_paused = true)]
async fn operation_preserves_results_and_clears_only_its_session_marker() {
    let state = AppState::new();
    for expected in [Ok("polished"), Err("provider failure".to_string())] {
        state.mark_ai_polish_stream_started(43);
        state.mark_ai_polish_stream_started(44);
        let operation_result = expected.clone();
        let result = run_polish_operation(&state, 43, Duration::from_secs(120), async {
            sleep(Duration::from_secs(119)).await;
            operation_result
        })
        .await;

        assert_eq!(result, expected);
        assert!(!state.take_ai_polish_stream_started(43));
        assert!(state.take_ai_polish_stream_started(44));
    }
}

#[tokio::test(start_paused = true)]
async fn operation_timeout_and_caller_drop_clear_the_partial_stream_marker() {
    let state = AppState::new();
    state.mark_ai_polish_stream_started(45);
    let result = run_polish_operation(
        &state,
        45,
        Duration::from_secs(120),
        pending::<Result<(), String>>(),
    )
    .await;
    assert!(result.is_err());
    assert!(!state.take_ai_polish_stream_started(45));

    state.mark_ai_polish_stream_started(46);
    state.mark_ai_polish_stream_started(47);
    let mut operation = Box::pin(run_polish_operation(
        &state,
        46,
        Duration::from_secs(120),
        pending::<Result<(), String>>(),
    ));
    poll_fn(|cx| {
        assert!(operation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(operation);

    assert!(!state.take_ai_polish_stream_started(46));
    assert!(state.take_ai_polish_stream_started(47));
}

#[tokio::test]
async fn explicit_skip_wins_over_a_ready_polish_result_without_polling_it() {
    let (cancel, receiver) = oneshot::channel();
    cancel.send(()).unwrap();
    let polled = AtomicBool::new(false);

    let result = run_cancellable_polish(
        async {
            polled.store(true, Ordering::Release);
            Ok("rewritten text")
        },
        Some(receiver),
    )
    .await;

    assert_eq!(result.unwrap_err(), POLISH_CANCELLED_ERROR);
    assert!(!polled.load(Ordering::Acquire));
}

#[tokio::test(start_paused = true)]
async fn explicit_skip_drops_the_running_polish_future() {
    let (cancel, receiver) = oneshot::channel();
    let dropped = Arc::new(AtomicBool::new(false));
    let guard = DropFlag(dropped.clone());
    let start = Instant::now();
    let cancellation = async {
        sleep(Duration::from_secs(3)).await;
        cancel.send(()).unwrap();
    };
    let polishing = run_cancellable_polish(
        async move {
            let _guard = guard;
            pending::<Result<String, String>>().await
        },
        Some(receiver),
    );

    let (result, ()) = tokio::join!(polishing, cancellation);

    assert_eq!(result.unwrap_err(), POLISH_CANCELLED_ERROR);
    assert_eq!(start.elapsed(), Duration::from_secs(3));
    assert!(dropped.load(Ordering::Acquire));
}

#[tokio::test(start_paused = true)]
async fn cleanup_dropping_a_sender_does_not_skip_a_later_polish_result() {
    let (cancel, receiver) = oneshot::channel();
    drop(cancel);
    let result = run_cancellable_polish(
        async {
            sleep(Duration::from_secs(2)).await;
            Ok::<_, String>("completed after cleanup")
        },
        Some(receiver),
    )
    .await;
    assert_eq!(result.unwrap(), "completed after cleanup");

    let result =
        run_cancellable_polish(async { Err::<(), _>("provider failure".to_string()) }, None).await;
    assert_eq!(result.unwrap_err(), "provider failure");
}
