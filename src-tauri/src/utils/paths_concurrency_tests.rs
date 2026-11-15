use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn with_isolated_config(name: &str, test: impl FnOnce()) {
    const CHILD: &str = "LIGHT_WHISPER_ENGINE_CONFIG_TEST_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        assert_eq!(
            get_data_dir(),
            &PathBuf::from(std::env::var_os("LIGHT_WHISPER_DATA_DIR").unwrap())
        );
        test();
        std::fs::write(get_data_dir().join("test-completed"), []).unwrap();
        return;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "light-whisper-engine-config-{}-{nonce}",
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
    std::fs::remove_dir_all(&directory).unwrap();
    let output = output.unwrap();
    assert!(
        output.status.success() && completed,
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initial_config() -> serde_json::Value {
    serde_json::json!({
        "engine": "qwen3-asr-0.6b",
        "alibaba_model": "old-model",
        "models_dir": "old-models",
        "gpu_idle_seconds": 45,
        "gpu_idle_timeout_seconds": 90,
        "unknown": {"preserve": ["future-field", 7]}
    })
}

#[test]
fn concurrent_config_writers_preserve_each_others_updates() {
    with_isolated_config(
        "concurrent_config_writers_preserve_each_others_updates",
        || {
            type Writer = fn() -> Result<(), std::io::Error>;
            let cases: [(&str, Writer, serde_json::Value); 5] = [
                (
                    "engine",
                    || write_engine_config("alibaba-asr"),
                    serde_json::json!({"engine": "alibaba-asr"}),
                ),
                (
                    "model",
                    || write_alibaba_model("new-model"),
                    serde_json::json!({"alibaba_model": "new-model"}),
                ),
                (
                    "gpu",
                    || write_gpu_idle_seconds(0, Some(75)),
                    serde_json::json!({"gpu_idle_seconds": 0, "gpu_idle_timeout_seconds": 75}),
                ),
                (
                    "models_dir",
                    || write_models_dir(Some("new-models")),
                    serde_json::json!({"models_dir": "new-models"}),
                ),
                (
                    "reset_models_dir",
                    || write_models_dir(None),
                    serde_json::json!({"models_dir": null}),
                ),
            ];
            let mut lost_updates = Vec::new();
            for (name, write_second, changes) in cases {
                let mut expected = initial_config();
                atomic_write(
                    &get_engine_config_path(),
                    &serde_json::to_vec(&expected).unwrap(),
                )
                .unwrap();
                let (read_tx, read_rx) = mpsc::channel();
                let (release_tx, release_rx) = mpsc::channel();
                let first = std::thread::spawn(move || {
                    update_engine_json(|config| {
                        config["first_update"] = serde_json::json!(name);
                        read_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    })
                });
                read_rx.recv_timeout(Duration::from_secs(5)).unwrap();

                let (started_tx, started_rx) = mpsc::channel();
                let (completed_tx, completed_rx) = mpsc::channel();
                let second = std::thread::spawn(move || {
                    started_tx.send(()).unwrap();
                    completed_tx.send(write_second()).unwrap();
                });
                started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

                let completed = completed_rx.recv_timeout(Duration::from_secs(1));
                release_tx.send(()).unwrap();
                first.join().unwrap().unwrap();
                match completed {
                    Ok(result) => result.unwrap(),
                    Err(mpsc::RecvTimeoutError::Timeout) => completed_rx
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap()
                        .unwrap(),
                    Err(error) => panic!("second configuration writer disconnected: {error}"),
                }
                second.join().unwrap();

                expected["first_update"] = serde_json::json!(name);
                for (field, value) in changes.as_object().unwrap() {
                    if value.is_null() {
                        expected.as_object_mut().unwrap().remove(field);
                    } else {
                        expected[field] = value.clone();
                    }
                }
                let actual = read_engine_json();
                if actual != expected {
                    lost_updates.push(format!("{name}: expected {expected}, got {actual}"));
                }
            }
            assert!(lost_updates.is_empty(), "{}", lost_updates.join("\n"));
        },
    );
}

#[test]
fn failed_config_write_does_not_block_later_updates() {
    with_isolated_config("failed_config_write_does_not_block_later_updates", || {
        let config_path = get_engine_config_path();
        std::fs::create_dir(&config_path).unwrap();
        let marker = config_path.join("preserved-marker");
        std::fs::write(&marker, b"preserve me").unwrap();
        assert!(write_gpu_idle_seconds(60, None).is_err());
        assert_eq!(std::fs::read(&marker).unwrap(), b"preserve me");
        std::fs::remove_file(marker).unwrap();
        std::fs::remove_dir(&config_path).unwrap();

        let (completed_tx, completed_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            completed_tx
                .send(write_engine_config("alibaba-asr"))
                .unwrap();
        });
        completed_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("failed writes must release the configuration transaction")
            .unwrap();
        writer.join().unwrap();
        assert_eq!(read_engine_config(), "alibaba-asr");
    });
}
