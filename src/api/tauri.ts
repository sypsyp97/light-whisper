import { invoke } from "@tauri-apps/api/core";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import type {
  ApiFormat,
  AppUpdateInfo,
  AiModelListPayload,
  FunASRStatus,
  HotkeyDiagnostic,
  InputDeviceListPayload,
  LlmReasoningMode,
  LlmReasoningSupport,
  ModelCheckResult,
  TranscriptionResult,
  UserProfile,
} from "@/types";

type InvokeArgs = Record<string, unknown>;

function normalizeInvokeError(command: string, err: unknown): Error {
  if (err instanceof Error) {
    return err;
  }
  if (typeof err === "string" && err.trim()) {
    return new Error(err);
  }
  if (typeof err === "object" && err !== null) {
    const message = Reflect.get(err, "message");
    if (typeof message === "string" && message.trim()) {
      return new Error(message);
    }
    const error = Reflect.get(err, "error");
    if (typeof error === "string" && error.trim()) {
      return new Error(error);
    }
    try {
      return new Error(JSON.stringify(err));
    } catch {
      // Fall through to generic message.
    }
  }
  return new Error(`${command} 调用失败`);
}

function invokeCommand<T>(
  command: string,
  args?: InvokeArgs
): Promise<T> {
  const task = args ? invoke<T>(command, args) : invoke<T>(command);
  return task.catch((err) => {
    throw normalizeInvokeError(command, err);
  });
}

function createNoArgCommand<T = string>(
  command: string
): () => Promise<T> {
  return () => invokeCommand<T>(command);
}

export const startFunASR = createNoArgCommand<string>("start_funasr");
export const checkAppUpdate = createNoArgCommand<AppUpdateInfo>("check_app_update");

export function transcribeAudio(audioBase64: string): Promise<TranscriptionResult> {
  return invokeCommand<TranscriptionResult>("transcribe_audio", { audioBase64 });
}

export function openAppReleasePage(url?: string | null): Promise<string> {
  return invokeCommand<string>("open_app_release_page", { url: url ?? null });
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

export function registerAssistantHotkey(shortcut: string): Promise<string> {
  return invokeCommand<string>("register_assistant_hotkey", { shortcut });
}

export const startRecording = createNoArgCommand<number>("start_recording");
export const stopRecording = createNoArgCommand<void>("stop_recording");
export const testMicrophone = createNoArgCommand<string>("test_microphone");
export const listInputDevices = createNoArgCommand<InputDeviceListPayload>("list_input_devices");
export const startMicrophoneLevelMonitor = createNoArgCommand<string>("start_microphone_level_monitor");
export const stopMicrophoneLevelMonitor = createNoArgCommand<void>("stop_microphone_level_monitor");

export function setInputDevice(name?: string | null): Promise<void> {
  return invokeCommand<void>("set_input_device", { name: name ?? null });
}

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

export function setAiPolishScreenContextEnabled(enabled: boolean): Promise<void> {
  return invokeCommand<void>("set_ai_polish_screen_context_enabled", { enabled });
}

export function listAiModels(
  provider: string,
  baseUrl: string | undefined,
  apiKey: string,
): Promise<AiModelListPayload> {
  return invokeCommand<AiModelListPayload>("list_ai_models", {
    provider,
    baseUrl: baseUrl ?? null,
    apiKey,
  });
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
  polishReasoningMode?: LlmReasoningMode,
  assistantReasoningMode?: LlmReasoningMode,
  assistantUseSeparateModel?: boolean,
  assistantModel?: string,
  assistantProvider?: string | null,
): Promise<void> {
  return invokeCommand<void>("set_llm_provider_config", {
    active,
    customBaseUrl: customBaseUrl ?? null,
    customModel: customModel ?? null,
    polishReasoningMode: polishReasoningMode ?? null,
    assistantReasoningMode: assistantReasoningMode ?? null,
    assistantUseSeparateModel: assistantUseSeparateModel ?? null,
    assistantModel: assistantModel ?? null,
    assistantProvider: assistantProvider !== undefined ? assistantProvider : null,
  });
}

export function setAssistantApiKey(apiKey: string): Promise<void> {
  return invokeCommand<void>("set_assistant_api_key", { apiKey });
}

export function getAssistantApiKey(): Promise<string> {
  return invokeCommand<string>("get_assistant_api_key", {});
}

export function getLlmReasoningSupport(
  provider: string,
  baseUrl?: string,
  model?: string,
  apiFormat?: ApiFormat,
): Promise<LlmReasoningSupport> {
  return invokeCommand<LlmReasoningSupport>("get_llm_reasoning_support", {
    provider,
    baseUrl: baseUrl ?? null,
    model: model ?? null,
    apiFormat: apiFormat ?? null,
  });
}

export const exportUserProfile = createNoArgCommand<string>("export_user_profile");
export const getHotkeyDiagnostic = createNoArgCommand<HotkeyDiagnostic>("get_hotkey_diagnostic");

export function importUserProfile(jsonData: string): Promise<void> {
  return invokeCommand<void>("import_user_profile", { jsonData });
}

export function submitUserCorrection(original: string, corrected: string): Promise<void> {
  return invokeCommand<void>("submit_user_correction", { original, corrected });
}

export function setRecordingMode(toggle: boolean): Promise<void> {
  return invokeCommand<void>("set_recording_mode", { toggle });
}

/** 设置翻译目标语言。返回是否自动开启了 AI 润色。 */
export function setTranslationTarget(target: string | null): Promise<boolean> {
  return invokeCommand<boolean>("set_translation_target", { target });
}

export function setTranslationHotkey(shortcut: string | null): Promise<void> {
  return invokeCommand<void>("set_translation_hotkey", { shortcut });
}

export function setCustomPrompt(prompt: string | null): Promise<void> {
  return invokeCommand<void>("set_custom_prompt", { prompt });
}

export function setAssistantHotkey(shortcut: string | null): Promise<void> {
  return invokeCommand<void>("set_assistant_hotkey", { shortcut });
}

export function setAssistantSystemPrompt(prompt: string | null): Promise<void> {
  return invokeCommand<void>("set_assistant_system_prompt", { prompt });
}

export function setAssistantScreenContextEnabled(enabled: boolean): Promise<void> {
  return invokeCommand<void>("set_assistant_screen_context_enabled", { enabled });
}

export function addCustomProvider(
  name: string,
  baseUrl: string,
  model: string,
  apiFormat: "openai_compat" | "anthropic",
): Promise<string> {
  return invokeCommand<string>("add_custom_provider", { name, baseUrl, model, apiFormat });
}

export function updateCustomProvider(
  id: string,
  name?: string,
  baseUrl?: string,
  model?: string,
  apiFormat?: "openai_compat" | "anthropic",
): Promise<void> {
  return invokeCommand<void>("update_custom_provider", {
    id,
    name: name ?? null,
    baseUrl: baseUrl ?? null,
    model: model ?? null,
    apiFormat: apiFormat ?? null,
  });
}

export function removeCustomProvider(id: string): Promise<void> {
  return invokeCommand<void>("remove_custom_provider", { id });
}

export function setOnlineAsrApiKey(apiKey: string): Promise<void> {
  return invokeCommand<void>("set_online_asr_api_key", { apiKey });
}

export function getOnlineAsrApiKey(): Promise<string> {
  return invokeCommand<string>("get_online_asr_api_key");
}

export function getOnlineAsrEndpoint(): Promise<{ region: string; url: string }> {
  return invokeCommand<{ region: string; url: string }>("get_online_asr_endpoint");
}

export function setOnlineAsrEndpoint(region: string): Promise<{ region: string; url: string }> {
  return invokeCommand<{ region: string; url: string }>("set_online_asr_endpoint", { region });
}

export function getModelsDir(): Promise<{ path: string; is_custom: boolean }> {
  return invokeCommand<{ path: string; is_custom: boolean }>("get_models_dir");
}

export function pickFolder(): Promise<string | null> {
  return invokeCommand<string | null>("pick_folder");
}

export function setModelsDir(path: string | null, migrate: boolean): Promise<string> {
  return invokeCommand<string>("set_models_dir", { path, migrate });
}

export { enableAutostart, disableAutostart, isAutostartEnabled };
