// Included inside history_service::tests; every public API call runs in a child
// process whose data directory is a fresh temporary directory.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Mutex,
};
use std::time::{Duration, Instant};

static STATS_SCANS: AtomicUsize = AtomicUsize::new(0);
static STATS_SCAN_GATE: Mutex<Option<(tokio::sync::oneshot::Sender<()>, mpsc::Receiver<()>)>> =
    Mutex::new(None);

pub(super) fn record_stats_scan() {
    STATS_SCANS.fetch_add(1, Ordering::SeqCst);
}

pub(super) fn after_stats_scan() {
    let gate = STATS_SCAN_GATE.lock().unwrap().take();
    if let Some((entered, release)) = gate {
        let _ = entered.send(());
        release
            .recv_timeout(Duration::from_secs(10))
            .expect("release the paused statistics scan");
    }
}

fn stats_test_in_child(name: &str) -> bool {
    const CHILD: &str = "LIGHT_WHISPER_STATS_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        assert_eq!(
            super::paths::get_data_dir(),
            &std::path::PathBuf::from(std::env::var_os("LIGHT_WHISPER_DATA_DIR").unwrap())
        );
        return false;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "light-whisper-stats-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{module}::{name}"),
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("LIGHT_WHISPER_DATA_DIR", &directory)
        .env(CHILD, name)
        .output();
    let completed = directory.join("stats-test-completed").is_file();
    std::fs::remove_dir_all(&directory).unwrap();
    let output = output.expect("run statistics test with an isolated data directory");
    assert!(
        output.status.success() && completed,
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
    true
}

fn complete_stats_test() {
    std::fs::write(
        super::paths::get_data_dir().join("stats-test-completed"),
        [],
    )
    .unwrap();
}

fn stats_draft(session_id: u64, text: &str) -> super::HistoryDraft {
    super::HistoryDraft {
        session_id,
        mode: "dictation".into(),
        workflow: "dictation".into(),
        status: "success".into(),
        text: text.into(),
        original_text: text.into(),
        source_text: None,
        duration_sec: None,
        language: None,
        engine: "synthetic-stats-test".into(),
        provider: None,
        model: None,
        app_process: None,
        app_window_title: None,
        app_rule_name: None,
        audio_file: None,
        asr_ms: None,
        polish_ms: None,
        total_ms: None,
        error: None,
        reprocessed_from_id: None,
    }
}

async fn assert_reused_stats(expected_total: u64) -> super::HistoryStats {
    let first = super::stats().await.unwrap();
    assert_eq!(first.total, expected_total);
    let scans = STATS_SCANS.load(Ordering::SeqCst);
    let second = super::stats().await.unwrap();
    assert_eq!(
        serde_json::to_value(&second).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    assert_eq!(
        STATS_SCANS.load(Ordering::SeqCst),
        scans,
        "unchanged history must reuse statistics without another full scan"
    );
    second
}

#[tokio::test]
async fn stats_cache_reuses_exact_empty_and_nullable_statistics() {
    if stats_test_in_child("stats_cache_reuses_exact_empty_and_nullable_statistics") {
        return;
    }
    let empty = super::stats().await.unwrap();
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        serde_json::to_value(super::HistoryStats::default()).unwrap()
    );
    // Assert all correctness cases before the cache assertion, so the original
    // implementation demonstrates preserved values before failing for reuse.
    let fixtures = [
        ("success", "中🙂a", Some(0), None, Some(100), None),
        ("success", "", Some(10), Some(20), None, Some(0.0)),
        ("success", "é", Some(30), Some(40), Some(300), Some(1.25)),
        ("success", "two", Some(90), Some(0), Some(900), Some(-1.0)),
        (
            "success",
            "overflow",
            Some(u64::MAX),
            Some(u64::MAX),
            Some(u64::MAX),
            None,
        ),
        (
            "failed",
            "failed",
            Some(9_999),
            Some(9_999),
            Some(9_999),
            Some(99.0),
        ),
        ("cancelled", "!", None, None, None, None),
    ];
    for (index, (status, text, asr, polish, total, duration)) in fixtures.into_iter().enumerate() {
        let mut draft = stats_draft(index as u64, text);
        draft.status = status.into();
        draft.asr_ms = asr;
        draft.polish_ms = polish;
        draft.total_ms = total;
        draft.duration_sec = duration;
        super::insert(draft, 0).await.unwrap();
    }
    let stats = super::stats().await.unwrap();
    assert_eq!(
        serde_json::to_value(stats).unwrap(),
        serde_json::json!({
            "total": 7, "success": 5, "failed": 2, "totalCharacters": 22,
            "asr": {"p50Ms": 30, "p95Ms": 90},
            "polish": {"p50Ms": 20, "p95Ms": 40},
            "totalLatency": {"p50Ms": 300, "p95Ms": 900}
        })
    );
    assert_reused_stats(7).await;
    complete_stats_test();
}

#[tokio::test]
async fn stats_cache_invalidates_after_insert_delete_and_both_retention_paths() {
    if stats_test_in_child("stats_cache_invalidates_after_insert_delete_and_both_retention_paths") {
        return;
    }
    assert_eq!(super::stats().await.unwrap().total, 0);
    let old = super::insert(stats_draft(1, "old"), 0).await.unwrap();
    assert_eq!(super::stats().await.unwrap().total, 1);
    let deleted = super::insert(stats_draft(2, "deleted"), 0).await.unwrap();
    assert_eq!(super::stats().await.unwrap().total, 2);
    assert!(super::delete(deleted).await.unwrap());
    assert_eq!(super::stats().await.unwrap().total, 1);
    assert!(!super::delete(deleted).await.unwrap());

    let connection = super::open_connection().unwrap();
    connection
        .execute(
            "UPDATE transcription_history SET created_at = 1 WHERE id = ?1",
            [old],
        )
        .unwrap();
    assert_eq!(super::stats().await.unwrap().total_characters, 3);
    super::insert(stats_draft(3, "fresh"), 1).await.unwrap();
    let after_insert_cleanup = super::stats().await.unwrap();
    assert_eq!(after_insert_cleanup.total, 1);
    assert_eq!(after_insert_cleanup.total_characters, 5);
    assert!(super::get(old).await.unwrap().is_none());

    let expired = super::insert(stats_draft(4, "expired"), 0).await.unwrap();
    connection
        .execute(
            "UPDATE transcription_history SET created_at = 1 WHERE id = ?1",
            [expired],
        )
        .unwrap();
    assert_eq!(super::stats().await.unwrap().total, 2);
    assert_eq!(super::cleanup(0).await.unwrap(), 0);
    assert_eq!(super::cleanup(1).await.unwrap(), 1);
    let after_cleanup = super::stats().await.unwrap();
    assert_eq!(after_cleanup.total, 1);
    assert_eq!(after_cleanup.total_characters, 5);
    assert_eq!(super::cleanup(1).await.unwrap(), 0);
    assert_reused_stats(1).await;
    complete_stats_test();
}

#[tokio::test]
async fn stats_cache_recovers_from_failed_mutations_and_partial_insert_cleanup() {
    if stats_test_in_child("stats_cache_recovers_from_failed_mutations_and_partial_insert_cleanup")
    {
        return;
    }
    let old = super::insert(stats_draft(1, "old"), 0).await.unwrap();
    let connection = super::open_connection().unwrap();
    connection
        .execute(
            "UPDATE transcription_history SET created_at = 1 WHERE id = ?1",
            [old],
        )
        .unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_stats_insert BEFORE INSERT ON transcription_history
         BEGIN SELECT RAISE(ABORT, 'synthetic insertion failure'); END;
         CREATE TRIGGER reject_stats_delete BEFORE DELETE ON transcription_history
         BEGIN SELECT RAISE(ABORT, 'synthetic deletion failure'); END;",
        )
        .unwrap();
    assert_eq!(super::stats().await.unwrap().total, 1);
    assert!(super::insert(stats_draft(2, "rejected"), 0).await.is_err());
    assert!(super::delete(old).await.is_err());
    assert!(super::cleanup(1).await.is_err());
    assert_eq!(super::stats().await.unwrap().total, 1);

    connection
        .execute_batch("DROP TRIGGER reject_stats_insert;")
        .unwrap();
    assert_eq!(super::stats().await.unwrap().total, 1);
    super::insert(stats_draft(3, "fresh"), 1).await.unwrap();
    let partially_successful = super::stats().await.unwrap();
    assert_eq!(partially_successful.total, 2);
    assert_eq!(partially_successful.total_characters, 8);
    connection
        .execute_batch("DROP TRIGGER reject_stats_delete;")
        .unwrap();
    assert_eq!(super::cleanup(1).await.unwrap(), 1);
    let recovered = assert_reused_stats(1).await;
    assert_eq!(recovered.total_characters, 5);
    complete_stats_test();
}

#[tokio::test]
async fn stats_cache_observes_external_updates_without_row_count_changes() {
    if stats_test_in_child("stats_cache_observes_external_updates_without_row_count_changes") {
        return;
    }
    let mut draft = stats_draft(1, "before");
    draft.asr_ms = Some(10);
    super::insert(draft, 0).await.unwrap();
    let before = super::stats().await.unwrap();
    assert_eq!(before.total, 1);
    assert_eq!(before.asr.p50_ms, Some(10));
    let connection = super::open_connection().unwrap();
    connection
        .execute_batch("UPDATE transcription_history SET text = '改', asr_ms = 90, polish_ms = -1;")
        .unwrap();
    let changed = super::stats().await.unwrap();
    assert_eq!(changed.total, 1);
    assert_eq!(changed.total_characters, 1);
    assert_eq!(changed.asr.p50_ms, Some(90));
    assert_eq!(changed.polish.p50_ms, None);
    connection
        .execute_batch(
            "UPDATE transcription_history SET status = 'failed';
         PRAGMA wal_checkpoint(TRUNCATE);",
        )
        .unwrap();
    let failed = super::stats().await.unwrap();
    assert_eq!((failed.success, failed.failed), (0, 1));
    assert_eq!(failed.asr.p50_ms, None);
    connection
        .execute_batch("DELETE FROM transcription_history;")
        .unwrap();
    let empty = assert_reused_stats(0).await;
    assert_eq!(
        serde_json::to_value(empty).unwrap(),
        serde_json::to_value(super::HistoryStats::default()).unwrap()
    );
    complete_stats_test();
}

#[tokio::test]
async fn stats_cache_does_not_publish_a_stale_scan_after_external_commit() {
    if stats_test_in_child("stats_cache_does_not_publish_a_stale_scan_after_external_commit") {
        return;
    }
    super::insert(stats_draft(1, "old"), 0).await.unwrap();
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *STATS_SCAN_GATE.lock().unwrap() = Some((entered_tx, release_rx));
    let calculating = tokio::spawn(super::stats());
    tokio::time::timeout(Duration::from_secs(10), entered_rx)
        .await
        .expect("statistics scan reaches publication boundary")
        .unwrap();
    let connection = super::open_connection().unwrap();
    let committed = connection.execute(
        "UPDATE transcription_history SET text = 'new text', asr_ms = 90",
        [],
    );
    release_tx.send(()).unwrap();
    committed.unwrap();
    // The overlapping call may return either snapshot. Only calls beginning
    // after the external commit are required to see its new values.
    calculating.await.unwrap().unwrap();
    let fresh = assert_reused_stats(1).await;
    assert_eq!(fresh.total_characters, 8);
    assert_eq!(fresh.asr.p50_ms, Some(90));
    complete_stats_test();
}

#[tokio::test]
#[ignore = "explicit synthetic workload measurement; no timing threshold"]
async fn stats_cache_synthetic_workload_measurement() {
    if stats_test_in_child("stats_cache_synthetic_workload_measurement") {
        return;
    }
    super::initialize().await.unwrap();
    let mut connection = super::open_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO transcription_history
             (session_id, created_at, updated_at, mode, status, text, original_text,
              engine, asr_ms, polish_ms, total_ms)
             VALUES (?1, 1, 1, 'dictation', ?2, ?3, ?3, 'synthetic-benchmark', ?4, ?5, ?6)",
            )
            .unwrap();
        for index in 0..25_000_i64 {
            statement
                .execute(params![
                    index,
                    if index % 10 == 0 { "failed" } else { "success" },
                    "Synthetic transcription sample 中".repeat(8),
                    (index * 37) % 5_000,
                    if index % 3 == 0 {
                        None
                    } else {
                        Some((index * 19) % 2_000)
                    },
                    (index * 53) % 7_000
                ])
                .unwrap();
        }
    }
    transaction.commit().unwrap();
    let start = Instant::now();
    let first = super::stats().await.unwrap();
    let cold_us = start.elapsed().as_micros();
    assert_eq!(first.total, 25_000);
    assert_eq!(first.success, 22_500);
    let scans_before = STATS_SCANS.load(Ordering::SeqCst);
    let mut times = Vec::new();
    for _ in 0..30 {
        let start = Instant::now();
        let value = super::stats().await.unwrap();
        times.push(start.elapsed().as_micros());
        assert_eq!(
            serde_json::to_value(value).unwrap(),
            serde_json::to_value(&first).unwrap()
        );
    }
    times.sort_unstable();
    println!(
        "SYNTHETIC_HISTORY_STATS rows=25000 repeated_calls=30 cold_us={cold_us} \
         repeated_median_us={} repeated_p95_us={} repeated_scans={}",
        times[15],
        times[28],
        STATS_SCANS.load(Ordering::SeqCst) - scans_before
    );
    complete_stats_test();
}
