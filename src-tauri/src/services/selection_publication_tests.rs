use super::SelectionPublicationOwner;
use std::sync::{mpsc, Arc, Barrier, Mutex};
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
enum PublicationEffect {
    CacheScreenshots(u64),
    ShowWindow(u64),
}

fn record_publication(effects: &mut Vec<PublicationEffect>, worker: u64) {
    effects.push(PublicationEffect::CacheScreenshots(worker));
    effects.push(PublicationEffect::ShowWindow(worker));
}

#[test]
fn selection_publication_defaults_to_disabled_and_never_invokes_the_callback() {
    let owner = SelectionPublicationOwner::default();
    assert!(owner.snapshot().is_none());
    assert!(owner
        .publish_if_current(0, || panic!("disabled owner published"))
        .is_none());
}

#[test]
fn selection_publication_current_owner_runs_the_whole_callback_and_returns_its_value() {
    let owner = SelectionPublicationOwner::default();
    owner.set_enabled(true);
    let epoch = owner
        .snapshot()
        .expect("enabled listener has a publication owner");
    let mut effects = Vec::new();
    let returned = owner.publish_if_current(epoch, || {
        record_publication(&mut effects, 1);
        "selection payload".to_string()
    });
    assert_eq!(returned.as_deref(), Some("selection payload"));
    assert_eq!(
        effects,
        [
            PublicationEffect::CacheScreenshots(1),
            PublicationEffect::ShowWindow(1)
        ]
    );
}

#[test]
fn selection_publication_disable_retires_a_worker_before_either_effect() {
    let owner = SelectionPublicationOwner::default();
    owner.set_enabled(true);
    let pending_epoch = owner.snapshot().unwrap();
    owner.set_enabled(false);

    let mut effects = Vec::new();
    assert!(owner.snapshot().is_none());
    assert!(owner
        .publish_if_current(pending_epoch, || record_publication(&mut effects, 1))
        .is_none());
    assert!(
        effects.is_empty(),
        "retired capture must neither refill screenshots nor show a window"
    );
}

#[test]
fn selection_publication_reenable_accepts_new_capture_but_never_revives_old_capture() {
    let owner = SelectionPublicationOwner::default();
    owner.set_enabled(true);
    let old_epoch = owner.snapshot().unwrap();
    owner.set_enabled(false);
    owner.set_enabled(true);
    let new_epoch = owner.snapshot().unwrap();

    let mut effects = Vec::new();
    assert!(owner
        .publish_if_current(old_epoch, || record_publication(&mut effects, 1))
        .is_none());
    assert!(effects.is_empty());
    assert!(owner
        .publish_if_current(new_epoch, || record_publication(&mut effects, 2))
        .is_some());
    assert_eq!(
        effects,
        [
            PublicationEffect::CacheScreenshots(2),
            PublicationEffect::ShowWindow(2)
        ]
    );
}

#[test]
fn selection_publication_finishes_the_whole_callback_before_disable_can_complete() {
    let owner = Arc::new(SelectionPublicationOwner::default());
    owner.set_enabled(true);
    let epoch = owner.snapshot().unwrap();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (retired_tx, retired_rx) = mpsc::channel();
    let disable_start = Arc::new(Barrier::new(2));

    std::thread::scope(|scope| {
        let publishing_owner = Arc::clone(&owner);
        let publishing_effects = Arc::clone(&effects);
        let publication = scope.spawn(move || {
            let result = publishing_owner.publish_if_current(epoch, || {
                entered_tx.send(true).unwrap();
                release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                record_publication(&mut publishing_effects.lock().unwrap(), 1);
            });
            // A declaration-only scaffold must fail immediately, without the
            // watchdog being mistaken for semantic RED.
            if result.is_none() {
                let _ = entered_tx.send(false);
            }
            result
        });
        let callback_entered = entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        if !callback_entered {
            assert!(
                publication.join().unwrap().is_some(),
                "current owner must execute its publication callback"
            );
            return;
        }

        let disabling_owner = Arc::clone(&owner);
        let disabling_start = Arc::clone(&disable_start);
        let disabling = scope.spawn(move || {
            disabling_start.wait();
            disabling_owner.set_enabled(false);
            retired_tx.send(()).unwrap();
        });
        disable_start.wait();
        // Bounded concurrency observation, not a UI timing requirement. Source
        // correspondence must also confirm the same guard covers retirement
        // and the entire show_selection closure (including screenshot cache).
        let early_retirement = match retired_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(error) => panic!("disable worker disconnected: {error}"),
        };
        // Always release/join before asserting: a broken guard cannot strand a
        // correctly blocked publication or disable thread during test failure.
        release_tx.send(()).unwrap();
        assert!(publication.join().unwrap().is_some());
        disabling.join().unwrap();
        if !early_retirement {
            retired_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        assert!(!early_retirement, "disable completed while the publication callback could still mutate screenshots/show the window");
    });

    let effects = effects.lock().unwrap();
    assert_eq!(
        *effects,
        [
            PublicationEffect::CacheScreenshots(1),
            PublicationEffect::ShowWindow(1)
        ]
    );
    assert!(owner.snapshot().is_none());
    assert!(owner
        .publish_if_current(epoch, || panic!("callback ran after disable completed"))
        .is_none());
}

#[test]
fn selection_publication_hook_enabled_writes_use_owner_retiring_setter() {
    // Static correspondence guard only. Native hook installation failure and
    // GetMessageW exit cannot be driven through this owner-only test harness.
    // Check the actual compiled source, including those two Windows exit paths.
    let source = include_str!("selection_service.rs").replace("\r\n", "\n");
    let (before, setter_and_after) = source
        .split_once("pub fn set_selection_listener_enabled(")
        .expect("listener setter remains the shared state transition");
    let (setter, after) = setter_and_after
        .split_once("\n}\n")
        .expect("listener setter has a complete top-level body");
    let compact = |text: &str| text.split_whitespace().collect::<String>();
    let setter = compact(setter);
    let retirement = setter
        .find("selection_publication_owner().set_enabled(enabled)")
        .expect("listener setter must update the same production owner");
    let hook_store = setter
        .find("HOOK_ENABLED.store(enabled,")
        .expect("listener setter must update the native hook flag");
    assert!(
        retirement < hook_store,
        "listener retirement must precede the native flag update"
    );

    let outside_setter = compact(&format!("{before}{after}"));
    for mutation in [
        "store",
        "swap",
        "compare_exchange",
        "compare_exchange_weak",
        "fetch_and",
        "fetch_or",
        "fetch_xor",
        "fetch_nand",
        "fetch_not",
        "get_mut",
    ] {
        let direct_write = format!("HOOK_ENABLED.{mutation}(");
        assert_eq!(
            outside_setter.matches(&direct_write).count(),
            0,
            "native hook state mutation {mutation} bypasses owner retirement; route every hook failure/exit through set_selection_listener_enabled"
        );
    }
}

// Source correspondence reviewed separately from these owner API tests:
// - start_selection_listener and set_selection_listener_enabled share this owner;
// - run_selection_worker snapshots ownership before settle/grab/capture completion;
// - its publish_if_current closure contains the WHOLE show_selection call;
// - show_selection's screenshot cache, selection payload and visible window are
//   all mutated within that closure, not before it or after the guard drops;
// - commands::selection::set_selection_assistant_config retires first, then
//   cancels/hides before returning. Completion is the externally visible bound.
// These are bounded production-owner tests, not a native Windows UI replay.
