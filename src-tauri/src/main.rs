// 防止在 Windows 发布版本中显示额外的控制台窗口，请勿删除！
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    light_whisper_lib::run()
}
