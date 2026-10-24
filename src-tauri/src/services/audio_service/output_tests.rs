use crate::services::audio_service::output::{PendingPaste, PendingPasteQueue};

fn output(session_id: u64, target_window: Option<isize>, text: &str) -> PendingPaste {
    PendingPaste {
        session_id,
        target_window,
        text: text.into(),
    }
}

#[test]
fn output_waits_for_the_recording_slot_to_be_released() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "Dictation A"));

    // Starting and active slots both count as occupied; repeated flushes must retain A.
    assert!(queue.take_if_idle(true).is_empty());
    assert!(queue.take_if_idle(true).is_empty());

    let ready = queue.take_if_idle(false);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].session_id, 41);
    assert_eq!(ready[0].target_window, Some(100));
    assert_eq!(ready[0].text, "Dictation A");
}

#[test]
fn output_is_retired_after_one_idle_drain() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "Dictation A"));

    assert_eq!(queue.take_if_idle(false).len(), 1);
    assert!(queue.take_if_idle(false).is_empty());
    assert!(queue.take_if_idle(true).is_empty());
    assert!(queue.take_if_idle(false).is_empty());
}

#[test]
fn output_from_different_sessions_is_not_combined() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "  Dictation A\n"));
    queue.push(output(43, Some(200), "Dictation C!  "));

    let ready: Vec<_> = queue
        .take_if_idle(false)
        .into_iter()
        .map(|item| (item.session_id, item.target_window, item.text))
        .collect();
    assert_eq!(
        ready,
        vec![
            (41, Some(100), "  Dictation A\n".to_string()),
            (43, Some(200), "Dictation C!  ".to_string()),
        ]
    );
}

#[test]
fn output_deduplicates_a_pending_session_without_deduplicating_other_sessions() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "Repeated words"));
    queue.push(output(41, Some(100), "Repeated words"));
    queue.push(output(43, Some(100), "Repeated words"));

    let sessions: Vec<_> = queue
        .take_if_idle(false)
        .into_iter()
        .map(|item| item.session_id)
        .collect();
    assert_eq!(sessions, vec![41, 43]);
}

#[test]
fn output_deduplicates_by_session_when_content_and_target_differ() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "First content"));
    queue.push(output(41, Some(200), "Changed content"));

    let ready = queue.take_if_idle(false);
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].session_id, 41);
}

#[test]
fn output_can_enqueue_the_same_session_again_after_its_pending_item_is_drained() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "First content"));
    let first = queue.take_if_idle(false);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].session_id, 41);

    queue.push(output(41, Some(200), "New pending content"));
    let second = queue.take_if_idle(false);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].session_id, 41);
    assert_eq!(second[0].target_window, Some(200));
    assert_eq!(second[0].text, "New pending content");
    assert!(queue.take_if_idle(false).is_empty());
}

#[test]
fn output_only_matches_a_known_original_window() {
    let known = output(41, Some(100), "Dictation A");
    let unknown = output(43, None, "Dictation C");

    assert!(known.target_matches(Some(100)));
    assert!(!known.target_matches(Some(200)));
    assert!(!known.target_matches(None));
    assert!(!unknown.target_matches(Some(100)));
    assert!(!unknown.target_matches(None));
}

#[test]
fn output_keeps_original_content_available_when_target_check_rejects_it() {
    let mut queue = PendingPasteQueue::default();
    queue.push(output(41, Some(100), "  原始听写\nOriginal punctuation!  "));

    let ready = queue.take_if_idle(false);
    assert!(!ready[0].target_matches(Some(200)));
    assert_eq!(ready[0].session_id, 41);
    assert_eq!(ready[0].text, "  原始听写\nOriginal punctuation!  ");
    assert!(queue.take_if_idle(false).is_empty());
}

mod completion_tests {
    use super::*;
    use crate::services::audio_service::finalize::drain_pending_pastes;
    use crate::state::{AppState, PendingRecordingSession, RecordingSlot, RecordingTrigger};
    use std::cell::{Cell, RefCell};
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};
    use tokio::sync::oneshot::error::TryRecvError;

    fn started() -> Instant {
        tokio::time::Instant::now().into_std()
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    #[tokio::test(start_paused = true)]
    async fn output_receipt_waits_through_busy_and_idle_drains_until_successful_settlement() {
        let mut queue = PendingPasteQueue::default();
        let mut receipt = queue.push_with_completion(output(41, Some(100), "Original"), started());
        assert!(queue.take_if_idle(true).is_empty());
        assert!(matches!(receipt.try_recv(), Err(TryRecvError::Empty)));
        tokio::time::advance(Duration::from_millis(300)).await;
        let ready = queue.take_if_idle(false);
        assert_eq!(ready.len(), 1);
        assert!(matches!(receipt.try_recv(), Err(TryRecvError::Empty)));

        tokio::time::advance(Duration::from_millis(60)).await;
        queue.finish_output(41, true);
        assert_eq!(receipt.await.unwrap(), Some(360));
        queue.finish_output(41, true);
        assert!(queue.take_if_idle(false).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn tracked_duplicates_preserve_first_output_and_receipt_even_after_drain() {
        for already_drained in [false, true] {
            let mut queue = PendingPasteQueue::default();
            let first = queue.push_with_completion(output(41, Some(100), "Original"), started());
            tokio::time::advance(Duration::from_millis(500)).await;
            let ready = if already_drained {
                queue.take_if_idle(false)
            } else {
                Vec::new()
            };
            let duplicate =
                queue.push_with_completion(output(41, Some(200), "Replacement"), started());
            assert_eq!(duplicate.await.unwrap(), None);
            let ready = if already_drained {
                assert!(queue.take_if_idle(false).is_empty());
                ready
            } else {
                queue.take_if_idle(false)
            };
            assert_eq!(ready.len(), 1);
            assert_eq!(ready[0].text, "Original");
            assert_eq!(ready[0].target_window, Some(100));
            queue.finish_output(41, true);
            assert_eq!(first.await.unwrap(), Some(500));

            let next = queue.push_with_completion(output(41, Some(200), "New output"), started());
            assert_eq!(queue.take_if_idle(false)[0].text, "New output");
            queue.finish_output(41, false);
            assert_eq!(next.await.unwrap(), None);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn actual_drain_receipt_waits_for_native_paste_after_the_recording_slot_is_released() {
        let state = AppState::default();
        *state.recording.recording.lock() =
            Some(RecordingSlot::Starting(PendingRecordingSession {
                session_id: 42,
                subtitle_show_gen: 1,
                trigger: RecordingTrigger::Assistant,
                stop_flag: Default::default(),
                stop_notify: Default::default(),
            }));
        let mut receipt = state
            .recording
            .pending_paste
            .lock()
            .push_with_completion(output(41, Some(100), "  Original\n"), started());
        let calls = Cell::new(0);
        let (settle, waiting) = tokio::sync::oneshot::channel();
        let waiting = RefCell::new(Some(waiting));
        let paste = |item: PendingPaste| {
            calls.set(calls.get() + 1);
            assert_eq!(item.text, "  Original\n");
            let waiting = waiting.borrow_mut().take().unwrap();
            async move { waiting.await.unwrap() }
        };
        assert!(drain_pending_pastes(&state, || Some(100), &paste)
            .await
            .is_empty());
        assert_eq!(calls.get(), 0);
        assert!(matches!(receipt.try_recv(), Err(TryRecvError::Empty)));
        tokio::time::advance(Duration::from_millis(200)).await;

        *state.recording.recording.lock() = None;
        tokio::time::advance(Duration::from_millis(60)).await;
        let mut drain = Box::pin(drain_pending_pastes(&state, || Some(100), paste));
        assert!(poll_once(drain.as_mut()).is_pending());
        assert_eq!(calls.get(), 1);
        assert!(matches!(receipt.try_recv(), Err(TryRecvError::Empty)));
        tokio::time::advance(Duration::from_millis(140)).await;
        settle.send(Ok(())).unwrap();
        assert!(drain.await.is_empty());
        assert_eq!(receipt.await.unwrap(), Some(400));
    }

    #[tokio::test]
    async fn actual_drain_settles_failed_and_mismatched_outputs_without_successful_timing() {
        for (target, initial_window, changed_window, reason) in [
            (None, Some(100), Some(100), "target_unknown"),
            (Some(100), Some(200), Some(200), "target_changed"),
            (Some(100), Some(100), Some(100), "paste_failed"),
            (Some(100), Some(100), Some(200), "target_changed"),
        ] {
            let state = AppState::default();
            let receipt = state
                .recording
                .pending_paste
                .lock()
                .push_with_completion(output(41, target, "  Original punctuation!\n"), started());
            let foreground = Cell::new(initial_window);
            let calls = Cell::new(0);
            let recovery = drain_pending_pastes(
                &state,
                || foreground.get(),
                |_| {
                    calls.set(calls.get() + 1);
                    foreground.set(changed_window);
                    std::future::ready(Err("native failure".to_string()))
                },
            )
            .await;
            assert_eq!(receipt.await.unwrap(), None);
            assert_eq!(recovery.len(), 1);
            assert_eq!(recovery[0].1, reason);
            assert_eq!(recovery[0].0.text, "  Original punctuation!\n");
            assert_eq!(calls.get(), usize::from(target == initial_window));
            assert!(state
                .recording
                .pending_paste
                .lock()
                .take_if_idle(false)
                .is_empty());
        }
    }
}

// Formal replay ownership: separate append-only test module.
mod async_ownership_replay_tests {
    use super::*;
    use crate::services::audio_service::finalize::drain_pending_pastes;
    use crate::state::{AppState, PendingRecordingSession, RecordingSlot, RecordingTrigger};
    mod trace {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test-support/async_ownership_trace.rs"
        ));
    }

    fn reserve_assistant(state: &AppState) {
        *state.recording.recording.lock() =
            Some(RecordingSlot::Starting(PendingRecordingSession {
                session_id: 42,
                subtitle_show_gen: 1,
                trigger: RecordingTrigger::Assistant,
                stop_flag: Default::default(),
                stop_notify: Default::default(),
            }));
    }

    #[tokio::test]
    async fn tlc_output_replays_actual_drain_controller_and_foreground_recovery() {
        for name in [
            "output-drain",
            "output-success",
            "output-start-error",
            "output-target",
        ] {
            let events = trace::load(name);
            let state = AppState::default();
            let foreground = std::cell::Cell::new(Some(100));
            let accepted = std::cell::RefCell::new(Vec::new());
            let mut recoveries = Vec::new();
            let mut handled = 0;
            for event in &events {
                match event.as_str() {
                    "output:B:start" | "output:B:recording" => {
                        reserve_assistant(&state);
                    }
                    "output:A:complete" => {
                        state.recording.pending_paste.lock().push(output(
                            41,
                            Some(100),
                            "Dictation A",
                        ));
                        assert!(drain_pending_pastes(
                            &state,
                            || foreground.get(),
                            |item| {
                                accepted.borrow_mut().push(item);
                                std::future::ready(Ok(()))
                            }
                        )
                        .await
                        .is_empty());
                        assert!(accepted.borrow().is_empty());
                    }
                    "output:target:changed" => {
                        foreground.set(Some(200));
                    }
                    "output:B:exit:cancel"
                    | "output:B:exit:error"
                    | "output:B:exit:success"
                    | "output:B:exit:start_error" => {
                        *state.recording.recording.lock() = None;
                        // Terminal handlers schedule this same production controller.
                        recoveries.extend(
                            drain_pending_pastes(
                                &state,
                                || foreground.get(),
                                |item| {
                                    accepted.borrow_mut().push(item);
                                    std::future::ready(Ok(()))
                                },
                            )
                            .await,
                        );
                    }
                    "output:drain" => {
                        recoveries.extend(
                            drain_pending_pastes(
                                &state,
                                || foreground.get(),
                                |item| {
                                    accepted.borrow_mut().push(item);
                                    std::future::ready(Ok(()))
                                },
                            )
                            .await,
                        );
                    }
                    _ => panic!("unmapped TLC output event: {event}"),
                }
                handled += 1;
            }
            assert_eq!(handled, events.len());
            if name == "output-target" {
                assert!(accepted.borrow().is_empty());
                assert_eq!(recoveries.len(), 1);
                assert_eq!(recoveries[0].1, "target_changed");
                let item = &recoveries[0].0;
                assert_eq!(
                    (item.session_id, item.target_window, item.text.as_str()),
                    (41, Some(100), "Dictation A")
                );
            } else {
                assert!(recoveries.is_empty());
                let items = accepted.borrow();
                assert_eq!(items.len(), 1);
                assert_eq!(
                    (
                        items[0].session_id,
                        items[0].target_window,
                        items[0].text.as_str()
                    ),
                    (41, Some(100), "Dictation A")
                );
            }
            assert!(state
                .recording
                .pending_paste
                .lock()
                .take_if_idle(false)
                .is_empty());
        }
    }

    #[tokio::test]
    async fn output_controller_recovers_exact_content_after_native_failure() {
        let state = AppState::default();
        let text = "  原始听写\nOriginal punctuation!  ";
        state
            .recording
            .pending_paste
            .lock()
            .push(output(41, Some(100), text));
        let recovery = drain_pending_pastes(
            &state,
            || Some(100),
            |_| async { Err("native paste failed".into()) },
        )
        .await;
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].1, "paste_failed");
        assert_eq!(recovery[0].0.text, text);
        assert_eq!(recovery[0].0.session_id, 41);
        assert!(state
            .recording
            .pending_paste
            .lock()
            .take_if_idle(false)
            .is_empty());
    }

    #[tokio::test]
    async fn output_controller_serializes_recording_reservation_with_pending_native_paste() {
        let state = AppState::default();
        state
            .recording
            .pending_paste
            .lock()
            .push(output(41, Some(100), "Dictation A"));
        let (finish, waiting) = tokio::sync::oneshot::channel::<()>();
        let waiting = std::cell::RefCell::new(Some(waiting));
        let mut drain = Box::pin(drain_pending_pastes(
            &state,
            || Some(100),
            |_| {
                let waiting = waiting.borrow_mut().take().unwrap();
                async move {
                    waiting.await.unwrap();
                    Ok(())
                }
            },
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), drain.as_mut())
                .await
                .is_err()
        );
        assert!(
            state.recording.paste_dispatch.try_lock().is_err(),
            "recording reservation must wait for native paste"
        );
        finish.send(()).unwrap();
        assert!(drain.await.is_empty());
        assert!(state.recording.paste_dispatch.try_lock().is_ok());
    }

    #[test]
    fn terminal_output_paths_reach_the_shared_drain_scheduler() {
        let source = include_str!("finalize.rs");
        for function in ["fn emit_done_with_timing(", "fn emit_terminal_outcome("] {
            let body = source
                .split(function)
                .nth(1)
                .unwrap()
                .split("\nfn ")
                .next()
                .unwrap();
            assert!(
                body.contains("flush_pending_paste(app)"),
                "{function} must schedule queued output even after assistant work"
            );
        }
        let source = include_str!("../../commands/audio.rs");
        let startup = source
            .split("fn emit_start_error(")
            .nth(1)
            .unwrap()
            .split("pub(crate) async fn start_recording_inner(")
            .next()
            .unwrap();
        assert!(startup.contains("audio_service::flush_pending_paste(app_handle)"));
        let stop = source
            .split("async fn stop_recording_inner(")
            .nth(1)
            .unwrap()
            .split("let session = match recording {")
            .next()
            .unwrap();
        assert!(stop.contains("audio_service::flush_pending_paste(&app_handle)"));
    }
}
