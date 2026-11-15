use super::AppState;
use std::sync::{mpsc, Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

struct MockKeyStorage {
    disk: Mutex<String>,
    cache: Mutex<String>,
}

impl MockKeyStorage {
    fn new(disk: &str, cache: &str) -> Self {
        Self {
            disk: Mutex::new(disk.to_string()),
            cache: Mutex::new(cache.to_string()),
        }
    }

    fn set_disk(&self, value: &str) {
        *self.disk.lock().unwrap() = value.to_string();
    }

    fn read_disk(&self) -> String {
        self.disk.lock().unwrap().clone()
    }

    fn set_cache(&self, value: &str) {
        *self.cache.lock().unwrap() = value.to_string();
    }

    fn values(&self) -> (String, String) {
        (
            self.disk.lock().unwrap().clone(),
            self.cache.lock().unwrap().clone(),
        )
    }
}

#[test]
fn overlapping_writes_finish_with_matching_disk_and_cache() {
    let state = Arc::new(AppState::new());
    let storage = Arc::new(MockKeyStorage::new("old", "old"));
    let (owner_started_tx, owner_started_rx) = mpsc::channel();
    let (owner_release_tx, owner_release_rx) = mpsc::channel();

    let owner_state = Arc::clone(&state);
    let owner_storage = Arc::clone(&storage);
    let owner = thread::spawn(move || {
        owner_state.with_web_search_key_operation(|| {
            owner_storage.set_disk("first");
            owner_started_tx.send(()).unwrap();
            owner_release_rx.recv().unwrap();
            owner_storage.set_cache("first");
        });
    });
    owner_started_rx.recv().unwrap();

    let contender_barrier = Arc::new(Barrier::new(2));
    let (contender_ready_tx, contender_ready_rx) = mpsc::channel();
    let (contender_release_tx, contender_release_rx) = mpsc::channel();
    let (contender_done_tx, contender_done_rx) = mpsc::channel();
    let contender_state = Arc::clone(&state);
    let contender_storage = Arc::clone(&storage);
    let contender_barrier_for_thread = Arc::clone(&contender_barrier);
    let contender = thread::spawn(move || {
        contender_barrier_for_thread.wait();
        contender_state.with_web_search_key_operation(|| {
            contender_storage.set_disk("second");
            contender_storage.set_cache("second");
            contender_ready_tx.send(()).unwrap();
            contender_release_rx.recv().unwrap();
        });
        contender_done_tx.send(()).unwrap();
    });
    contender_barrier.wait();

    let overlapped = contender_ready_rx
        .recv_timeout(Duration::from_secs(1))
        .is_ok();
    if overlapped {
        contender_release_tx.send(()).unwrap();
        contender_done_rx.recv().unwrap();
        owner_release_tx.send(()).unwrap();
    } else {
        owner_release_tx.send(()).unwrap();
        contender_ready_rx.recv().unwrap();
        contender_release_tx.send(()).unwrap();
        contender_done_rx.recv().unwrap();
    }

    owner.join().unwrap();
    contender.join().unwrap();
    assert_eq!(
        storage.values(),
        ("second".to_string(), "second".to_string())
    );
}

#[test]
fn stale_lazy_fill_cannot_overwrite_completed_save() {
    let state = Arc::new(AppState::new());
    let storage = Arc::new(MockKeyStorage::new("old", ""));
    let (lazy_started_tx, lazy_started_rx) = mpsc::channel();
    let (lazy_release_tx, lazy_release_rx) = mpsc::channel();

    let lazy_state = Arc::clone(&state);
    let lazy_storage = Arc::clone(&storage);
    let lazy = thread::spawn(move || {
        lazy_state.with_web_search_key_operation(|| {
            let stale = lazy_storage.read_disk();
            lazy_started_tx.send(stale.clone()).unwrap();
            lazy_release_rx.recv().unwrap();
            lazy_storage.set_cache(&stale);
        });
    });
    assert_eq!(lazy_started_rx.recv().unwrap(), "old");

    let setter_barrier = Arc::new(Barrier::new(2));
    let (setter_ready_tx, setter_ready_rx) = mpsc::channel();
    let (setter_release_tx, setter_release_rx) = mpsc::channel();
    let (setter_done_tx, setter_done_rx) = mpsc::channel();
    let setter_state = Arc::clone(&state);
    let setter_storage = Arc::clone(&storage);
    let setter_barrier_for_thread = Arc::clone(&setter_barrier);
    let setter = thread::spawn(move || {
        setter_barrier_for_thread.wait();
        setter_state.with_web_search_key_operation(|| {
            setter_storage.set_disk("new");
            setter_storage.set_cache("new");
            setter_ready_tx.send(()).unwrap();
            setter_release_rx.recv().unwrap();
        });
        setter_done_tx.send(()).unwrap();
    });
    setter_barrier.wait();

    let overlapped = setter_ready_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    if overlapped {
        setter_release_tx.send(()).unwrap();
        setter_done_rx.recv().unwrap();
        lazy_release_tx.send(()).unwrap();
    } else {
        lazy_release_tx.send(()).unwrap();
        setter_ready_rx.recv().unwrap();
        setter_release_tx.send(()).unwrap();
        setter_done_rx.recv().unwrap();
    }

    lazy.join().unwrap();
    setter.join().unwrap();
    assert_eq!(storage.values(), ("new".to_string(), "new".to_string()));
}

#[test]
fn failed_operation_releases_ownership_without_publishing() {
    let state = Arc::new(AppState::new());
    let storage = Arc::new(MockKeyStorage::new("old", "old"));
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    let failing_state = Arc::clone(&state);
    let failing = thread::spawn(move || {
        let result = failing_state.with_web_search_key_operation(|| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Err::<(), _>("storage failed")
        });
        result_tx.send(result).unwrap();
    });
    started_rx.recv().unwrap();
    release_tx.send(()).unwrap();
    failing.join().unwrap();
    let result = result_rx.recv().unwrap();
    assert_eq!(result, Err("storage failed"));
    assert_eq!(storage.values(), ("old".to_string(), "old".to_string()));

    state.with_web_search_key_operation(|| {
        storage.set_disk("recovered");
        storage.set_cache("recovered");
    });
    assert_eq!(
        storage.values(),
        ("recovered".to_string(), "recovered".to_string())
    );
}
