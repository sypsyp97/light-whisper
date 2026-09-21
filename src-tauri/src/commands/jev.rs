use std::sync::atomic::Ordering;

use crate::services::{jev_service, profile_service};
use crate::state::user_profile::{JevProvider, UserProfile};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeatureMode {
    Off,
    On,
    Auto,
}

impl FeatureMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "off" => Ok(Self::Off),
            "on" => Ok(Self::On),
            "auto" => Ok(Self::Auto),
            _ => Err("模式必须为 off、on 或 auto".to_string()),
        }
    }

    fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    fn jev_routing(self) -> bool {
        matches!(self, Self::Auto)
    }
}

fn apply_screen_context_mode(profile: &mut UserProfile, mode: FeatureMode) {
    profile.ai_polish_screen_context_enabled = mode.enabled();
    profile.assistant_screen_context_enabled = mode.enabled();
    profile.jev.screen_routing = mode.jev_routing();
}

fn apply_web_search_mode(profile: &mut UserProfile, mode: FeatureMode) {
    profile.web_search.enabled = mode.enabled();
    profile.jev.search_routing = mode.jev_routing();
}

#[tauri::command]
pub async fn set_jev_config(
    state: tauri::State<'_, AppState>,
    enabled: bool,
    provider: JevProvider,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.jev.enabled = enabled;
        profile.jev.provider = provider;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_jev_provider(
    state: tauri::State<'_, AppState>,
    provider: JevProvider,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.jev.provider = provider;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_polish_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    let mode = FeatureMode::parse(&mode)?;
    state
        .profile
        .ai_polish_enabled
        .store(mode.enabled(), Ordering::Release);
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.jev.enabled = mode.jev_routing();
    });
    Ok(())
}

#[tauri::command]
pub async fn set_jev_features(
    state: tauri::State<'_, AppState>,
    correction_review: bool,
    polish_audit: bool,
) -> Result<(), String> {
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        profile.jev.correction_review = correction_review;
        profile.jev.polish_audit = polish_audit;
    });
    Ok(())
}

#[tauri::command]
pub async fn set_screen_context_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    let mode = FeatureMode::parse(&mode)?;
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        apply_screen_context_mode(profile, mode);
    });
    Ok(())
}

#[tauri::command]
pub async fn set_web_search_mode(
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    let mode = FeatureMode::parse(&mode)?;
    profile_service::update_profile_and_schedule(state.inner(), |profile| {
        apply_web_search_mode(profile, mode);
    });
    Ok(())
}

#[tauri::command]
pub async fn get_jev_api_key(
    app_handle: tauri::AppHandle,
    provider: JevProvider,
) -> Result<String, String> {
    jev_service::load_api_key_for_provider(&app_handle, provider)
}

#[tauri::command]
pub async fn set_jev_api_key(
    app_handle: tauri::AppHandle,
    provider: JevProvider,
    api_key: String,
) -> Result<(), String> {
    jev_service::save_or_delete_api_key(&app_handle, provider, &api_key)
}

#[cfg(test)]
#[path = "jev_tests.rs"]
mod tests;
