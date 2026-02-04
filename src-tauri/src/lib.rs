//! 轻语 Whisper - 语音转文字应用
//!
//! 这是应用的主入口文件，负责：
//! 1. 注册所有 Tauri 插件
//! 2. 初始化全局状态
//! 3. 设置系统托盘
//! 4. 注册所有前端可调用的命令
//! 5. 启动应用
//!
//! # 应用架构概览
//! ```text
//! ┌────────────────────────────────────────┐
//! │           前端 (React + TypeScript)       │
//! │  ┌──────┐ ┌──────┐ ┌──────┐           │
//! │  │ 录音  │ │ 转写  │ │ 设置  │           │
//! │  └──┬───┘ └──┬───┘ └──┬───┘           │
//! └─────┼────────┼────────┼────────────────┘
//!       │ invoke │        │
//! ┌─────┼────────┼────────┼───────────────┐
//! │     v        v        v               │
//! │          Tauri 命令层 (commands/)        │
//! │              │                          │
//! │          服务层 (services/)              │
//! │    ┌─────────┼──────────┐              │
//! │    v         v          v              │
//! │  FunASR                              │
//! │  (Python)                            │
//! │           Rust 后端                     │
//! └────────────────────────────────────────┘
//! ```

// 声明子模块
// `mod` 关键字告诉编译器去找对应的文件或目录
mod commands;   // -> commands/mod.rs
mod services;   // -> services/mod.rs
mod state;      // -> state/mod.rs
mod utils;      // -> utils/mod.rs

use state::AppState;
use tauri::Emitter;
use tauri::Manager;

/// 构建并运行 Tauri 应用
///
/// # Rust 知识点：pub fn 和 crate 可见性
/// `pub` 表示这个函数是公开的，外部可以调用。
/// `lib.rs` 中的 `pub fn` 可以被 `main.rs` 调用。
///
/// # Rust 知识点：为什么没有 main 函数？
/// Tauri v2 使用 `lib.rs` 作为入口而不是 `main.rs`。
/// 实际的 `main.rs` 很简单，只是调用这里的 `run()` 函数。
/// 这种设计让代码可以同时在桌面端和移动端使用。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 创建 Tauri 应用构建器
    //
    // `tauri::Builder::default()` 创建一个默认配置的构建器。
    // 然后通过链式调用（方法链）添加各种配置。
    // 最后 `.run()` 启动应用。
    tauri::Builder::default()
        // ============================================================
        // 注册 Tauri 插件
        // ============================================================
        //
        // 插件为应用提供额外的系统功能。
        // 每个 `.plugin()` 调用注册一个插件。

        // 全局快捷键插件：注册系统级快捷键
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())

        // 剪贴板插件：读写系统剪贴板
        .plugin(tauri_plugin_clipboard_manager::init())

        // 开机自启动插件
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),  // 启动时最小化
        ))

        // 日志插件：应用日志记录
        // 只输出到控制台（Stdout），避免重复
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )

        // ============================================================
        // 注册全局状态
        // ============================================================
        //
        // `.manage()` 把一个值注册为全局状态。
        // 之后在任何 Tauri 命令中，都可以通过 `tauri::State<AppState>` 访问它。
        // Tauri 会自动处理线程安全问题。
        .manage(AppState::new())

        // ============================================================
        // 应用启动回调
        // ============================================================
        //
        // `.setup()` 注册一个在应用启动后执行的回调函数。
        // 这里我们在启动时：
        // 1. 注册 F2 快捷键
        // 2. 异步启动 FunASR 服务器
        .setup(|app| {
            // 使用 Once 确保 setup 逻辑只执行一次
            // （Tauri v2 在某些平台上可能多次调用 setup）
            use std::sync::Once;
            static SETUP_ONCE: Once = Once::new();
            let mut already_ran = true;
            SETUP_ONCE.call_once(|| { already_ran = false; });
            if already_ran {
                log::warn!("setup() 被重复调用，跳过");
                return Ok(());
            }

            let app_handle = app.handle().clone();

            log::info!("轻语 Whisper 应用正在启动...");
            log::info!("数据目录: {:?}", utils::paths::get_data_dir());
            // 在后台启动 FunASR 服务器
            //
            // `tauri::async_runtime::spawn` 创建一个后台异步任务。
            // 这个任务不会阻塞应用启动，FunASR 会在后台初始化。
            //
            // `move` 关键字让闭包获取 `app_handle` 的所有权。
            // 因为闭包会在另一个线程执行，需要拥有数据的所有权。
            let handle_clone = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                log::info!("正在后台启动 FunASR 服务器...");

                // 获取全局状态
                //
                // `state::<AppState>()` 从应用中获取之前通过 `.manage()` 注册的状态。
                let state = handle_clone.state::<AppState>();

                // 尝试启动 FunASR
                match services::funasr_service::start_server(
                    &handle_clone,
                    state.inner(),
                )
                .await
                {
                    Ok(()) => {
                        log::info!("FunASR 服务器启动成功！");
                    }
                    Err(e) => {
                        log::error!("FunASR 服务器启动失败: {}", e);
                        // 通知前端启动失败
                        let _ = handle_clone.emit(
                            "funasr-status",
                            serde_json::json!({
                                "status": "error",
                                "message": format!("FunASR 启动失败: {}", e)
                            }),
                        );
                    }
                }
            });

            // 设置系统托盘
            setup_system_tray(&app_handle)?;

            Ok(())
        })

        // ============================================================
        // 注册所有 Tauri 命令
        // ============================================================
        //
        // `.invoke_handler()` 注册前端可以通过 `invoke()` 调用的命令。
        // `tauri::generate_handler![]` 宏自动生成命令注册代码。
        //
        // 每个命令对应一个 `#[tauri::command]` 标注的函数。
        // 前端通过函数名（蛇形命名）调用。
        .invoke_handler(tauri::generate_handler![
            // FunASR 命令
            commands::funasr::start_funasr,
            commands::funasr::transcribe_audio,
            commands::funasr::check_funasr_status,
            commands::funasr::check_model_files,
            commands::funasr::download_models,
            commands::funasr::cancel_model_download,
            commands::funasr::restart_funasr,
            commands::funasr::stop_funasr,
            // 剪贴板命令
            commands::clipboard::copy_to_clipboard,
            commands::clipboard::paste_text,
            // 窗口命令
            commands::window::hide_main_window,
            // 快捷键命令
            commands::hotkey::register_f2_hotkey,
            commands::hotkey::unregister_f2_hotkey,
            commands::hotkey::register_custom_hotkey,
        ])

        // ============================================================
        // 启动应用
        // ============================================================
        //
        // `.run()` 启动事件循环，应用正式运行。
        // `tauri::generate_context!()` 宏读取 `tauri.conf.json` 配置。
        // `.expect()` 如果启动失败则 panic（程序崩溃并显示错误信息）。
        .run(tauri::generate_context!())
        .expect("启动轻语 Whisper 时发生错误");
}

/// 设置系统托盘
///
/// 系统托盘（System Tray）是任务栏/菜单栏上的小图标，
/// 即使主窗口关闭，应用仍然在后台运行。
///
/// # 托盘菜单项
/// - 显示/隐藏主窗口
/// - 退出应用
///
/// # 参数
/// - `app_handle`：Tauri 应用句柄
///
/// # Rust 知识点：&引用
/// `&tauri::AppHandle` 中的 `&` 表示借用（引用）。
/// 我们只是"借用"app_handle 来使用，不需要获取它的所有权。
/// 函数结束后，所有权仍然属于调用者。
fn setup_system_tray(app_handle: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder};
    use tauri::tray::TrayIconBuilder;

    // 创建托盘菜单项
    //
    // `MenuItemBuilder::with_id()` 创建一个带 ID 的菜单项。
    // ID 用于在点击事件中识别哪个菜单项被点击。
    let show_item = MenuItemBuilder::with_id("show", "显示主窗口")
        .build(app_handle)?;
    let hide_item = MenuItemBuilder::with_id("hide", "隐藏主窗口")
        .build(app_handle)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出")
        .build(app_handle)?;

    // 构建菜单
    //
    // `separator()` 添加一条分隔线
    let menu = MenuBuilder::new(app_handle)
        .item(&show_item)
        .item(&hide_item)
        .item(&quit_item)
        .build()?;

    // 创建托盘图标
    //
    // `icon()` 设置托盘图标
    // `menu()` 设置右键菜单
    // `on_menu_event()` 设置菜单点击事件处理
    // `on_tray_icon_event()` 设置图标点击事件处理
    // 使用固定 ID 创建托盘图标，避免重复创建
    // 如果已存在同 ID 的托盘，Tauri 会复用它
    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app_handle.default_window_icon().unwrap().clone())
        .tooltip("轻语 Whisper - 语音转文字")
        .menu(&menu)
        .on_menu_event(|app, event| {
            // 处理托盘菜单点击事件
            //
            // `event.id().as_ref()` 获取菜单项的 ID 字符串
            // `match` 根据 ID 执行对应操作
            match event.id().as_ref() {
                "show" => {
                    // 显示主窗口
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                "hide" => {
                    // 隐藏主窗口
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                "quit" => {
                    // 退出应用
                    //
                    // 退出前先停止 FunASR 服务器
                    log::info!("用户请求退出应用");

                    // 获取状态并停止 FunASR
                    let state = app.state::<AppState>();
                    let funasr_process = state.funasr_process.clone();

                    // 尝试终止子进程（非阻塞）
                    if let Ok(mut guard) = funasr_process.try_lock() {
                        if let Some(ref mut process) = *guard {
                            log::info!("正在停止 FunASR 进程...");
                            let _ = process.child.start_kill();
                        }
                    }

                    // 退出应用
                    // `exit(0)` 以状态码 0（正常）退出
                    app.exit(0);
                }
                _ => {
                    // 未知的菜单项，忽略
                    log::warn!("未知的托盘菜单项: {:?}", event.id());
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            // 处理托盘图标点击事件
            //
            // 双击托盘图标时显示/隐藏主窗口
            if let tauri::tray::TrayIconEvent::DoubleClick { .. } = event {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    // 切换窗口显示/隐藏状态
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app_handle)?;

    log::info!("系统托盘已设置");
    Ok(())
}
