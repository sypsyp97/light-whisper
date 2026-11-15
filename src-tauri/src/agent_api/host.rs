use super::{controller, transport};
use crate::{services, state::AppState, utils::paths};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::sync::watch;

struct HostOwner {
    shutdown: watch::Sender<bool>,
    server: parking_lot::Mutex<Option<transport::AgentServer>>,
}

pub(crate) fn register(app: &tauri::AppHandle) -> io::Result<()> {
    let (shutdown, receiver) = watch::channel(false);
    let handle = app.clone();
    let dispatch = Arc::new(move |request| {
        tauri::async_runtime::block_on(controller::dispatch(&handle, request, receiver.clone()))
    });
    let server = transport::AgentServer::start(paths::get_data_dir(), dispatch)?;
    app.manage(HostOwner {
        shutdown,
        server: parking_lot::Mutex::new(Some(server)),
    });
    Ok(())
}

pub(crate) fn shutdown(app: &tauri::AppHandle) {
    if let Some(owner) = app.try_state::<HostOwner>() {
        // Cancel admitted operations before joining workers; they may own native IPC.
        owner.shutdown.send_replace(true);
        let server = owner.server.lock().take();
        // The server's dispatch captures AppHandle. Drop outside the lock to drain
        // workers and break that ownership cycle before backend/state teardown.
        drop(server);
    }
}

pub(crate) fn run_client() -> io::Result<()> {
    transport::run_client(
        paths::get_data_dir(),
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        Duration::from_secs(180),
    )
}

pub(crate) fn run_host() -> Result<(), Box<dyn std::error::Error>> {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            let profile = services::profile_service::load_profile();
            state.update_profile_mut(|current| *current = profile);
            register(app.handle())?;
            if let Err(error) = announce_and_watch(app.handle()) {
                shutdown(app.handle());
                return Err(error.into());
            }
            Ok(())
        })
        .build(context)?;
    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            shutdown(app);
            let state = app.state::<AppState>();
            if let Err(error) =
                tauri::async_runtime::block_on(services::funasr_service::stop_server(state.inner()))
            {
                eprintln!("Agent host shutdown failed: {error}");
            }
        }
    });
    Ok(())
}

fn announce_and_watch(app: &tauri::AppHandle) -> io::Result<()> {
    let ready = json!({
        "success": true,
        "surface": "desktop-host",
        "agent_protocol": "LIGHT_WHISPER_AGENT_PROTOCOL_1",
        "pid": std::process::id(),
        "window_count": app.webview_windows().len(),
    });
    {
        let mut output = io::stdout().lock();
        serde_json::to_writer(&mut output, &ready)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }

    let handle = app.clone();
    std::thread::Builder::new()
        .name("agent-owner-input".into())
        .spawn(move || {
            for line in io::stdin().lock().lines() {
                match line {
                    Ok(line) => {
                        let request = serde_json::from_str::<Value>(&line).unwrap_or(Value::Null);
                        if request["action"] == "exit" {
                            break;
                        }
                    }
                    Err(error) => {
                        eprintln!("Agent host input failed: {error}");
                        handle.exit(1);
                        return;
                    }
                }
            }
            handle.exit(0);
        })?;
    Ok(())
}
