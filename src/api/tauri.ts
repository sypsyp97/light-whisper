import { invoke } from "@tauri-apps/api/core";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import type {
  FunASRStatus,
  ModelCheckResult,
  TranscriptionResult,
  UserProfile,
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

export const startRecording = createNoArgCommand<number>("start_recording");
export const stopRecording = createNoArgCommand<void>("stop_recording");
export const testMicrophone = createNoArgCommand<string>("test_microphone");

export function setInputMethodCommand(method: string): Promise<void> {
  return invokeCommand<void>("set_input_method", { method });
}

export function setSoundEnabled(enabled: boolean): Promise<void> {
  return invokeCommand<void>("set_sound_enabled", { enabled });
}

export function setAiPolishConfig(enabled: boolean, apiKey: string): Promise<void> {
  return invokeCommand<void>("set_ai_polish_config", { enabled, apiKey });
}

export function getAiPolishApiKey(): Promise<string> {
  return invokeCommand<string>("get_ai_polish_api_key");
}

// Profile commands
export const getUserProfile = createNoArgCommand<UserProfile>("get_user_profile");

export function addHotWord(text: string, weight: number): Promise<void> {
  return invokeCommand<void>("add_hot_word", { text, weight });
}

export function removeHotWord(text: string): Promise<void> {
  return invokeCommand<void>("remove_hot_word", { text });
}

export function setLlmProviderConfig(
  active: string,
  customBaseUrl?: string,
  customModel?: string,
): Promise<void> {
  return invokeCommand<void>("set_llm_provider_config", {
    active,
    customBaseUrl: customBaseUrl ?? null,
    customModel: customModel ?? null,
  });
}

export const exportUserProfile = createNoArgCommand<string>("export_user_profile");

export function importUserProfile(jsonData: string): Promise<void> {
  return invokeCommand<void>("import_user_profile", { jsonData });
}

export function submitUserCorrection(original: string, corrected: string): Promise<void> {
  return invokeCommand<void>("submit_user_correction", { original, corrected });
}

export { enableAutostart, disableAutostart, isAutostartEnabled };
