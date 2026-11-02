use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::test::{mock_context, noop_assets};
use tauri::Listener;

use super::{polish_text_with_overrides_detailed, PolishOverrides};
use crate::state::{user_profile::OpenaiAuthMode, AppState};

#[test]
fn missing_auth_is_reported_through_the_public_polish_operation() {
    // Every credential lookup uses an empty in-memory entry, never the OS keyring.
    keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    // The public API takes the native AppHandle. Build its runtime without any
    // windows, app setup, persisted profiles, or startup OAuth session loading.
    let app = tauri::Builder::default()
        .any_thread()
        .plugin(tauri_plugin_keyring::init())
        .build(mock_context(noop_assets()))
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = events.clone();
    app.listen("ai-polish-status", move |event| {
        received
            .lock()
            .unwrap()
            .push(serde_json::from_str(event.payload()).unwrap());
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let original = "请保留这段原始听写文字。";
    let mut observations = Vec::new();
    for provider in ["cerebras", "openai"] {
        let state = AppState::default();
        state.update_profile_mut(|profile| {
            profile.llm_provider.active = provider.to_owned();
            profile.llm_provider.openai_auth_mode = Some(OpenaiAuthMode::Oauth);
        });
        assert!(state.read_openai_codex_oauth_session().is_none());
        for (enabled, require_execution, emit_status) in [
            (true, false, true),
            (true, true, true),
            (true, false, false),
            (false, false, true),
        ] {
            events.lock().unwrap().clear();
            let session_id = 731;
            let result = runtime.block_on(polish_text_with_overrides_detailed(
                &state,
                original,
                app.handle(),
                session_id,
                PolishOverrides {
                    ai_polish_enabled: Some(enabled),
                    require_execution,
                    emit_status,
                    ..PolishOverrides::default()
                },
            ));
            observations.push((
                provider,
                enabled,
                require_execution,
                emit_status,
                result,
                events.lock().unwrap().clone(),
            ));
        }
    }
    for (provider, enabled, required, emit_status, result, events) in observations {
        if !enabled {
            let outcome = result.expect("disabled normal polish passes through");
            assert_eq!(outcome.text, original);
            assert!(!outcome.executed);
            assert!(events.is_empty());
            continue;
        }
        let error = result.unwrap_err();
        assert!(
            error.contains("API Key") || error.contains("登录"),
            "missing-auth error must explain how to restore authentication: {error}"
        );
        if emit_status {
            let errors: Vec<_> = events
                .iter()
                .filter(|event| event["status"] == "error")
                .collect();
            assert_eq!(errors.len(), 1, "{provider}, require_execution={required}");
            assert_eq!(errors[0]["original"], original);
            assert_eq!(errors[0]["polished"], original);
            assert_eq!(errors[0]["sessionId"], 731);
            assert_eq!(errors[0]["error"], error);
        } else {
            assert!(events.is_empty());
        }
    }
}
