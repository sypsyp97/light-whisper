import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import * as tauri from "@/api/tauri";
import type { UserProfile } from "@/types";

vi.mock("@/api/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/tauri")>();
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [
    name,
    typeof value === "function" && name !== "IpcError"
      ? vi.fn().mockResolvedValue(undefined)
      : value,
  ]));
});
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn().mockResolvedValue("1.7.4") }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => undefined) }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ minimize: vi.fn() }),
}));
vi.mock("@/contexts/RecordingContext", () => ({
  useRecordingContext: () => ({
    hotkeyDiagnostic: null,
    hotkeyDisplay: "F2",
    hotkeyError: null,
    isRecording: false,
    retryModel: vi.fn(),
    setHotkey: vi.fn().mockResolvedValue(undefined),
  }),
}));
vi.mock("@/hooks/useTheme", () => ({
  useTheme: () => ({ isDark: false, setTheme: vi.fn(), theme: "light" }),
}));
vi.mock("@/lib/storage", () => ({
  readLocalStorage: () => null,
  writeLocalStorage: vi.fn(),
}));
vi.mock("sonner", () => ({ toast: { error: vi.fn(), info: vi.fn(), success: vi.fn() } }));

const translate = vi.hoisted(() => (key: string) => ({
  "common.retry": "Retry",
  "settings.historyEnabled": "Save local history",
}[key] ?? key));

vi.mock("@/i18n", () => ({
  default: { changeLanguage: vi.fn(), language: "en", t: translate },
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { init: vi.fn(), type: "3rdParty" },
  useTranslation: () => ({
    i18n: { changeLanguage: vi.fn(), language: "en" },
    t: translate,
  }),
}));

const profile: UserProfile = {
  blocked_hot_words: [],
  correction_patterns: [],
  correction_validation_enabled: false,
  custom_prompt: null,
  history_settings: { enabled: true, save_audio: false, retention_days: 30 },
  hot_words: [],
  last_correction_validation: 0,
  last_updated: 0,
  llm_provider: { active: "cerebras", custom_providers: [] },
  total_transcriptions: 0,
  translation_hotkey: null,
  translation_target: null,
  vocab_frequency: {},
  web_search: { enabled: false, max_results: 5, provider: "model_native" },
};

beforeEach(() => {
  for (const value of Object.values(tauri)) {
    if (vi.isMockFunction(value)) {
      value.mockReset();
      value.mockResolvedValue(undefined);
    }
  }
  vi.mocked(tauri.getAiPolishApiKey).mockResolvedValue("");
  vi.mocked(tauri.getAlibabaAsrConfig).mockResolvedValue({
    model: "qwen3-asr-flash",
    models: ["qwen3-asr-flash"],
    region: "international",
    url: "https://dashscope-intl.aliyuncs.com",
  });
  vi.mocked(tauri.getAssistantApiKey).mockResolvedValue("");
  vi.mocked(tauri.getEngine).mockResolvedValue("qwen3-asr-0.6b");
  vi.mocked(tauri.getAudioProcessingEnabled).mockResolvedValue(false);
  vi.mocked(tauri.setAudioProcessingEnabled).mockImplementation(async (enabled) => enabled);
  vi.mocked(tauri.getGpuIdleSeconds).mockResolvedValue(0);
  vi.mocked(tauri.getLlmReasoningSupport).mockResolvedValue({
    strategy: null,
    summary: "reasoning unavailable",
    supported: false,
  });
  vi.mocked(tauri.getModelsDir).mockResolvedValue({
    is_custom: false,
    path: "C:\\models",
  });
  vi.mocked(tauri.getOnlineAsrApiKey).mockResolvedValue("");
  vi.mocked(tauri.getOnlineAsrEndpoint).mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  vi.mocked(tauri.getGrokBuildOauthStatus).mockResolvedValue({ loggedIn: false });
  vi.mocked(tauri.getOpenaiCodexOauthStatus).mockResolvedValue({ loggedIn: false });
  vi.mocked(tauri.isAutostartEnabled).mockResolvedValue(false);
  vi.mocked(tauri.listAiModels).mockResolvedValue({ models: [], sourceUrl: "" });
  vi.mocked(tauri.listAlibabaAsrModels).mockResolvedValue({
    models: ["qwen3-asr-flash"],
    source: "fallback",
  });
  vi.mocked(tauri.listInputDevices).mockResolvedValue({ devices: [], selectedDeviceName: null });
  Object.defineProperty(window, "IntersectionObserver", {
    configurable: true,
    value: class {
      disconnect() {}
      observe() {}
      unobserve() {}
    },
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: vi.fn(),
  });
});

describe("SettingsPage profile load", () => {
  it("waits for the saved profile before exposing editable settings", async () => {
    let resolveProfile!: (value: UserProfile) => void;
    vi.mocked(tauri.getUserProfile).mockReturnValue(new Promise((resolve) => {
      resolveProfile = resolve;
    }));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    await waitFor(() => expect(tauri.getUserProfile).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("switch", { name: "Save local history" })).not.toBeInTheDocument();
    expect(tauri.setHistorySettings).not.toHaveBeenCalled();
    expect(tauri.getGrokBuildOauthStatus).not.toHaveBeenCalled();

    await act(async () => resolveProfile(profile));
    const historyToggle = await screen.findByRole("switch", { name: "Save local history" });
    expect(historyToggle).toHaveAttribute("aria-checked", "true");
    expect(tauri.setHistorySettings).not.toHaveBeenCalled();
    await waitFor(() => expect(tauri.getGrokBuildOauthStatus).toHaveBeenCalledTimes(1));
  });

  it("blocks default-profile writes on load failure and restores saved settings after retry", async () => {
    vi.mocked(tauri.getUserProfile)
      .mockRejectedValueOnce(new Error("profile unavailable"))
      .mockResolvedValue(profile);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const failure = await screen.findByRole("alert");
    expect(failure).toHaveTextContent(/\S/);
    expect(screen.queryByRole("switch", { name: "Save local history" })).not.toBeInTheDocument();
    expect(tauri.setHistorySettings).not.toHaveBeenCalled();
    expect(tauri.getGrokBuildOauthStatus).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(tauri.getUserProfile).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    const restoredToggle = await screen.findByRole("switch", { name: "Save local history" });
    expect(restoredToggle).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(tauri.getGrokBuildOauthStatus).toHaveBeenCalledTimes(1));
    fireEvent.click(restoredToggle);
    await waitFor(() => expect(tauri.setHistorySettings).toHaveBeenCalledWith(false, false, 30));
  });
});
