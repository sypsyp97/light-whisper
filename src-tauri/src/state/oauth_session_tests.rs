use super::OAuthSessionState;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;

#[test]
fn account_removal_cancels_pending_login_without_signing_out_another_account() {
    let state = OAuthSessionState::default();
    state.restore(Some("active-account".to_string()));
    let login = state.begin_login();
    let token = login.token();
    let mut removed = false;
    state
        .invalidate_matching(
            |s| s == "removed-account",
            |active| {
                assert!(!active);
                removed = true;
                Ok(())
            },
        )
        .unwrap();
    assert!(removed);
    assert_eq!(state.read().as_deref(), Some("active-account"));
    assert_eq!(
        state.commit(token, "removed-account".to_string(), |_| panic!(
            "late login must not persist"
        )),
        Ok(false)
    );
    assert!(state.snapshot_for_refresh().is_some());
}

#[test]
fn failed_account_removal_keeps_the_current_session() {
    let state = OAuthSessionState::default();
    state.restore(Some("active-account".to_string()));
    assert!(state
        .invalidate_matching(|s| s == "other-account", |_| Err("storage failure".into()))
        .is_err());
    assert_eq!(state.read().as_deref(), Some("active-account"));
}

#[test]
fn active_account_removal_invalidates_refresh_even_if_storage_fails() {
    let state = OAuthSessionState::default();
    state.restore(Some("removed-account".to_string()));
    let (refresh, _) = state.snapshot_for_refresh().unwrap();
    assert!(state
        .invalidate_matching(
            |s| s == "removed-account",
            |active| {
                assert!(active);
                Err("storage failure".into())
            }
        )
        .is_err());
    assert!(state.read().is_none());
    assert_eq!(
        state.commit(refresh, "removed-account".to_string(), |_| panic!(
            "late refresh must not persist"
        )),
        Ok(false)
    );
}

#[test]
fn removal_checks_the_current_account_after_a_concurrent_switch() {
    let state = OAuthSessionState::default();
    state.restore(Some("old-account".to_string()));
    let login = state.begin_login();
    assert_eq!(
        state.commit(login.token(), "new-account".to_string(), |_| Ok(())),
        Ok(true)
    );
    assert!(state
        .invalidate_matching(
            |s| s == "old-account",
            |active| {
                assert!(!active);
                Ok(())
            }
        )
        .unwrap()
        .is_none());
    assert_eq!(state.read().as_deref(), Some("new-account"));
}

#[test]
fn pending_registration_progress_cannot_recreate_a_removed_or_signed_out_record() {
    let state = OAuthSessionState::<String>::default();
    let login = state.begin_login();
    let token = login.token();
    assert_eq!(state.record_login_progress(token, || Ok(())), Ok(true));
    assert!(state.read().is_none()); // No credentials published before identity verification.
    state.invalidate_matching(|_| false, |_| Ok(())).unwrap();
    assert_eq!(
        state.record_login_progress(token, || panic!("removed registration must stay removed")),
        Ok(false)
    );
}

fn with_session(session: &str) -> OAuthSessionState<String> {
    let state = OAuthSessionState::default();
    state.restore(Some(session.to_owned()));
    state
}

struct NoopWaker;

impl Wake for NoopWaker {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn default_is_empty_and_restore_replaces_session() {
    let state = OAuthSessionState::<String>::default();

    assert_eq!(state.read(), None);

    state.restore(Some("first".to_owned()));
    assert_eq!(state.read().as_deref(), Some("first"));

    state.restore(Some("second".to_owned()));
    assert_eq!(state.read().as_deref(), Some("second"));

    state.restore(None);
    assert_eq!(state.read(), None);
}

#[test]
fn pending_login_hides_prior_session_until_operation_drops() {
    let state = with_session("prior");

    assert!(state.snapshot_for_refresh().is_some());

    let login = state.begin_login();
    assert_eq!(state.snapshot_for_refresh(), None);

    drop(login);

    let snapshot = state.snapshot_for_refresh();
    assert_eq!(
        snapshot.map(|(_, session)| session),
        Some("prior".to_owned())
    );
}

#[test]
fn dropping_old_login_does_not_clear_new_pending_login() {
    let state = with_session("prior");
    let old_login = state.begin_login();
    let new_login = state.begin_login();

    drop(old_login);
    assert_eq!(state.snapshot_for_refresh(), None);

    drop(new_login);
    assert_eq!(
        state.snapshot_for_refresh().map(|(_, session)| session),
        Some("prior".to_owned())
    );
}

#[test]
fn logout_invalidates_old_login_before_late_success() {
    let state = OAuthSessionState::<String>::default();
    let login = state.begin_login();
    let token = login.token();
    let clear_calls = Arc::new(AtomicUsize::new(0));

    let clear_calls_for_logout = Arc::clone(&clear_calls);
    assert_eq!(
        state.invalidate(None, move || {
            clear_calls_for_logout.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(true)
    );

    let persist_calls = Arc::new(AtomicUsize::new(0));
    let persist_calls_for_commit = Arc::clone(&persist_calls);
    assert_eq!(
        state.commit(token, "old-login".to_owned(), move |_| {
            persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    assert_eq!(clear_calls.load(Ordering::SeqCst), 1);
    assert_eq!(persist_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.read(), None);

    drop(login);
}

#[test]
fn logout_invalidates_old_refresh_before_late_success() {
    let state = with_session("old-refresh");
    let (token, _) = state.snapshot_for_refresh().expect("authenticated");

    assert_eq!(state.invalidate(None, || Ok(())), Ok(true));

    let persist_calls = Arc::new(AtomicUsize::new(0));
    let persist_calls_for_commit = Arc::clone(&persist_calls);
    assert_eq!(
        state.commit(token, "old-refresh-result".to_owned(), move |_| {
            persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    assert_eq!(persist_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.read(), None);
}

#[test]
fn current_refresh_commit_persists_and_updates_memory() {
    let state = with_session("old-refresh");
    let (token, _) = state.snapshot_for_refresh().expect("authenticated");
    let persist_calls = Arc::new(AtomicUsize::new(0));
    let persisted = Arc::new(Mutex::new(None::<String>));

    let persist_calls_for_commit = Arc::clone(&persist_calls);
    let persisted_for_commit = Arc::clone(&persisted);
    assert_eq!(
        state.commit(token, "new-refresh".to_owned(), move |session| {
            persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            *persisted_for_commit.lock().expect("persisted lock") = Some(session.clone());
            Ok(())
        }),
        Ok(true)
    );

    assert_eq!(persist_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        persisted.lock().expect("persisted lock").as_deref(),
        Some("new-refresh")
    );
    assert_eq!(state.read().as_deref(), Some("new-refresh"));
}

#[test]
fn current_token_invalidate_invokes_clear_and_clears_memory() {
    let state = with_session("current");
    let (token, _) = state.snapshot_for_refresh().expect("authenticated");
    let clear_calls = Arc::new(AtomicUsize::new(0));

    let clear_calls_for_invalidate = Arc::clone(&clear_calls);
    assert_eq!(
        state.invalidate(Some(token), move || {
            clear_calls_for_invalidate.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(true)
    );

    assert_eq!(clear_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.read(), None);
}

#[test]
fn stale_login_commit_cannot_overwrite_new_login() {
    let state = OAuthSessionState::<String>::default();
    let old_login = state.begin_login();
    let old_token = old_login.token();
    let new_login = state.begin_login();
    let new_token = new_login.token();

    assert_eq!(
        state.commit(new_token, "new-login".to_owned(), |_| Ok(())),
        Ok(true)
    );
    drop(new_login);

    let stale_persist_calls = Arc::new(AtomicUsize::new(0));
    let stale_persist_calls_for_commit = Arc::clone(&stale_persist_calls);
    assert_eq!(
        state.commit(old_token, "old-login".to_owned(), move |_| {
            stale_persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    assert_eq!(stale_persist_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.read().as_deref(), Some("new-login"));

    drop(old_login);
}

#[test]
fn stale_fatal_refresh_cannot_clear_new_account() {
    let state = with_session("old-account");
    let (old_refresh, old_snapshot) = state.snapshot_for_refresh().expect("authenticated");
    assert_eq!(old_snapshot, "old-account");

    let new_login = state.begin_login();
    let new_token = new_login.token();
    assert_eq!(
        state.commit(new_token, "new-account".to_owned(), |_| Ok(())),
        Ok(true)
    );
    drop(new_login);

    let clear_calls = Arc::new(AtomicUsize::new(0));
    let clear_calls_for_invalidate = Arc::clone(&clear_calls);
    assert_eq!(
        state.invalidate(Some(old_refresh), move || {
            clear_calls_for_invalidate.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    assert_eq!(clear_calls.load(Ordering::SeqCst), 0);
    assert_eq!(state.read().as_deref(), Some("new-account"));
}

#[test]
fn failed_commit_leaves_current_memory_intact() {
    let state = with_session("current");
    let login = state.begin_login();
    let token = login.token();
    let persist_calls = Arc::new(AtomicUsize::new(0));

    let persist_calls_for_commit = Arc::clone(&persist_calls);
    let result = state.commit(token, "replacement".to_owned(), move |_| {
        persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
        Err("storage unavailable".to_owned())
    });

    assert!(result.is_err());
    assert_eq!(persist_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.read().as_deref(), Some("current"));

    drop(login);
    assert_eq!(
        state.snapshot_for_refresh().map(|(_, session)| session),
        Some("current".to_owned())
    );
}

#[test]
fn logout_clears_memory_and_invalidates_operations_when_delete_fails() {
    let state = with_session("current");
    let (refresh_token, _) = state.snapshot_for_refresh().expect("authenticated");
    let login = state.begin_login();
    let login_token = login.token();
    let clear_calls = Arc::new(AtomicUsize::new(0));

    let clear_calls_for_logout = Arc::clone(&clear_calls);
    let result = state.invalidate(None, move || {
        clear_calls_for_logout.fetch_add(1, Ordering::SeqCst);
        Err("storage deletion unavailable".to_owned())
    });

    assert!(result.is_err());
    assert_eq!(clear_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.read(), None);

    let stale_refresh_persist_calls = Arc::new(AtomicUsize::new(0));
    let stale_refresh_persist_calls_for_commit = Arc::clone(&stale_refresh_persist_calls);
    assert_eq!(
        state.commit(refresh_token, "stale-refresh".to_owned(), move |_| {
            stale_refresh_persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    let stale_login_persist_calls = Arc::new(AtomicUsize::new(0));
    let stale_login_persist_calls_for_commit = Arc::clone(&stale_login_persist_calls);
    assert_eq!(
        state.commit(login_token, "stale-login".to_owned(), move |_| {
            stale_login_persist_calls_for_commit.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Ok(false)
    );

    assert_eq!(stale_refresh_persist_calls.load(Ordering::SeqCst), 0);
    assert_eq!(stale_login_persist_calls.load(Ordering::SeqCst), 0);

    drop(login);
}

#[test]
fn commit_and_logout_serialize_storage_side_effects() {
    let state = Arc::new(with_session("old"));
    let operation = state.snapshot_for_refresh().expect("authenticated").0;
    let rendezvous = Arc::new(Barrier::new(2));
    let commit_in_progress = Arc::new(AtomicBool::new(false));
    let clear_saw_commit_in_progress = Arc::new(AtomicBool::new(false));
    let commit_holds_inner_lock = Arc::new(AtomicBool::new(false));
    let clear_holds_inner_lock = Arc::new(AtomicBool::new(false));
    let events = Arc::new(Mutex::new(Vec::<&'static str>::new()));

    let commit_thread = {
        let state = Arc::clone(&state);
        let rendezvous = Arc::clone(&rendezvous);
        let commit_in_progress = Arc::clone(&commit_in_progress);
        let commit_holds_inner_lock = Arc::clone(&commit_holds_inner_lock);
        let state_for_callback = Arc::clone(&state);
        let events = Arc::clone(&events);
        thread::spawn(move || {
            state.commit(operation, "refreshed".to_owned(), move |_| {
                commit_holds_inner_lock.store(
                    state_for_callback.inner.try_lock().is_none(),
                    Ordering::SeqCst,
                );
                commit_in_progress.store(true, Ordering::SeqCst);
                events.lock().expect("event lock").push("commit-start");
                rendezvous.wait();
                commit_in_progress.store(false, Ordering::SeqCst);
                events.lock().expect("event lock").push("commit-end");
                Ok(())
            })
        })
    };

    let logout_thread = {
        let state = Arc::clone(&state);
        let rendezvous = Arc::clone(&rendezvous);
        let commit_in_progress = Arc::clone(&commit_in_progress);
        let clear_saw_commit_in_progress = Arc::clone(&clear_saw_commit_in_progress);
        let clear_holds_inner_lock = Arc::clone(&clear_holds_inner_lock);
        let state_for_callback = Arc::clone(&state);
        let events = Arc::clone(&events);
        thread::spawn(move || {
            rendezvous.wait();
            state.invalidate(None, move || {
                clear_holds_inner_lock.store(
                    state_for_callback.inner.try_lock().is_none(),
                    Ordering::SeqCst,
                );
                if commit_in_progress.load(Ordering::SeqCst) {
                    clear_saw_commit_in_progress.store(true, Ordering::SeqCst);
                }
                events.lock().expect("event lock").push("logout");
                Ok(())
            })
        })
    };

    assert_eq!(commit_thread.join().expect("commit thread"), Ok(true));
    assert_eq!(logout_thread.join().expect("logout thread"), Ok(true));
    assert!(commit_holds_inner_lock.load(Ordering::SeqCst));
    assert!(clear_holds_inner_lock.load(Ordering::SeqCst));
    assert!(!clear_saw_commit_in_progress.load(Ordering::SeqCst));
    assert_eq!(state.read(), None);
    assert_eq!(
        *events.lock().expect("event lock"),
        vec!["commit-start", "commit-end", "logout"]
    );
}

#[tokio::test]
async fn refresh_lock_keeps_second_future_pending_until_first_releases() {
    let state = with_session("current");
    let first_guard = state.lock_refresh().await;
    let mut second_future = Box::pin(state.lock_refresh());
    let waker = Waker::from(Arc::new(NoopWaker));
    let mut context = Context::from_waker(&waker);

    assert!(matches!(
        second_future.as_mut().poll(&mut context),
        Poll::Pending
    ));

    drop(first_guard);

    let second_guard = match second_future.as_mut().poll(&mut context) {
        Poll::Ready(guard) => guard,
        Poll::Pending => panic!("second refresh remained pending after release"),
    };
    drop(second_guard);
}

#[tokio::test]
async fn account_switch_waits_for_rotated_token_and_failed_login_keeps_it() {
    let state = with_session("old-refresh-token");
    let refresh_guard = state.lock_refresh().await;
    let (operation, _) = state.snapshot_for_refresh().unwrap();
    let mut switch = Box::pin(state.begin_login_after_refresh());
    let waker = Waker::from(Arc::new(NoopWaker));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(switch.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(
        state.commit(operation, "rotated-refresh-token".into(), |_| Ok(())),
        Ok(true)
    );
    drop(refresh_guard);
    let login = switch.await;
    assert!(state.snapshot_for_refresh().is_none());
    drop(login); // Browser cancellation or a failed exchange.
    assert_eq!(
        state.snapshot_for_refresh().unwrap().1,
        "rotated-refresh-token"
    );
}

#[tokio::test]
async fn refresh_guard_does_not_block_session_state_access() {
    let state = with_session("current");
    let guard = state.lock_refresh().await;

    assert_eq!(state.read().as_deref(), Some("current"));

    let login = state.begin_login();
    assert_eq!(state.snapshot_for_refresh(), None);
    drop(login);
    assert!(state.snapshot_for_refresh().is_some());

    drop(guard);
}

#[derive(Default)]
struct LegacyUnversionedSession {
    session: Option<String>,
    next_login: u64,
}

impl LegacyUnversionedSession {
    fn begin_login(&mut self) -> u64 {
        self.next_login += 1;
        self.next_login
    }

    fn logout(&mut self) {
        self.session = None;
    }

    fn finish_login(&mut self, _login: u64, session: &str) {
        self.session = Some(session.to_owned());
    }
}

// Evidence-only abstraction of the audited pre-coordinator service behavior.
// It models the unversioned late publication; it does not call production code.
#[test]
#[ignore = "intentional legacy RED; run explicitly for audit evidence"]
fn legacy_unversioned_late_login_publication_violates_logout_contract() {
    let mut legacy = LegacyUnversionedSession::default();
    let old_login = legacy.begin_login();

    legacy.logout();
    legacy.finish_login(old_login, "late-old-session");

    assert_eq!(
        legacy.session, None,
        "legacy late publication resurrected a session after logout"
    );
}
