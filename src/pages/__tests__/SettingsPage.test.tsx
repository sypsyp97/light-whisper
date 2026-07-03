import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  getEngine: vi.fn(async () => "glm-asr"),
  setEngine: vi.fn(async () => "ok"),
  getOnlineAsrApiKey: vi.fn(async () => ""),
  setOnlineAsrApiKey: vi.fn(async () => undefined),
  getOnlineAsrEndpoint: vi.fn(async () => ({ region: "international", url: "https://api" })),
  setOnlineAsrEndpoint: vi.fn(async () => ({ region: "international", url: "https://api" })),
  getAlibabaAsrConfig: vi.fn(async () => ({ region: "international", url: "", model: "m", models: ["m"] })),
  setAlibabaAsrModel: vi.fn(async () => ({ model: "m" })),
  listAlibabaAsrModels: vi.fn(async () => ({ models: ["m"], source: "live" })),
  listInputDevices: vi.fn(async () => ({ devices: [], selectedDeviceName: null })),
  setInputDevice: vi.fn(async () => undefined),
  testMicrophone: vi.fn(async () => "ok"),
  startMicrophoneLevelMonitor: vi.fn(async () => "ok"),
  stopMicrophoneLevelMonitor: vi.fn(async () => undefined),
  setInputMethodCommand: vi.fn(async () => undefined),
  setSoundEnabled: vi.fn(async () => undefined),
  getUserProfile: vi.fn(async () => ({
    hot_words: [],
    correction_patterns: [],
    vocab_frequency: {},
    total_transcriptions: 0,
    last_updated: 0,
    llm_provider: { active: "openai", custom_providers: [] },
    web_search: { enabled: false, provider: "model_native", max_results: 5 },
  })),
  getAiPolishApiKey: vi.fn(async () => ""),
  setAiPolishConfig: vi.fn(async () => undefined),
  setAiPolishScreenContextEnabled: vi.fn(async () => undefined),
  setLlmProviderConfig: vi.fn(async () => undefined),
  listAiModels: vi.fn(async () => ({ models: [], sourceUrl: "x" })),
  getLlmReasoningSupport: vi.fn(async () => ({ supported: false, summary: "" })),
  addCustomProvider: vi.fn(async () => "id"),
  setCustomPrompt: vi.fn(async () => undefined),
  setOpenaiFastMode: vi.fn(async () => undefined),
  getOpenaiCodexOauthStatus: vi.fn(async () => ({ loggedIn: false })),
  loginOpenaiCodexOauth: vi.fn(async () => ({ loggedIn: false })),
  logoutOpenaiCodexOauth: vi.fn(async () => undefined),
  setAssistantHotkey: vi.fn(async () => undefined),
  setAssistantSystemPrompt: vi.fn(async () => undefined),
  setAssistantScreenContextEnabled: vi.fn(async () => undefined),
  setAssistantApiKey: vi.fn(async () => undefined),
  getAssistantApiKey: vi.fn(async () => ""),
  setAssistantLlmConfig: vi.fn(async () => undefined),
  setWebSearchConfig: vi.fn(async () => undefined),
  setWebSearchApiKey: vi.fn(async () => undefined),
  getWebSearchApiKey: vi.fn(async () => ""),
  setTranslationTarget: vi.fn(async () => false),
  setTranslationHotkey: vi.fn(async () => undefined),
  registerCustomHotkey: vi.fn(async () => "ok"),
  unregisterAllHotkeys: vi.fn(async () => "ok"),
  setRecordingMode: vi.fn(async () => undefined),
  getHotkeyDiagnostic: vi.fn(async () => ({
    shortcut: "F2",
    registered: true,
    backend: "RegisterHotKey",
    isPressed: false,
  })),
  addHotWord: vi.fn(async () => undefined),
  removeHotWord: vi.fn(async () => undefined),
  removeCorrection: vi.fn(async () => undefined),
  validateCorrections: vi.fn(async () => 0),
  setCorrectionValidationConfig: vi.fn(async () => undefined),
  checkPermission: vi.fn(async () => ({ granted: false, canRequest: true })),
  requestPermission: vi.fn(async () => ({ granted: true, canRequest: false })),
  pasteText: vi.fn(async () => "ok"),
  isAutostartEnabled: vi.fn(async () => false),
  enableAutostart: vi.fn(async () => undefined),
  disableAutostart: vi.fn(async () => undefined),
  exportUserProfile: vi.fn(async () => "{}"),
  importUserProfile: vi.fn(async () => undefined),
  checkAppUpdate: vi.fn(async () => ({ available: false, currentVersion: "1.3.3" })),
  openAppReleasePage: vi.fn(async () => "ok"),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "1.3.3"),
}));

vi.mock("@/contexts/RecordingContext", async () => {
  const helper = await import("@/test/renderWithContext");
  return {
    useRecordingContext: helper.useRecordingContext,
    RecordingProvider: helper.RecordingProvider,
  };
});

import SettingsPage from "@/pages/SettingsPage";

const onNavigate = vi.fn();

describe("SettingsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the settings-page root", () => {
    render(<SettingsPage onNavigate={onNavigate} active animClass="" />);
    expect(screen.getByTestId("settings-page")).toBeInTheDocument();
  });

  it("renders every settings section", () => {
    render(<SettingsPage onNavigate={onNavigate} active animClass="" />);
    const expected = [
      "appearance",
      "engine",
      "hotkey",
      "microphone",
      "input",
      "ai-polish",
      "assistant",
      "translation",
      "vocabulary",
      "permissions",
      "startup",
      "data",
      "update",
    ];
    for (const id of expected) {
      expect(screen.getByTestId(`settings-section-${id}`)).toBeInTheDocument();
    }
  });

  it("clicking the back button navigates to main", async () => {
    render(<SettingsPage onNavigate={onNavigate} active animClass="" />);
    await userEvent.click(screen.getByTestId("settings-back-btn"));
    expect(onNavigate).toHaveBeenCalledWith("main");
  });
});
