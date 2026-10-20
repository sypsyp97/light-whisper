use super::*;
use std::future::{poll_fn, Future};
use std::path::PathBuf;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::{oneshot, watch};

type WriteGate = (oneshot::Sender<()>, oneshot::Receiver<()>);

fn write_gate_slot() -> &'static parking_lot::Mutex<Option<WriteGate>> {
    static GATE: OnceLock<parking_lot::Mutex<Option<WriteGate>>> = OnceLock::new();
    GATE.get_or_init(|| parking_lot::Mutex::new(None))
}

pub(super) async fn before_profile_write() {
    let gate = write_gate_slot().lock().take();
    if let Some((entered, release)) = gate {
        entered.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), release)
            .await
            .expect("test must release the profile write")
            .unwrap();
    }
}

fn pause_next_write() -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    assert!(write_gate_slot()
        .lock()
        .replace((entered_tx, release_rx))
        .is_none());
    (entered_rx, release_tx)
}

fn with_isolated_profile(name: &str, test: impl Future<Output = ()>) {
    const CHILD: &str = "LIGHT_WHISPER_PROFILE_SAVE_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        assert_eq!(
            paths::get_data_dir(),
            &PathBuf::from(std::env::var_os("LIGHT_WHISPER_DATA_DIR").unwrap())
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        tauri::async_runtime::set(runtime.handle().clone());
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), test)
                .await
                .expect("profile save test timed out");
        });
        std::fs::write(paths::get_data_dir().join("test-completed"), []).unwrap();
        return;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "light-whisper-profile-save-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let module = module_path!().split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("{module}::{name}"),
            "--nocapture",
            "--test-threads=1",
        ])
        .env("LIGHT_WHISPER_DATA_DIR", &directory)
        .env(CHILD, name)
        .output();
    let completed = directory.join("test-completed").is_file();
    assert!(directory
        .canonicalize()
        .unwrap()
        .starts_with(std::env::temp_dir().canonicalize().unwrap()));
    std::fs::remove_dir_all(&directory).unwrap();
    let output = output.unwrap();
    assert!(
        output.status.success() && completed,
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn status_json() -> serde_json::Value {
    serde_json::to_value(profile_save_status()).unwrap()
}

async fn wait_for_status(
    receiver: &mut watch::Receiver<ProfileSaveStatus>,
    expected: &str,
) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = serde_json::to_value(receiver.borrow_and_update().clone()).unwrap();
            if status["state"] == expected {
                return status;
            }
            receiver.changed().await.unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "profile save status never became {expected}: {}",
            status_json()
        )
    })
}

fn pending_prompt() -> Option<String> {
    pending_profile_save_slot()
        .lock()
        .as_ref()
        .and_then(|pending| pending.profile.custom_prompt.clone())
}

#[test]
fn immediate_failure_retains_unsaved_profile() {
    with_isolated_profile("immediate_failure_retains_unsaved_profile", async {
        let path = paths::get_data_dir().join("user_profile.json");
        std::fs::create_dir(&path).unwrap();
        let state = AppState::new();
        update_profile_and_schedule(&state, |profile| {
            profile.custom_prompt = Some("unsaved change".into());
        });

        assert!(save_profile_async(&state).await.is_err());

        assert_eq!(
            pending_prompt().as_deref(),
            Some("unsaved change"),
            "a failed immediate write must retain the unsaved snapshot for recovery"
        );
        assert_eq!(
            state.snapshot_profile().custom_prompt.as_deref(),
            Some("unsaved change")
        );
        assert_eq!(status_json()["state"], "error");
        assert!(path.is_dir());
    });
}

#[test]
fn background_failure_is_observable_and_retry_saves_current_state() {
    with_isolated_profile(
        "background_failure_is_observable_and_retry_saves_current_state",
        async {
            let path = paths::get_data_dir().join("user_profile.json");
            std::fs::create_dir(&path).unwrap();
            let state = Arc::new(AppState::new());
            let mut statuses = subscribe_profile_save_status();
            let initial = status_json();
            update_profile_and_schedule(&state, |profile| {
                profile.custom_prompt = Some("failed background snapshot".into());
            });
            assert_eq!(status_json()["state"], "saving");
            let failure = wait_for_status(&mut statuses, "error").await;
            assert!(failure["revision"].as_u64().unwrap() > initial["revision"].as_u64().unwrap());
            assert_eq!(
                pending_prompt().as_deref(),
                Some("failed background snapshot")
            );

            // A retry must read AppState again, including changes made after the failure.
            state.update_profile_mut(|profile| {
                profile.custom_prompt = Some("latest state at retry".into());
            });
            std::fs::remove_dir(&path).unwrap();
            let (entered, release) = pause_next_write();
            let retry_state = state.clone();
            let retry = tokio::spawn(async move { save_profile_async(&retry_state).await });
            entered.await.unwrap();
            let saving = status_json();
            assert_eq!(saving["state"], "saving");
            assert!(saving["revision"].as_u64().unwrap() > failure["revision"].as_u64().unwrap());
            release.send(()).unwrap();
            retry.await.unwrap().unwrap();

            let saved = wait_for_status(&mut statuses, "saved").await;
            assert!(saved["revision"].as_u64().unwrap() > saving["revision"].as_u64().unwrap());
            assert!(pending_profile_save_slot().lock().is_none());
            let persisted: UserProfile =
                serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(
                persisted.custom_prompt.as_deref(),
                Some("latest state at retry")
            );
        },
    );
}

async fn older_completion_preserves_newer_snapshot(fail_older: bool) {
    let path = paths::get_data_dir().join("user_profile.json");
    if fail_older {
        std::fs::create_dir(&path).unwrap();
    }
    let state = Arc::new(AppState::new());
    state.update_profile_mut(|profile| {
        profile.custom_prompt = Some("older write".into());
    });
    let (entered, release) = pause_next_write();
    let older_state = state.clone();
    let older = tokio::spawn(async move { save_profile_async(&older_state).await });
    entered.await.unwrap();

    update_profile_and_schedule(&state, |profile| {
        profile.custom_prompt = Some("newest pending change".into());
    });
    let newest_status = status_json();
    assert_eq!(newest_status["state"], "saving");

    // Queue ahead of the newer debounce task on this single-thread runtime.
    // Holding this guard after the old write finishes isolates its completion.
    let mut next_write_guard = Box::pin(profile_save_lock().lock());
    poll_fn(|cx| {
        assert!(next_write_guard.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    release.send(()).unwrap();
    let guard = next_write_guard.await;
    assert_eq!(older.await.unwrap().is_err(), fail_older);
    assert_eq!(pending_prompt().as_deref(), Some("newest pending change"));
    assert_eq!(
        status_json(),
        newest_status,
        "an older completion must not publish over the newer save status"
    );

    if fail_older {
        std::fs::remove_dir(&path).unwrap();
    }
    let mut statuses = subscribe_profile_save_status();
    drop(guard);
    wait_for_status(&mut statuses, "saved").await;
    let persisted: UserProfile = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        persisted.custom_prompt.as_deref(),
        Some("newest pending change")
    );
    assert!(pending_profile_save_slot().lock().is_none());
}

#[test]
fn older_success_cannot_clear_newer_pending_save_or_status() {
    with_isolated_profile(
        "older_success_cannot_clear_newer_pending_save_or_status",
        older_completion_preserves_newer_snapshot(false),
    );
}

#[test]
fn older_failure_cannot_replace_newer_pending_save_or_status() {
    with_isolated_profile(
        "older_failure_cannot_replace_newer_pending_save_or_status",
        older_completion_preserves_newer_snapshot(true),
    );
}
