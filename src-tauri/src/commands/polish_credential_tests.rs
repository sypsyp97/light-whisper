use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi, CredentialPersistence};
use keyring::mock::MockCredential;
use serde_json::{json, Value};
use tauri::test::{
    get_ipc_response, mock_builder, mock_context, noop_assets, MockRuntime, INVOKE_KEY,
};
use tauri::Manager;

use crate::services::llm_provider::keyring_user_for_provider;
use crate::state::AppState;

type MockEntries = HashMap<(String, String), Arc<MockCredential>>;

#[derive(Clone, Default)]
struct SharedMockBuilder {
    entries: Arc<Mutex<MockEntries>>,
}

impl SharedMockBuilder {
    fn credential(&self, service: &str, user: &str) -> Arc<MockCredential> {
        self.entries
            .lock()
            .unwrap()
            .entry((service.to_owned(), user.to_owned()))
            .or_default()
            .clone()
    }

    fn provider_credential(&self, provider: &str) -> Arc<MockCredential> {
        self.credential("light-whisper", &keyring_user_for_provider(provider))
    }
}

struct SharedMockCredential(Arc<MockCredential>);

impl CredentialApi for SharedMockCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        self.0.set_secret(secret)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.get_secret()
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        self.0.delete_credential()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CredentialBuilderApi for SharedMockBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(SharedMockCredential(
            self.credential(service, user),
        )))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::ProcessOnly
    }
}

fn invoke(
    window: &tauri::WebviewWindow<MockRuntime>,
    command: &str,
    body: Value,
) -> Result<tauri::ipc::InvokeResponseBody, Value> {
    get_ipc_response(
        window,
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        },
    )
}

#[test]
fn polish_credential_ipc_keeps_explicit_provider_slots_and_legacy_fallback() {
    let store = SharedMockBuilder::default();

    keyring::set_default_credential_builder(Box::new(store.clone()));
    store
        .provider_credential("cerebras")
        .set_password("saved-cerebras-key")
        .unwrap();
    store
        .provider_credential("openai")
        .set_password("saved-openai-key")
        .unwrap();

    let state = AppState::default();
    state.update_profile_mut(|profile| profile.llm_provider.active = "openai".to_owned());
    let enabled_before = state
        .profile
        .ai_polish_enabled
        .load(std::sync::atomic::Ordering::Acquire);
    let app = mock_builder()
        .plugin(tauri_plugin_keyring::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            super::ai_polish::get_ai_polish_api_key,
            super::ai_polish::set_ai_polish_api_key,
            super::ai_polish::set_ai_polish_config,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let explicit_read = invoke(
        &window,
        "get_ai_polish_api_key",
        json!({ "provider": "cerebras" }),
    )
    .unwrap()
    .deserialize::<String>()
    .unwrap();
    assert_eq!(explicit_read, "saved-cerebras-key");
    invoke(
        &window,
        "set_ai_polish_api_key",
        json!({ "provider": "cerebras", "apiKey": "edited-cerebras-key" }),
    )
    .unwrap();
    assert_eq!(
        store
            .provider_credential("cerebras")
            .get_password()
            .unwrap(),
        "edited-cerebras-key"
    );
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "saved-openai-key"
    );

    let legacy_read = invoke(&window, "get_ai_polish_api_key", json!({}))
        .unwrap()
        .deserialize::<String>()
        .unwrap();
    assert_eq!(legacy_read, "saved-openai-key");
    invoke(
        &window,
        "set_ai_polish_api_key",
        json!({ "apiKey": "replacement-openai-key" }),
    )
    .unwrap();
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "replacement-openai-key"
    );

    store
        .provider_credential("cerebras")
        .set_error(keyring::Error::BadEncoding(vec![0xff]));
    assert!(invoke(
        &window,
        "set_ai_polish_api_key",
        json!({ "provider": "cerebras", "apiKey": "failed-cerebras-key" }),
    )
    .is_err());
    assert_eq!(
        store
            .provider_credential("cerebras")
            .get_password()
            .unwrap(),
        "edited-cerebras-key"
    );
    invoke(
        &window,
        "set_ai_polish_api_key",
        json!({ "provider": "cerebras", "apiKey": "" }),
    )
    .unwrap();
    assert!(matches!(
        store.provider_credential("cerebras").get_password(),
        Err(keyring::Error::NoEntry)
    ));
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "replacement-openai-key"
    );
    assert_eq!(app.state::<AppState>().active_llm_provider(), "openai");
    assert_eq!(
        app.state::<AppState>()
            .profile
            .ai_polish_enabled
            .load(std::sync::atomic::Ordering::Acquire),
        enabled_before
    );

    store
        .provider_credential("cerebras")
        .set_password("preserved-cerebras-key")
        .unwrap();
    invoke(&window, "set_ai_polish_config", json!({ "enabled": true })).unwrap();
    assert!(app
        .state::<AppState>()
        .profile
        .ai_polish_enabled
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(
        store
            .provider_credential("cerebras")
            .get_password()
            .unwrap(),
        "preserved-cerebras-key"
    );
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "replacement-openai-key"
    );
    invoke(
        &window,
        "set_ai_polish_config",
        json!({ "enabled": false, "apiKey": "legacy-openai-key" }),
    )
    .unwrap();
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "legacy-openai-key"
    );
    invoke(
        &window,
        "set_ai_polish_config",
        json!({ "enabled": true, "apiKey": "configured-cerebras-key", "provider": "cerebras" }),
    )
    .unwrap();
    assert_eq!(
        store
            .provider_credential("cerebras")
            .get_password()
            .unwrap(),
        "configured-cerebras-key"
    );
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "legacy-openai-key"
    );
    invoke(
        &window,
        "set_ai_polish_config",
        json!({ "enabled": false, "apiKey": "", "provider": "cerebras" }),
    )
    .unwrap();
    assert!(matches!(
        store.provider_credential("cerebras").get_password(),
        Err(keyring::Error::NoEntry)
    ));
    assert_eq!(
        store.provider_credential("openai").get_password().unwrap(),
        "legacy-openai-key"
    );
    assert!(!app
        .state::<AppState>()
        .profile
        .ai_polish_enabled
        .load(std::sync::atomic::Ordering::Acquire));
}
