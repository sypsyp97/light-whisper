// Reuse the public-API fixtures and child-process data isolation in this module.
#[tokio::test]
async fn history_gc_retains_each_live_owner_and_collects_only_after_the_last_one() {
    if stats_test_in_child(
        "history_gc_retains_each_live_owner_and_collects_only_after_the_last_one",
    ) {
        return;
    }
    let wav = b"two-reprocess ownership fixture".to_vec();
    let audio_file = super::save_audio(511, wav.clone()).await.unwrap();
    let mut source = stats_draft(511, "source");
    source.audio_file = Some(audio_file.clone());
    let source_id = super::insert(source, 0).await.unwrap();
    for _ in 0..2 {
        assert!(
            super::get_for_reprocess(source_id)
                .await
                .unwrap()
                .unwrap()
                .record
                .audio_available
        );
    }

    assert!(super::delete(source_id).await.unwrap());
    assert_eq!(super::read_audio(&audio_file).await.unwrap(), wav);
    super::release_audio_lease(audio_file.clone())
        .await
        .unwrap();
    assert_eq!(super::read_audio(&audio_file).await.unwrap(), wav);

    let mut replacement = stats_draft(512, "completed reprocess");
    replacement.audio_file = Some(audio_file.clone());
    replacement.reprocessed_from_id = Some(source_id);
    let replacement_id = super::insert(replacement, 0).await.unwrap();
    super::release_audio_lease(audio_file.clone())
        .await
        .unwrap();
    assert!(
        super::get(replacement_id)
            .await
            .unwrap()
            .unwrap()
            .record
            .audio_available
    );
    assert_eq!(super::read_audio(&audio_file).await.unwrap(), wav);

    assert!(super::delete(replacement_id).await.unwrap());
    assert!(!super::safe_audio_path(&audio_file).unwrap().exists());
    complete_stats_test();
}

#[test]
fn history_gc_keeps_audio_when_reprocess_finishes_between_ownership_reads() {
    if stats_test_in_child("history_gc_keeps_audio_when_reprocess_finishes_between_ownership_reads")
    {
        return;
    }

    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let runtime = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap(),
    );
    let wav = b"isolated GC fixture; decoding is outside this ownership test".to_vec();
    let audio_file = runtime
        .block_on(super::save_audio(501, wav.clone()))
        .unwrap();
    let mut source = stats_draft(501, "source text");
    source.audio_file = Some(audio_file.clone());
    let source_id = runtime.block_on(super::insert(source, 0)).unwrap();
    let leased = runtime
        .block_on(super::get_for_reprocess(source_id))
        .unwrap()
        .unwrap();
    assert!(leased.record.audio_available);

    // delete() commits this row deletion before calling the same GC function.
    // Stop at that real boundary so the test controls both GC ownership reads.
    let mut gc_connection = super::open_connection().unwrap();
    let transaction = gc_connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    assert_eq!(
        transaction
            .execute(
                "DELETE FROM transcription_history WHERE id = ?1",
                [source_id]
            )
            .unwrap(),
        1
    );
    transaction.commit().unwrap();

    let entered = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(AtomicBool::new(false));
    let hook_entered = Arc::clone(&entered);
    let hook_completed = Arc::clone(&completed);
    let hook_runtime = Arc::clone(&runtime);
    let hook_audio = audio_file.clone();
    gc_connection.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "history_audio_leases",
                ..
            }
        ) && !hook_entered.swap(true, Ordering::SeqCst)
        {
            let mut writer = super::open_connection().unwrap();
            writer.busy_timeout(Duration::ZERO).unwrap();
            match writer.execute_batch("BEGIN IMMEDIATE") {
                Ok(()) => {
                    writer.execute_batch("ROLLBACK").unwrap();
                    let mut replacement = stats_draft(502, "completed reprocess");
                    replacement.audio_file = Some(hook_audio.clone());
                    replacement.reprocessed_from_id = Some(source_id);
                    hook_runtime
                        .block_on(super::insert(replacement, 0))
                        .unwrap();
                    super::release_audio_lease_with_connection(&mut writer, &hook_audio).unwrap();
                    hook_completed.store(true, Ordering::SeqCst);
                }
                Err(rusqlite::Error::SqliteFailure(error, _))
                    if matches!(
                        error.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) => {}
                Err(error) => panic!("probe GC ownership transaction: {error}"),
            }
        }
        Authorization::Allow
    }));

    super::cleanup_audio_if_unreferenced_with_connection(&gc_connection, &audio_file);
    gc_connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(
        entered.load(Ordering::SeqCst),
        "GC must check active audio leases"
    );
    if !completed.load(Ordering::SeqCst) {
        // A GC transaction can legitimately serialize the reprocess behind it.
        let mut replacement = stats_draft(502, "completed reprocess");
        replacement.audio_file = Some(audio_file.clone());
        replacement.reprocessed_from_id = Some(source_id);
        runtime.block_on(super::insert(replacement, 0)).unwrap();
        runtime
            .block_on(super::release_audio_lease(audio_file.clone()))
            .unwrap();
    }

    let references: i64 = gc_connection
        .query_row(
            "SELECT COUNT(*) FROM transcription_history WHERE audio_file = ?1",
            [&audio_file],
            |row| row.get(0),
        )
        .unwrap();
    let leases: i64 = gc_connection
        .query_row(
            "SELECT COALESCE(SUM(lease_count), 0) FROM history_audio_leases WHERE audio_file = ?1",
            [&audio_file],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(references, 1, "completed reprocess owns the shared WAV");
    assert_eq!(
        leases, 0,
        "completed reprocess released its temporary lease"
    );
    assert!(
        super::safe_audio_path(&audio_file).unwrap().is_file(),
        "GC deleted WAV still referenced by the completed reprocess"
    );
    assert_eq!(
        runtime.block_on(super::read_audio(&audio_file)).unwrap(),
        wav
    );
    complete_stats_test();
}

#[test]
fn history_gc_finishes_unlink_before_releasing_its_database_write_lock() {
    if stats_test_in_child("history_gc_finishes_unlink_before_releasing_its_database_write_lock") {
        return;
    }
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let audio_file = runtime
        .block_on(super::save_audio(521, b"unowned WAV".to_vec()))
        .unwrap();
    let path = super::safe_audio_path(&audio_file).unwrap();
    let transaction_ended = Arc::new(AtomicBool::new(false));
    let early_end = Arc::new(AtomicBool::new(false));
    let missing_write_lock = Arc::new(AtomicBool::new(false));
    let hook_ended = Arc::clone(&transaction_ended);
    let hook_early_end = Arc::clone(&early_end);
    let hook_missing_lock = Arc::clone(&missing_write_lock);
    let hook_path = path.clone();
    let connection = super::open_connection().unwrap();
    connection.authorizer(Some(move |context: AuthContext<'_>| {
        // SQLite authorizes COMMIT/ROLLBACK before executing it. rusqlite 0.32
        // maps COMMIT to Unknown; every non-BEGIN transaction event ends it.
        if let AuthAction::Transaction { operation } = context.action {
            if operation != TransactionOperation::Begin {
                hook_ended.store(true, Ordering::SeqCst);
                hook_early_end.fetch_or(hook_path.exists(), Ordering::SeqCst);
                let writer = super::open_connection().unwrap();
                writer.busy_timeout(Duration::ZERO).unwrap();
                match writer.execute_batch("BEGIN IMMEDIATE") {
                    Ok(()) => {
                        hook_missing_lock.store(true, Ordering::SeqCst);
                        writer.execute_batch("ROLLBACK").unwrap();
                    }
                    Err(rusqlite::Error::SqliteFailure(error, _))
                        if matches!(
                            error.code,
                            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                        ) => {}
                    Err(error) => panic!("probe the GC write lock at transaction end: {error}"),
                }
            }
        }
        Authorization::Allow
    }));

    super::cleanup_audio_if_unreferenced_with_connection(&connection, &audio_file);
    connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(!path.exists(), "unowned WAV must be collected");
    assert!(
        transaction_ended.load(Ordering::SeqCst),
        "GC must hold a database transaction through unlink"
    );
    assert!(
        !early_end.load(Ordering::SeqCst),
        "GC released its transaction before unlinking the WAV"
    );
    assert!(
        !missing_write_lock.load(Ordering::SeqCst),
        "GC must still exclude database writers when unlink finishes"
    );
    let writer = super::open_connection().unwrap();
    writer.busy_timeout(Duration::ZERO).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
    complete_stats_test();
}

#[test]
fn history_gc_retains_audio_on_either_ownership_query_failure_and_retries() {
    if stats_test_in_child("history_gc_retains_audio_on_either_ownership_query_failure_and_retries")
    {
        return;
    }
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for (index, table) in ["transcription_history", "history_audio_leases"]
        .into_iter()
        .enumerate()
    {
        let wav = b"retain after ownership query error".to_vec();
        let audio_file = runtime
            .block_on(super::save_audio(531 + index as u64, wav.clone()))
            .unwrap();
        let denied = Arc::new(AtomicBool::new(false));
        let hook_denied = Arc::clone(&denied);
        let connection = super::open_connection().unwrap();
        connection.authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(context.action, AuthAction::Read { table_name, .. } if table_name == table)
            {
                hook_denied.store(true, Ordering::SeqCst);
                return Authorization::Deny;
            }
            Authorization::Allow
        }));
        super::cleanup_audio_if_unreferenced_with_connection(&connection, &audio_file);
        connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        assert!(
            denied.load(Ordering::SeqCst),
            "ownership query fault must be exercised"
        );
        assert_eq!(
            runtime.block_on(super::read_audio(&audio_file)).unwrap(),
            wav
        );

        // A failed check must also release its transaction for the next attempt.
        let writer = super::open_connection().unwrap();
        writer.busy_timeout(Duration::ZERO).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
        super::cleanup_audio_if_unreferenced_with_connection(&connection, &audio_file);
        assert!(!super::safe_audio_path(&audio_file).unwrap().exists());
    }
    complete_stats_test();
}

// Windows enforces deny-delete sharing on an open file. Unix permits unlinking
// open files, so this deterministic OS fault is intentionally Windows-only.
#[cfg(target_os = "windows")]
#[tokio::test]
async fn history_gc_unlink_failure_preserves_committed_delete_and_retries() {
    if stats_test_in_child("history_gc_unlink_failure_preserves_committed_delete_and_retries") {
        return;
    }
    use std::os::windows::fs::OpenOptionsExt;

    let wav = b"retryable unlink failure".to_vec();
    let audio_file = super::save_audio(541, wav.clone()).await.unwrap();
    let mut draft = stats_draft(541, "committed deletion");
    draft.audio_file = Some(audio_file.clone());
    let id = super::insert(draft, 0).await.unwrap();
    let path = super::safe_audio_path(&audio_file).unwrap();
    let locked_file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();

    assert!(
        super::delete(id).await.unwrap(),
        "retryable file GC cannot turn committed row deletion into failure"
    );
    assert!(super::get(id).await.unwrap().is_none());
    assert!(path.is_file(), "failed unlink must leave the WAV for retry");
    drop(locked_file);
    assert_eq!(super::read_audio(&audio_file).await.unwrap(), wav);
    super::cleanup_audio_if_unreferenced(audio_file)
        .await
        .unwrap();
    assert!(
        !path.exists(),
        "retry must collect the unreferenced WAV after the file lock is gone"
    );
    complete_stats_test();
}
