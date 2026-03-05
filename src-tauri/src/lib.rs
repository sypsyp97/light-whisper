mod commands;
mod services;
mod state;
mod utils;

use state::AppState;
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_keyring::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("app".into()),
                    },
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .manage(AppState::new())
        .setup(|app| {
            if !mark_setup_once() {
                log::warn!("setup() 被重复调用，已跳过");
                return Ok(());
            }

            let app_handle = app.handle().clone();
            log::info!(
                "轻语 Whisper 应用启动，数据目录: {:?}",
                utils::paths::get_data_dir()
            );

            // 启动时加载用户画像
            {
                let state = app_handle.state::<AppState>();
                let loaded = services::profile_service::load_profile();
                if let Ok(mut profile) = state.user_profile.lock() {
                    *profile = loaded;
                }
                log::info!("已加载用户画像");
            }

            // 启动时根据活跃 provider 从系统密钥环加载对应 API Key
            {
                use tauri_plugin_keyring::KeyringExt;
                let state = app_handle.state::<AppState>();
                let provider = match state.user_profile.lock() {
                    Ok(p) => p.llm_provider.active.clone(),
                    Err(poisoned) => poisoned.into_inner().llm_provider.active.clone(),
                };
                let keyring_user = services::llm_provider::keyring_user_for_provider(&provider);
                if let Ok(Some(key)) = app_handle
                    .keyring()
                    .get_password("light-whisper", keyring_user)
                {
                    if !key.is_empty() {
                        if let Ok(mut api_key) = state.ai_polish_api_key.lock() {
                            *api_key = key;
                        }
                        log::info!(
                            "已从系统密钥环加载 AI 润色 API Key (provider: {})",
                            provider
                        );
                    }
                }
            }

            spawn_funasr_startup(app_handle.clone());
            spawn_subtitle_prewarm(app_handle.clone());
            spawn_profile_maintenance(app_handle.clone());
            setup_system_tray(&app_handle)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::funasr::start_funasr,
            commands::funasr::transcribe_audio,
            commands::funasr::check_funasr_status,
            commands::funasr::check_model_files,
            commands::funasr::download_models,
            commands::funasr::cancel_model_download,
            commands::funasr::restart_funasr,
            commands::funasr::get_engine,
            commands::funasr::set_engine,
            commands::clipboard::copy_to_clipboard,
            commands::clipboard::paste_text,
            commands::window::hide_main_window,
            commands::window::show_subtitle_window,
            commands::window::hide_subtitle_window,
            commands::hotkey::register_custom_hotkey,
            commands::hotkey::unregister_all_hotkeys,
            commands::audio::start_recording,
            commands::audio::stop_recording,
            commands::audio::test_microphone,
            commands::audio::set_input_method,
            commands::audio::set_sound_enabled,
            commands::ai_polish::set_ai_polish_config,
            commands::ai_polish::get_ai_polish_api_key,
            commands::profile::get_user_profile,
            commands::profile::add_hot_word,
            commands::profile::remove_hot_word,
            commands::profile::set_llm_provider_config,
            commands::profile::export_user_profile,
            commands::profile::import_user_profile,
            commands::profile::submit_user_correction,
        ])
        .run(tauri::generate_context!())
        .expect("启动轻语 Whisper 时发生错误");
}

fn mark_setup_once() -> bool {
    use std::sync::Once;

    static SETUP_ONCE: Once = Once::new();
    let mut first_run = false;
    SETUP_ONCE.call_once(|| {
        first_run = true;
    });
    first_run
}

fn spawn_funasr_startup(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        log::info!("正在后台启动 FunASR 服务器...");
        let state = app_handle.state::<AppState>();
        if let Err(err) = services::funasr_service::start_server(&app_handle, state.inner()).await {
            log::error!("FunASR 服务器启动失败: {}", err);
            let _ = app_handle.emit(
                "funasr-status",
                serde_json::json!({
                    "status": "error",
                    "message": format!("FunASR 启动失败: {}", err),
                }),
            );
        }
    });
}

fn spawn_subtitle_prewarm(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        match commands::window::create_subtitle_window(app_handle).await {
            Ok(_) => log::info!("字幕窗口预创建成功"),
            Err(err) => log::warn!("字幕窗口预创建失败（首次录音会重试）: {}", err),
        }
    });
}

fn spawn_profile_maintenance(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        const HOT_WORD_CLEANUP_INTERVAL_SECS: u64 = 24 * 60 * 60;
        log::info!(
            "已启动定期热词清理任务，周期 {} 秒",
            HOT_WORD_CLEANUP_INTERVAL_SECS
        );

        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            HOT_WORD_CLEANUP_INTERVAL_SECS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            let profile_to_save = {
                let state = app_handle.state::<AppState>();
                let mut profile = match state.user_profile.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => {
                        log::warn!("用户画像锁中毒，继续执行热词清理");
                        poisoned.into_inner()
                    }
                };

                let cleanup = services::profile_service::cleanup_profile(&mut profile);
                if cleanup.removed_hot_words > 0 || cleanup.removed_corrections > 0 {
                    log::info!(
                        "定期画像清理完成：热词移除 {} 条，纠错移除 {} 条",
                        cleanup.removed_hot_words,
                        cleanup.removed_corrections
                    );
                    Some(profile.clone())
                } else {
                    None
                }
            };

            if let Some(profile) = profile_to_save {
                if let Err(err) = services::profile_service::save_profile_async(&profile).await {
                    log::warn!("定期热词清理后保存用户画像失败: {}", err);
                }
            }
        }
    });
}

fn focus_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn hide_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            focus_main_window(app);
        }
    }
}

fn stop_funasr_on_exit(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    // 停止正在进行的录音
    if let Ok(mut guard) = state.recording.lock() {
        if let Some(session) = guard.take() {
            session
                .stop_flag
                .store(true, std::sync::atomic::Ordering::Relaxed);
            if let Some(task) = session.interim_task {
                task.abort();
            }
        }
    }

    let funasr_process = state.funasr_process.clone();

    tauri::async_runtime::block_on(async {
        match tokio::time::timeout(std::time::Duration::from_secs(2), funasr_process.lock()).await {
            Ok(mut guard) => {
                if let Some(ref mut process) = *guard {
                    log::info!("正在停止 FunASR 进程...");
                    let _ = process.child.start_kill();
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        process.child.wait(),
                    )
                    .await;
                }
            }
            Err(_) => {
                log::warn!("退出时获取 FunASR 进程锁超时，进程可能未被清理");
            }
        }
    });
}

fn setup_system_tray(app_handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    let show_item = MenuItemBuilder::with_id("show", "显示主窗口").build(app_handle)?;
    let hide_item = MenuItemBuilder::with_id("hide", "隐藏主窗口").build(app_handle)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app_handle)?;

    let menu = MenuBuilder::new(app_handle)
        .item(&show_item)
        .item(&hide_item)
        .item(&quit_item)
        .build()?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(
            app_handle
                .default_window_icon()
                .ok_or("缺少默认窗口图标")?
                .clone(),
        )
        .tooltip("轻语 Whisper - 语音转文字")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_main_window(app),
            "hide" => hide_main_window(app),
            "quit" => {
                log::info!("用户请求退出应用");
                stop_funasr_on_exit(app);
                app.exit(0);
            }
            _ => log::warn!("未知托盘菜单项: {:?}", event.id()),
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                toggle_main_window(tray.app_handle());
            }
        })
        .build(app_handle)?;

    Ok(())
}
