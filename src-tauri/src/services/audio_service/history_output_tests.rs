use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use super::output::settle_history_output;

#[test]
fn history_audio_snapshot_survives_asr_drain_and_encodes_the_original_samples_and_rate() {
    let original = vec![i16::MIN, -1234, 0, 1234, i16::MAX];
    let samples = parking_lot::Mutex::new(original.clone());
    let archived = super::finalize::history_audio_snapshot(&samples, true).unwrap();
    let asr_input = std::mem::take(&mut *samples.lock());
    assert_eq!(asr_input, original);
    assert!(samples.lock().is_empty());

    let wav = super::wav::encode_wav(&archived, 48_000).unwrap();
    let mut reader = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
    assert_eq!(reader.spec().sample_rate, 48_000);
    assert_eq!(reader.spec().channels, 1);
    assert_eq!(
        reader
            .samples::<i16>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        original
    );
}

#[test]
fn history_audio_snapshot_is_absent_when_saving_is_disabled() {
    let original = vec![i16::MIN, 0, i16::MAX];
    let samples = parking_lot::Mutex::new(original.clone());
    assert!(super::finalize::history_audio_snapshot(&samples, false).is_none());
    assert_eq!(*samples.lock(), original);
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[tokio::test]
async fn history_insert_runs_while_output_waits_for_the_next_recording_to_finish() {
    let (inserted, insert) = tokio::sync::oneshot::channel();
    let (pasted, receipt) = tokio::sync::oneshot::channel();
    let persist_started = Cell::new(false);
    let published = RefCell::new(Vec::new());
    let updated = RefCell::new(Vec::new());
    let persist = async {
        persist_started.set(true);
        insert.await.unwrap()
    };
    let mut task = Box::pin(settle_history_output(
        persist,
        Some(receipt),
        |total| published.borrow_mut().push(total),
        |id, total| {
            updated.borrow_mut().push((id, total));
            std::future::ready(Ok(true))
        },
    ));
    assert!(poll_once(task.as_mut()).is_pending());
    assert!(persist_started.get());
    inserted.send(Ok(Some(73))).unwrap();
    assert!(poll_once(task.as_mut()).is_pending());
    assert!(published.borrow().is_empty());
    assert!(updated.borrow().is_empty());

    pasted.send(Some(460)).unwrap();
    task.await.unwrap();
    assert_eq!(*published.borrow(), vec![460]);
    assert_eq!(*updated.borrow(), vec![(73, 460)]);
}

#[tokio::test]
async fn output_timing_publishes_before_a_blocked_history_insert_and_updates_its_row_later() {
    let (inserted, insert) = tokio::sync::oneshot::channel();
    let (pasted, receipt) = tokio::sync::oneshot::channel();
    let published = RefCell::new(Vec::new());
    let updated = RefCell::new(Vec::new());
    let mut task = Box::pin(settle_history_output(
        async { insert.await.unwrap() },
        Some(receipt),
        |total| published.borrow_mut().push(total),
        |id, total| {
            updated.borrow_mut().push((id, total));
            std::future::ready(Ok(true))
        },
    ));
    assert!(poll_once(task.as_mut()).is_pending());
    pasted.send(Some(460)).unwrap();
    assert!(poll_once(task.as_mut()).is_pending());
    assert_eq!(*published.borrow(), vec![460]);
    assert!(updated.borrow().is_empty());

    inserted.send(Ok(Some(73))).unwrap();
    task.await.unwrap();
    assert_eq!(*published.borrow(), vec![460]);
    assert_eq!(*updated.borrow(), vec![(73, 460)]);
}

#[tokio::test]
async fn history_insert_failure_cannot_discard_later_successful_output_timing() {
    let (pasted, receipt) = tokio::sync::oneshot::channel();
    let published = RefCell::new(Vec::new());
    let updated = RefCell::new(Vec::new());
    let mut task = Box::pin(settle_history_output(
        std::future::ready(Err("disk unavailable".to_string())),
        Some(receipt),
        |total| published.borrow_mut().push(total),
        |id, total| {
            updated.borrow_mut().push((id, total));
            std::future::ready(Ok(true))
        },
    ));
    assert!(poll_once(task.as_mut()).is_pending());
    pasted.send(Some(460)).unwrap();
    assert_eq!(task.await, Err("disk unavailable".to_string()));
    assert_eq!(*published.borrow(), vec![460]);
    assert!(updated.borrow().is_empty());
}

#[tokio::test]
async fn disabled_history_still_publishes_actual_output_timing_without_an_update() {
    let (pasted, receipt) = tokio::sync::oneshot::channel();
    let published = RefCell::new(Vec::new());
    pasted.send(Some(460)).unwrap();
    settle_history_output(
        std::future::ready(Ok(None)),
        Some(receipt),
        |total| published.borrow_mut().push(total),
        |_, _| async { panic!("disabled history must not update a row") },
    )
    .await
    .unwrap();
    assert_eq!(*published.borrow(), vec![460]);
}

#[tokio::test]
async fn failed_or_dropped_output_receipts_never_publish_success_or_update_history_timing() {
    for dropped in [false, true] {
        let (pasted, receipt) = tokio::sync::oneshot::channel();
        if dropped {
            drop(pasted);
        } else {
            pasted.send(None).unwrap();
        }
        settle_history_output(
            std::future::ready(Ok(Some(73))),
            Some(receipt),
            |_| panic!("unsuccessful output must not publish successful timing"),
            |_, _| async { panic!("unsuccessful output must not update history timing") },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn history_without_an_automatic_output_receipt_only_persists() {
    let persisted = Cell::new(false);
    settle_history_output(
        async {
            persisted.set(true);
            Ok(Some(73))
        },
        None,
        |_| panic!("manual output must not publish automatic paste timing"),
        |_, _| async { panic!("manual output must not update automatic paste timing") },
    )
    .await
    .unwrap();
    assert!(persisted.get());
}
