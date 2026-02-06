import { invoke } from "@tauri-apps/api/core";

/**
 * Copy text to the system clipboard.
 * Rust returns Result<String, AppError> — resolves to a message string.
 */
export async function copyToClipboard(text: string): Promise<string> {
  return invoke<string>("copy_to_clipboard", { text });
}

/**
 * Paste text by writing to clipboard and simulating Ctrl+V.
 * Rust returns Result<String, AppError> — resolves to a message string.
 */
export async function pasteText(
  text: string,
  method?: "sendInput" | "clipboard"
): Promise<string> {
  return invoke<string>("paste_text", { text, method });
}
