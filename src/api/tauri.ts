import { invoke } from "@tauri-apps/api/core";
import {
  disable as disableAutostartPlugin,
  enable as enableAutostartPlugin,
  isEnabled as isAutostartEnabledPlugin,
} from "@tauri-apps/plugin-autostart";
import type {
  FunASRStatus,
  ModelCheckResult,
  TranscriptionResult,
} from "@/types";

type InvokeArgs = Record<string, unknown>;

function invokeCommand<T>(
  command: string,
  args?: InvokeArgs
): Promise<T> {
  return args ? invoke<T>(command, args) : invoke<T>(command);
}

function createNoArgCommand<T = string>(
  command: string
): () => Promise<T> {
  return () => invokeCommand<T>(command);
}

export const startFunASR = createNoArgCommand<string>("start_funasr");

export function transcribeAudio(audioBase64: string): Promise<TranscriptionResult> {
  return invokeCommand<TranscriptionResult>("transcribe_audio", { audioBase64 });
}

export const checkFunASRStatus = createNoArgCommand<FunASRStatus>("check_funasr_status");
export const checkModelFiles = createNoArgCommand<ModelCheckResult>("check_model_files");
export const downloadModels = createNoArgCommand<string>("download_models");
export const cancelModelDownload = createNoArgCommand<string>("cancel_model_download");
export const restartFunASR = createNoArgCommand<string>("restart_funasr");
export const getEngine = createNoArgCommand<string>("get_engine");

export function setEngine(engine: string): Promise<string> {
  return invokeCommand<string>("set_engine", { engine });
}

export function copyToClipboard(text: string): Promise<string> {
  return invokeCommand<string>("copy_to_clipboard", { text });
}

export function pasteText(
  text: string,
  method?: "sendInput" | "clipboard"
): Promise<string> {
  return invokeCommand<string>("paste_text", { text, method });
}

export const hideMainWindow = createNoArgCommand<string>("hide_main_window");
export const showSubtitleWindow = createNoArgCommand<string>("show_subtitle_window");
export const hideSubtitleWindow = createNoArgCommand<string>("hide_subtitle_window");

export const unregisterAllHotkeys = createNoArgCommand<string>("unregister_all_hotkeys");

export function registerCustomHotkey(shortcut: string): Promise<string> {
  return invokeCommand<string>("register_custom_hotkey", { shortcut });
}

export async function enableAutostart(): Promise<void> {
  await enableAutostartPlugin();
}

export async function disableAutostart(): Promise<void> {
  await disableAutostartPlugin();
}

export async function isAutostartEnabled(): Promise<boolean> {
  return await isAutostartEnabledPlugin();
}
