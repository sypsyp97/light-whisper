// Included in history_service::tests to reuse its isolated public-API fixtures.

#[tokio::test]
async fn history_timing_updates_only_the_inserted_row_when_session_ids_repeat() {
    if stats_test_in_child("history_timing_updates_only_the_inserted_row_when_session_ids_repeat") {
        return;
    }
    let mut first = stats_draft(41, "  First launch final\n");
    first.original_text = "First launch raw".into();
    first.source_text = Some("Original selected text".into());
    first.asr_ms = Some(40);
    first.polish_ms = Some(60);
    first.total_ms = Some(140);
    let first_id = super::insert(first, 0).await.unwrap();
    let mut second = stats_draft(41, "Second launch final");
    second.total_ms = Some(180);
    let second_id = super::insert(second, 0).await.unwrap();
    assert_ne!(first_id, second_id);
    assert_eq!(
        super::stats().await.unwrap().total_latency.p95_ms,
        Some(180)
    );

    assert!(super::update_total_ms(first_id, 460).await.unwrap());

    let first = super::get(first_id).await.unwrap().unwrap().record;
    let second = super::get(second_id).await.unwrap().unwrap().record;
    assert_eq!(first.total_ms, Some(460));
    assert_eq!(first.text, "  First launch final\n");
    assert_eq!(first.original_text, "First launch raw");
    assert_eq!(first.source_text.as_deref(), Some("Original selected text"));
    assert_eq!(first.asr_ms, Some(40));
    assert_eq!(first.polish_ms, Some(60));
    assert_eq!(second.total_ms, Some(180));
    assert_eq!(second.text, "Second launch final");
    assert_eq!(
        super::stats().await.unwrap().total_latency.p95_ms,
        Some(460)
    );
    complete_stats_test();
}

#[tokio::test]
async fn history_timing_does_not_resurrect_deleted_or_missing_rows() {
    if stats_test_in_child("history_timing_does_not_resurrect_deleted_or_missing_rows") {
        return;
    }
    let deleted = super::insert(stats_draft(41, "Deleted final"), 0)
        .await
        .unwrap();
    let mut retained = stats_draft(41, "Retained final");
    retained.total_ms = Some(180);
    let retained = super::insert(retained, 0).await.unwrap();
    assert!(super::delete(deleted).await.unwrap());

    assert!(!super::update_total_ms(deleted, 460).await.unwrap());
    assert!(!super::update_total_ms(i64::MAX, 460).await.unwrap());

    assert!(super::get(deleted).await.unwrap().is_none());
    assert!(super::get(i64::MAX).await.unwrap().is_none());
    let retained = super::get(retained).await.unwrap().unwrap().record;
    assert_eq!(retained.text, "Retained final");
    assert_eq!(retained.total_ms, Some(180));
    assert_eq!(super::stats().await.unwrap().total, 1);
    complete_stats_test();
}
