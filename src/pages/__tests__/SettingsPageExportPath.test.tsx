import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  addCustomProvider: vi.fn(),
  addHotWord: vi.fn(),
  checkAppUpdate: vi.fn(),
  completeOpenaiCodexOauthDeviceCode: vi.fn(),
  copyToClipboard: vi.fn(),
  disableAutostart: vi.fn(),
  enableAutostart: vi.fn(),
  exportUserProfile: vi.fn(),
  getAiPolishApiKey: vi.fn(),
  getAlibabaAsrConfig: vi.fn(),
  getAssistantApiKey: vi.fn(),
  getEngine: vi.fn(),
  getLlmReasoningSupport: vi.fn(),
  getModelsDir: vi.fn(),
  getOnlineAsrApiKey: vi.fn(),
  getOnlineAsrEndpoint: vi.fn(),
  getOpenaiCodexOauthStatus: vi.fn(),
  getUserProfile: vi.fn(),
  getWebSearchApiKey: vi.fn(),
  hideMainWindow: vi.fn(),
  importUserProfile: vi.fn(),
  isAutostartEnabled: vi.fn(),
  listAiModels: vi.fn(),
  listAlibabaAsrModels: vi.fn(),
  listInputDevices: vi.fn(),
  loginOpenaiCodexOauth: vi.fn(),
  logoutOpenaiCodexOauth: vi.fn(),
  openAppReleasePage: vi.fn(),
  pasteText: vi.fn(),
  pickFolder: vi.fn(),
  removeCorrection: vi.fn(),
  removeCustomProvider: vi.fn(),
  removeHotWord: vi.fn(),
  setAiPolishConfig: vi.fn(),
  setAiPolishScreenContextEnabled: vi.fn(),
  setAlibabaAsrModel: vi.fn(),
  setAssistantApiKey: vi.fn(),
  setAssistantHotkey: vi.fn(),
  setAssistantScreenContextEnabled: vi.fn(),
  setAssistantSystemPrompt: vi.fn(),
  setCorrectionValidationConfig: vi.fn(),
  setCustomPrompt: vi.fn(),
  setEngine: vi.fn(),
  setInputDevice: vi.fn(),
  setInputMethodCommand: vi.fn(),
  setLlmProviderConfig: vi.fn(),
  setModelsDir: vi.fn(),
  setOnlineAsrApiKey: vi.fn(),
  setOnlineAsrEndpoint: vi.fn(),
  setOpenaiFastMode: vi.fn(),
  setRecordingMode: vi.fn(),
  setSoundEnabled: vi.fn(),
  setTranslationHotkey: vi.fn(),
  setTranslationTarget: vi.fn(),
  setWebSearchApiKey: vi.fn(),
  setWebSearchConfig: vi.fn(),
  startMicrophoneLevelMonitor: vi.fn(),
  startOpenaiCodexOauthDeviceCode: vi.fn(),
  stopMicrophoneLevelMonitor: vi.fn(),
  testMicrophone: vi.fn(),
  validateCorrections: vi.fn(),
}));

const appMock = vi.hoisted(() => ({
  getVersion: vi.fn(),
}));

const eventMock = vi.hoisted(() => ({
  listen: vi.fn(),
}));

const recordingContextMock = vi.hoisted(() => ({
  retryModel: vi.fn(),
  setHotkey: vi.fn(),
}));

const storageMock = vi.hoisted(() => ({
  readLocalStorage: vi.fn(),
  writeLocalStorage: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
}));

vi.mock("@/api/tauri", () => tauriMock);

vi.mock("@tauri-apps/api/app", () => appMock);

vi.mock("@tauri-apps/api/event", () => eventMock);

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
  }),
}));

vi.mock("@/contexts/RecordingContext", () => ({
  useRecordingContext: () => ({
    hotkeyDiagnostic: null,
    hotkeyDisplay: "F2",
    hotkeyError: null,
    isRecording: false,
    retryModel: recordingContextMock.retryModel,
    setHotkey: recordingContextMock.setHotkey,
  }),
}));

vi.mock("@/hooks/useTheme", () => ({
  useTheme: () => ({
    isDark: false,
    setTheme: vi.fn(),
    theme: "light",
  }),
}));

vi.mock("@/lib/storage", () => storageMock);

vi.mock("sonner", () => ({
  toast: toastMock,
}));

const labels: Record<string, string> = {
  "common.copy": "Copy",
  "settings.copyExportPath": "Copy export path",
  "settings.exportConfig": "Export Config",
  "settings.exportPath": "Export path",
  "settings.importConfig": "Import Config",
  "settings.autostart": "Launch at Login",
};

vi.mock("@/i18n", () => ({
  default: {
    changeLanguage: vi.fn(),
    language: "en",
    t: (key: string) => labels[key] ?? key,
  },
}));

vi.mock("react-i18next", () => {
  return {
    initReactI18next: {
      init: vi.fn(),
      type: "3rdParty",
    },
    useTranslation: () => ({
      i18n: { changeLanguage: vi.fn(), language: "en" },
      t: (key: string) => labels[key] ?? key,
    }),
  };
});

const profile: UserProfile = {
  blocked_hot_words: [],
  correction_patterns: [],
  correction_validation_enabled: false,
  custom_prompt: null,
  hot_words: [],
  last_correction_validation: 0,
  last_updated: 0,
  llm_provider: {
    active: "cerebras",
    custom_providers: [],
  },
  total_transcriptions: 0,
  translation_hotkey: null,
  translation_target: null,
  vocab_frequency: {},
  web_search: {
    enabled: false,
    max_results: 5,
    provider: "model_native",
  },
};

function resetTauriMocks(exportPath: string | null = null) {
  for (const mock of Object.values(tauriMock)) {
    mock.mockReset();
    mock.mockResolvedValue(undefined);
  }

  tauriMock.copyToClipboard.mockResolvedValue("ok");
  tauriMock.exportUserProfile.mockResolvedValue(exportPath);
  tauriMock.getAiPolishApiKey.mockResolvedValue("");
  tauriMock.getAlibabaAsrConfig.mockResolvedValue({
    model: "qwen3-asr-flash",
    models: ["qwen3-asr-flash"],
    region: "international",
    url: "https://dashscope-intl.aliyuncs.com",
  });
  tauriMock.getAssistantApiKey.mockResolvedValue("");
  tauriMock.getEngine.mockResolvedValue("sensevoice");
  tauriMock.getLlmReasoningSupport.mockResolvedValue({
    strategy: null,
    summary: "reasoning unavailable",
    supported: false,
  });
  tauriMock.getModelsDir.mockResolvedValue({
    is_custom: false,
    path: "C:\\Users\\sun\\.cache\\light-whisper-models",
  });
  tauriMock.getOnlineAsrApiKey.mockResolvedValue("");
  tauriMock.getOnlineAsrEndpoint.mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: false });
  tauriMock.getUserProfile.mockResolvedValue(profile);
  tauriMock.getWebSearchApiKey.mockResolvedValue("");
  tauriMock.isAutostartEnabled.mockResolvedValue(false);
  tauriMock.listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  tauriMock.listAlibabaAsrModels.mockResolvedValue({
    models: ["qwen3-asr-flash"],
    source: "fallback",
  });
  tauriMock.listInputDevices.mockResolvedValue({
    devices: [],
    selectedDeviceName: null,
  });
  tauriMock.pickFolder.mockResolvedValue(null);
  tauriMock.setOnlineAsrEndpoint.mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  tauriMock.startOpenaiCodexOauthDeviceCode.mockResolvedValue({
    deviceAuthId: "device",
    intervalSecs: 5,
    userCode: "CODE-123",
    verificationUrl: "https://example.com",
  });
  tauriMock.validateCorrections.mockResolvedValue(0);
}

beforeEach(() => {
  resetTauriMocks();
  appMock.getVersion.mockReset();
  appMock.getVersion.mockResolvedValue("1.3.10");
  eventMock.listen.mockReset();
  eventMock.listen.mockResolvedValue(() => undefined);
  recordingContextMock.retryModel.mockReset();
  recordingContextMock.setHotkey.mockReset();
  recordingContextMock.setHotkey.mockResolvedValue(undefined);
  storageMock.readLocalStorage.mockReset();
  storageMock.readLocalStorage.mockReturnValue(null);
  storageMock.writeLocalStorage.mockReset();
  toastMock.error.mockReset();
  toastMock.success.mockReset();

  Object.defineProperty(window, "IntersectionObserver", {
    configurable: true,
    writable: true,
    value: class {
      disconnect() {}
      observe() {}
      unobserve() {}
    },
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    writable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SettingsPage config export path", () => {
  it("shows the saved export path and copies it from the small copy button", async () => {
    const exportPath = "C:\\Users\\sun\\Downloads\\light-whisper-profile.json";
    resetTauriMocks(exportPath);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Export Config" }));

    expect(await screen.findByText(exportPath)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Copy export path" }));

    await waitFor(() => {
      expect(tauriMock.copyToClipboard).toHaveBeenCalledWith(exportPath);
    });
  });
});

describe("SettingsPage autostart", () => {
  it("confirms the persisted plugin state and renders the enabled switch", async () => {
    tauriMock.isAutostartEnabled
      .mockReset()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    tauriMock.enableAutostart.mockResolvedValue(undefined);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).not.toBeDisabled();
    });

    fireEvent.click(toggle);

    await waitFor(() => {
      expect(tauriMock.enableAutostart).toHaveBeenCalledTimes(1);
      expect(tauriMock.isAutostartEnabled).toHaveBeenCalledTimes(2);
      expect(toggle).toHaveAttribute("aria-checked", "true");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.success).toHaveBeenCalledWith(
        "toast.autostartEnabled",
        { duration: 1100 },
      );
    });
  });

  it("reverts to off and reports an error when enable rejects", async () => {
    tauriMock.isAutostartEnabled.mockReset().mockResolvedValue(false);
    tauriMock.enableAutostart.mockRejectedValueOnce(new Error("enable failed"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.error).toHaveBeenCalledWith("toast.autostartFailed");
    });
    expect(tauriMock.isAutostartEnabled).toHaveBeenCalledTimes(1);
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("treats a persisted-state mismatch as failure without a success toast", async () => {
    tauriMock.isAutostartEnabled
      .mockReset()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(false);
    tauriMock.enableAutostart.mockResolvedValueOnce(undefined);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.error).toHaveBeenCalledWith("toast.autostartFailed");
    });
    expect(tauriMock.enableAutostart).toHaveBeenCalledTimes(1);
    expect(tauriMock.isAutostartEnabled).toHaveBeenCalledTimes(2);
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("reverts to off when persisted-state confirmation rejects", async () => {
    tauriMock.isAutostartEnabled
      .mockReset()
      .mockResolvedValueOnce(false)
      .mockRejectedValueOnce(new Error("confirmation unavailable"));
    tauriMock.enableAutostart.mockResolvedValueOnce(undefined);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.error).toHaveBeenCalledWith("toast.autostartFailed");
    });
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("disables autostart and confirms the persisted off state", async () => {
    tauriMock.isAutostartEnabled
      .mockReset()
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);
    tauriMock.disableAutostart.mockResolvedValueOnce(undefined);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "true");
      expect(toggle).not.toBeDisabled();
    });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(tauriMock.disableAutostart).toHaveBeenCalledTimes(1);
      expect(tauriMock.isAutostartEnabled).toHaveBeenCalledTimes(2);
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.success).toHaveBeenCalledWith(
        "toast.autostartDisabled",
        { duration: 1100 },
      );
    });
    expect(toastMock.error).not.toHaveBeenCalled();
  });

  it("reverts to on and reports an error when disable rejects", async () => {
    tauriMock.isAutostartEnabled.mockReset().mockResolvedValue(true);
    tauriMock.disableAutostart.mockRejectedValueOnce(new Error("disable failed"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "true");
      expect(toggle).not.toBeDisabled();
      expect(toastMock.error).toHaveBeenCalledWith("toast.autostartFailed");
    });
    expect(tauriMock.isAutostartEnabled).toHaveBeenCalledTimes(1);
    expect(toastMock.success).not.toHaveBeenCalled();
  });

  it("disables the switch and ignores repeated clicks while a write is pending", async () => {
    let resolveEnable!: () => void;
    const enablePending = new Promise<void>((resolve) => {
      resolveEnable = resolve;
    });
    tauriMock.isAutostartEnabled
      .mockReset()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    tauriMock.enableAutostart.mockReturnValueOnce(enablePending);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(toggle).toBeDisabled();
      expect(toggle).toHaveAttribute("aria-busy", "true");
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });
    fireEvent.click(toggle);
    expect(tauriMock.enableAutostart).toHaveBeenCalledTimes(1);
    expect(tauriMock.disableAutostart).not.toHaveBeenCalled();

    resolveEnable();
    await waitFor(() => {
      expect(toggle).not.toBeDisabled();
      expect(toggle).toHaveAttribute("aria-busy", "false");
      expect(toggle).toHaveAttribute("aria-checked", "true");
    });
  });
});
