use tauri::{ipc::Invoke, Runtime};

pub(crate) fn restrict_app_commands<R: Runtime, F>(
    handler: F,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static
where
    F: Fn(Invoke<R>) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        let window = invoke.message.webview_ref().window();
        let command = invoke.message.command();
        // Overlay APIs are defined by SubtitleOverlay.tsx and SelectionOverlay.tsx.
        // All other application commands, including future additions, require main.
        let allowed = match window.label() {
            "main" => true,
            "subtitle" => matches!(
                command,
                "get_recording_snapshot"
                    | "copy_to_clipboard"
                    | "hide_subtitle_window"
                    | "continue_assistant_conversation"
                    | "cancel_assistant_conversation"
                    | "retry_assistant_request"
                    | "open_assistant_source"
            ),
            "selection-toolbar" => matches!(
                command,
                "resize_selection_window"
                    | "hide_selection_assistant"
                    | "start_selection_window_drag"
                    | "get_selection_overlay_state"
                    | "copy_selection"
                    | "replace_selection"
                    | "search_selection"
                    | "run_selection_action"
                    | "cancel_selection_action"
            ),
            _ => false,
        };
        if allowed {
            handler(invoke)
        } else {
            invoke.resolver.reject("此窗口无权调用该应用命令");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

    #[tauri::command]
    fn get_ai_polish_api_key(calls: tauri::State<'_, AtomicUsize>) -> &'static str {
        calls.fetch_add(1, Ordering::Relaxed);
        "test credential"
    }

    #[tauri::command]
    fn run_selection_action() -> &'static str {
        "selection result"
    }

    fn invoke(
        window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
        command: &str,
    ) -> Result<tauri::ipc::InvokeResponseBody, serde_json::Value> {
        get_ipc_response(
            window,
            tauri::webview::InvokeRequest {
                cmd: command.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
    }

    #[test]
    fn ipc_window_boundary_blocks_credentials_and_profile_commands() {
        use tauri::Manager;
        let app = mock_builder()
            .manage(AtomicUsize::new(0))
            .manage(crate::state::AppState::default())
            .invoke_handler(restrict_app_commands(tauri::generate_handler![
                get_ai_polish_api_key,
                run_selection_action,
                crate::commands::profile::get_user_profile,
                crate::commands::audio::get_recording_snapshot,
            ]))
            .build(mock_context(noop_assets()))
            .unwrap();
        for label in ["subtitle", "selection-toolbar", "unknown"] {
            let window = tauri::WebviewWindowBuilder::new(&app, label, Default::default())
                .build()
                .unwrap();
            for command in [
                "get_ai_polish_api_key",
                "get_user_profile",
                "set_sound_enabled",
                "export_user_profile",
            ] {
                let Err(error) = invoke(&window, command) else {
                    panic!("restricted command must be rejected");
                };
                assert!(
                    error.as_str().unwrap().contains("无权调用"),
                    "{label}/{command}: {error}"
                );
            }
            if label == "subtitle" {
                assert!(invoke(&window, "get_recording_snapshot").is_ok());
                assert!(invoke(&window, "run_selection_action").is_err());
            } else if label == "selection-toolbar" {
                assert!(invoke(&window, "run_selection_action").is_ok());
                assert!(invoke(&window, "get_recording_snapshot").is_err());
            } else {
                assert!(invoke(&window, "get_recording_snapshot").is_err());
                assert!(invoke(&window, "run_selection_action").is_err());
            }
        }
        assert_eq!(app.state::<AtomicUsize>().load(Ordering::Relaxed), 0);
        let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .unwrap();
        assert_eq!(
            invoke(&main, "get_ai_polish_api_key")
                .unwrap()
                .deserialize::<String>()
                .unwrap(),
            "test credential"
        );
        assert!(invoke(&main, "get_user_profile").is_ok());
        assert_eq!(app.state::<AtomicUsize>().load(Ordering::Relaxed), 1);
    }
}
