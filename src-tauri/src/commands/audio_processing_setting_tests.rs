use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

use crate::state::AppState;
use crate::utils::paths;

#[tokio::test]
async fn idle_changes_during_asr_startup_persist_latest_choice_without_ending_startup() {
    // Isolate DATA_DIR's OnceLock without changing the test runner's environment.
    if std::env::var_os("LIGHT_WHISPER_AUDIO_SETTING_TEST_CHILD").is_none() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "light-whisper-audio-setting-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::audio_processing_setting_tests::idle_changes_during_asr_startup_persist_latest_choice_without_ending_startup",
                "--nocapture",
            ])
            .env("LIGHT_WHISPER_DATA_DIR", &dir)
            .env("LIGHT_WHISPER_AUDIO_SETTING_TEST_CHILD", "1")
            .output()
            .unwrap();
        let config = dir.join("engine.json");
        if config.exists() {
            std::fs::remove_file(config).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let app = mock_builder()
        .manage(AppState::default())
        .build(mock_context(noop_assets()))
        .unwrap();
    let state = app.state::<AppState>();
    assert!(state.recording.snapshot().is_none());
    assert!(state.engine.try_begin_funasr_start(1));
    assert!(!paths::read_audio_processing_enabled());

    for enabled in [true, false] {
        let chosen = super::funasr::set_audio_processing_enabled(app.state(), enabled)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "idle audio processing change to {enabled} during ASR startup failed: {error}"
                )
            });
        assert_eq!(chosen, enabled);
        assert_eq!(paths::read_audio_processing_enabled(), enabled);
        let config: serde_json::Value =
            serde_json::from_slice(&std::fs::read(paths::get_engine_config_path()).unwrap())
                .unwrap();
        assert_eq!(config["audio_processing_enabled"].as_bool(), Some(enabled));
        assert!(state.engine.owns_funasr_start(1));
        assert!(!state.is_funasr_ready());
        assert!(state.engine.funasr_process.lock().await.is_none());
    }
}
