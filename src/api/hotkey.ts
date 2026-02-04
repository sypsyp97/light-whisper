import { invoke } from "@tauri-apps/api/core";

/**
 * Register the global F2 hotkey for push-to-talk recording.
 * Rust returns Result<String, AppError> — resolves to a message string on success,
 * throws on failure.
 */
export async function registerF2Hotkey(): Promise<string> {
  return invoke<string>("register_f2_hotkey");
}

/**
 * Unregister the global F2 hotkey.
 */
export async function unregisterF2Hotkey(): Promise<string> {
  return invoke<string>("unregister_f2_hotkey");
}
