import { invoke } from "@tauri-apps/api/core";

/**
 * Hide the main application window (minimize to tray).
 */
export async function hideMainWindow(): Promise<string> {
  return invoke<string>("hide_main_window");
}
