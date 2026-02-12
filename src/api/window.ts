import { invoke } from "@tauri-apps/api/core";

/**
 * Hide the main application window (minimize to tray).
 */
export async function hideMainWindow(): Promise<string> {
  return invoke<string>("hide_main_window");
}

/**
 * Show the main application window.
 */
export async function showMainWindow(): Promise<string> {
  return invoke<string>("show_main_window");
}

/**
 * Create the subtitle overlay window (hidden by default).
 */
export async function createSubtitleWindow(): Promise<string> {
  return invoke<string>("create_subtitle_window");
}

/**
 * Show the subtitle overlay window (creates if needed).
 */
export async function showSubtitleWindow(): Promise<string> {
  return invoke<string>("show_subtitle_window");
}

/**
 * Hide the subtitle overlay window (keeps it alive for instant re-show).
 */
export async function hideSubtitleWindow(): Promise<string> {
  return invoke<string>("hide_subtitle_window");
}

/**
 * Destroy the subtitle overlay window.
 */
export async function destroySubtitleWindow(): Promise<string> {
  return invoke<string>("destroy_subtitle_window");
}
