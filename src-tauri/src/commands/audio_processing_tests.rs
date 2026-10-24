use std::sync::atomic::Ordering;

use tokio::sync::oneshot::error::TryRecvError;

use crate::state::app_state::{RecordingMode, RecordingPhase, RecordingState};

fn transition(state: &RecordingState, session_id: u64, phase: RecordingPhase, mode: RecordingMode) {
    state.session_counter.store(session_id, Ordering::Release);
    state
        .transition_snapshot_if_current(session_id, phase, mode, None, None)
        .unwrap();
}

#[test]
fn skip_requires_the_current_processing_dictation_session() {
    let state = RecordingState::default();
    assert!(state.begin_dictation_polish(0).is_none());
    assert!(!state.cancel_dictation_polish(0));
    transition(
        &state,
        0,
        RecordingPhase::Processing,
        RecordingMode::Dictation,
    );
    assert!(state.begin_dictation_polish(0).is_none());
    assert!(!state.cancel_dictation_polish(0));
    for (phase, mode) in [
        (RecordingPhase::Idle, RecordingMode::Dictation),
        (RecordingPhase::Starting, RecordingMode::Dictation),
        (RecordingPhase::Recording, RecordingMode::Dictation),
        (RecordingPhase::Outcome, RecordingMode::Dictation),
        (RecordingPhase::Processing, RecordingMode::Assistant),
    ] {
        transition(&state, 51, phase, mode);
        assert!(state.begin_dictation_polish(51).is_none());
        assert!(!state.cancel_dictation_polish(51));
    }

    transition(
        &state,
        52,
        RecordingPhase::Processing,
        RecordingMode::Dictation,
    );
    assert!(state.begin_dictation_polish(51).is_none());
    assert!(!state.cancel_dictation_polish(51));
    let mut receiver = state.begin_dictation_polish(52).unwrap();
    assert!(state.cancel_dictation_polish(52));
    assert_eq!(receiver.try_recv(), Ok(()));
    assert!(!state.cancel_dictation_polish(52));
    assert_eq!(state.snapshot().unwrap().phase, RecordingPhase::Processing);
}

#[test]
fn stale_skip_and_old_cleanup_cannot_cancel_the_new_session() {
    let state = RecordingState::default();
    transition(
        &state,
        53,
        RecordingPhase::Processing,
        RecordingMode::Dictation,
    );
    let mut old_receiver = state.begin_dictation_polish(53).unwrap();
    transition(
        &state,
        54,
        RecordingPhase::Processing,
        RecordingMode::Dictation,
    );
    let mut new_receiver = state.begin_dictation_polish(54).unwrap();

    assert!(!state.cancel_dictation_polish(53));
    assert_eq!(old_receiver.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(new_receiver.try_recv(), Err(TryRecvError::Empty));
    state.clear_dictation_polish(53);
    assert_eq!(old_receiver.try_recv(), Err(TryRecvError::Closed));
    assert_eq!(new_receiver.try_recv(), Err(TryRecvError::Empty));
    assert!(state.cancel_dictation_polish(54));
    assert_eq!(new_receiver.try_recv(), Ok(()));
}

#[test]
fn a_registered_sender_stops_accepting_skip_after_processing_ends() {
    let state = RecordingState::default();
    transition(
        &state,
        55,
        RecordingPhase::Processing,
        RecordingMode::Dictation,
    );
    let mut receiver = state.begin_dictation_polish(55).unwrap();
    transition(&state, 55, RecordingPhase::Idle, RecordingMode::Dictation);

    assert!(!state.cancel_dictation_polish(55));
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
    state.clear_dictation_polish(55);
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Closed));
}
