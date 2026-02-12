import { invokeCommand } from "./invoke";

/**
 * Hide the main application window (minimize to tray).
 */
export async function hideMainWindow(): Promise<string> {
  return invokeCommand<string>("hide_main_window");
}

/**
 * Show the main application window.
 */
export async function showMainWindow(): Promise<string> {
  return invokeCommand<string>("show_main_window");
}

/**
 * Create the subtitle overlay window (hidden by default).
 */
export async function createSubtitleWindow(): Promise<string> {
  return invokeCommand<string>("create_subtitle_window");
}

/**
 * Show the subtitle overlay window (creates if needed).
 */
export async function showSubtitleWindow(): Promise<string> {
  return invokeCommand<string>("show_subtitle_window");
}

/**
 * Hide the subtitle overlay window (keeps it alive for instant re-show).
 */
export async function hideSubtitleWindow(): Promise<string> {
  return invokeCommand<string>("hide_subtitle_window");
}

/**
 * Destroy the subtitle overlay window.
 */
export async function destroySubtitleWindow(): Promise<string> {
  return invokeCommand<string>("destroy_subtitle_window");
}
