import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppUpdateInfo, UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  addCustomProvider: vi.fn(),
  addHotWord: vi.fn(),
  checkAppUpdate: vi.fn(),
  completeGrokBuildOauthDeviceCode: vi.fn(),
  completeOpenaiCodexOauthDeviceCode: vi.fn(),
  copyToClipboard: vi.fn(),
  disableAutostart: vi.fn(),
  enableAutostart: vi.fn(),
  exportUserProfile: vi.fn(),
  getAiPolishApiKey: vi.fn(),
  getAlibabaAsrConfig: vi.fn(),
  getAssistantApiKey: vi.fn(),
  getEngine: vi.fn(),
  getGpuIdleSeconds: vi.fn(),
  setGpuIdleSeconds: vi.fn(),
  getLlmReasoningSupport: vi.fn(),
  getModelsDir: vi.fn(),
  getOnlineAsrApiKey: vi.fn(),
  getOnlineAsrEndpoint: vi.fn(),
  getGrokBuildOauthStatus: vi.fn(),
  getOpenaiCodexOauthStatus: vi.fn(),
  getUserProfile: vi.fn(),
  getWebSearchApiKey: vi.fn(),
  hideMainWindow: vi.fn(),
  importUserProfile: vi.fn(),
  isAutostartEnabled: vi.fn(),
  listAiModels: vi.fn(),
  listAlibabaAsrModels: vi.fn(),
  listInputDevices: vi.fn(),
  loginGrokBuildOauth: vi.fn(),
  loginOpenaiCodexOauth: vi.fn(),
  logoutGrokBuildOauth: vi.fn(),
  logoutOpenaiCodexOauth: vi.fn(),
  openAppReleasePage: vi.fn(),
  pasteText: vi.fn(),
  pickFolder: vi.fn(),
  removeCorrection: vi.fn(),
  removeCustomProvider: vi.fn(),
  removeHotWord: vi.fn(),
  setAiPolishConfig: vi.fn(),
  setAiPolishScreenContextEnabled: vi.fn(),
  setScreenContextEnabled: vi.fn(),
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
  setSelectionAssistantConfig: vi.fn(),
  setSoundEnabled: vi.fn(),
  setAutostartMinimized: vi.fn(),
  setTranslationHotkey: vi.fn(),
  setTranslationTarget: vi.fn(),
  setWebSearchApiKey: vi.fn(),
  setWebSearchConfig: vi.fn(),
  startMicrophoneLevelMonitor: vi.fn(),
  startGrokBuildOauthDeviceCode: vi.fn(),
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
  info: vi.fn(),
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
  "common.copyFailed": "Copy failed",
  "common.copiedToClipboard": "Copied to clipboard",
  "common.close": "Close",
  "common.copy": "Copy",
  "settings.addHotWordLabel": "Add hot word",
  "settings.correctionManage": "Manage correction rules",
  "settings.correctionRules": "Correction rules",
  "settings.correctionSearchLabel": "Search correction rules",
  "settings.correctionValidationToggle": "Audit correction rules",
  "settings.copyExportPath": "Copy export path",
  "settings.exportConfig": "Export Config",
  "settings.exportPath": "Export path",
  "settings.historySettings": "History settings",
  "settings.importConfig": "Import Config",
  "settings.checkAppUpdate": "Check for Updates",
  "settings.checkUpdate": "Check for Updates",
  "settings.checking": "Checking...",
  "settings.currentVersion": "Current version v{{version}}",
  "settings.goToDownload": "Go to Download",
  "settings.newVersionAvailable": "New version v{{version}} available",
  "settings.update": "Update",
  "settings.updateSource": "Source: GitHub Releases",
  "settings.startup": "Startup",
  "settings.autostart": "Launch at Login",
  "settings.autostartMinimized": "Start in the system tray",
  "settings.webSearchMaxResults": "Search result count",
  "toast.alreadyLatest": "Already up to date",
  "toast.checkingGitHub": "Checking GitHub Release...",
  "toast.checkUpdateFailed": "Update check failed",
  "toast.configExported": "Config exported",
  "toast.configExportFailed": "Export failed",
  "toast.newVersionFound": "New version v{{version}} available. Download from GitHub.",
  "toast.newVersionToast": "New version v{{version}} available",
  "toast.openReleaseFailed": "Failed to open download page",
};

function translate(key: string, options?: { version?: string | number }) {
  const template = labels[key] ?? key;
  return options?.version === undefined
    ? template
    : template.replace("{{version}}", String(options.version));
}

vi.mock("@/i18n", () => ({
  default: {
    changeLanguage: vi.fn(),
    language: "en",
    t: translate,
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
      t: translate,
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
  tauriMock.getEngine.mockResolvedValue("qwen3-asr-0.6b");
  tauriMock.getGpuIdleSeconds.mockResolvedValue(0);
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
  tauriMock.getGrokBuildOauthStatus.mockResolvedValue({ loggedIn: false });
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
  tauriMock.startGrokBuildOauthDeviceCode.mockResolvedValue({
    deviceCode: "device",
    intervalSecs: 5,
    userCode: "CODE-123",
    verificationUrl: "https://example.com",
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
  toastMock.info.mockReset();
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

describe("SettingsPage navigation", () => {
  it("scrolls only the settings content container when a navigation tab is clicked", async () => {
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    const rendered = render(<SettingsPage active onNavigate={vi.fn()} />);
    const content = rendered.container.querySelector<HTMLElement>(".settings-content");
    const target = rendered.container.querySelector<HTMLElement>(
      '[data-nav-id="history-settings"]',
    );
    expect(content).not.toBeNull();
    expect(target).not.toBeNull();

    const contentScrollTo = vi.fn();
    const targetScrollIntoView = vi.fn();
    Object.defineProperty(content!, "scrollTop", { configurable: true, value: 200 });
    Object.defineProperties(content!, {
      scrollHeight: { configurable: true, value: 2000 },
      clientHeight: { configurable: true, value: 500 },
    });
    Object.defineProperty(content!, "scrollTo", { configurable: true, value: contentScrollTo });
    Object.defineProperty(target!, "scrollIntoView", {
      configurable: true,
      value: targetScrollIntoView,
    });
    target!.style.scrollMarginTop = "4px";
    vi.spyOn(content!, "getBoundingClientRect").mockReturnValue({ top: 100 } as DOMRect);
    vi.spyOn(target!, "getBoundingClientRect").mockReturnValue({ top: 650 } as DOMRect);

    fireEvent.click(await screen.findByRole("button", { name: "History settings" }));

    expect(contentScrollTo).toHaveBeenCalledWith({ top: 746, behavior: "smooth" });
    expect(targetScrollIntoView).not.toHaveBeenCalled();
  });

  it("maps the startup navigation tab to the startup section", async () => {
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    const rendered = render(<SettingsPage active onNavigate={vi.fn()} />);
    const content = rendered.container.querySelector<HTMLElement>(".settings-content");
    const startupHeading = await screen.findByRole("heading", { name: "Startup" });
    const startupSection = startupHeading.closest<HTMLElement>("section");

    expect(content).not.toBeNull();
    expect(startupSection).not.toBeNull();
    expect(startupSection).toHaveAttribute("data-nav-id", "startup");
    expect(screen.getByText("settings.data").closest("section"))
      .not.toHaveAttribute("data-nav-id");

    const contentScrollTo = vi.fn();
    Object.defineProperty(content!, "scrollTop", { configurable: true, value: 100 });
    Object.defineProperties(content!, {
      scrollHeight: { configurable: true, value: 2000 },
      clientHeight: { configurable: true, value: 500 },
    });
    Object.defineProperty(content!, "scrollTo", { configurable: true, value: contentScrollTo });
    startupSection!.style.scrollMarginTop = "4px";
    vi.spyOn(content!, "getBoundingClientRect").mockReturnValue({ top: 100 } as DOMRect);
    vi.spyOn(startupSection!, "getBoundingClientRect").mockReturnValue({ top: 500 } as DOMRect);

    fireEvent.click(screen.getByRole("button", { name: "Startup" }));

    expect(contentScrollTo).toHaveBeenCalledWith({ top: 496, behavior: "smooth" });
  });
});

describe("SettingsPage correction rules dialog", () => {
  it("does not close the dialog when Escape dismisses its native provider picker", async () => {
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Manage correction rules" }));
    const dialog = await screen.findByRole("dialog", { name: "Correction rules" });
    fireEvent.click(screen.getByRole("switch", { name: "Audit correction rules" }));
    fireEvent.click(screen.getByRole("switch", { name: "settings.correctionValidationSeparateModel" }));
    const select = screen.getByRole("combobox", { name: "settings.provider" });
    fireEvent.keyDown(select.querySelector("option")!, { key: "Escape" });
    expect(dialog.closest(".correction-modal")).toHaveAttribute("data-closing", "false");
  });

  it("traps keyboard focus, closes with Escape, and restores the trigger", async () => {
    const user = userEvent.setup();
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const trigger = await screen.findByRole("button", { name: "Manage correction rules" });
    await user.click(trigger);

    const dialog = await screen.findByRole("dialog", { name: "Correction rules" });
    const search = await screen.findByRole("textbox", { name: "Search correction rules" });
    await waitFor(() => expect(search).toHaveFocus());

    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];

    last.focus();
    fireEvent.keyDown(last, { key: "Tab" });
    expect(first).toHaveFocus();

    first.focus();
    fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
    expect(last).toHaveFocus();

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Correction rules" })).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
  });

  it("closes only when the backdrop itself is clicked", async () => {
    const user = userEvent.setup();
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    const rendered = render(<SettingsPage active onNavigate={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Manage correction rules" }));

    const dialog = await screen.findByRole("dialog", { name: "Correction rules" });
    fireEvent.click(dialog);
    expect(dialog).toBeInTheDocument();

    const backdropDismiss = rendered.container.querySelector<HTMLElement>(".modal-dismiss");
    expect(backdropDismiss).not.toBeNull();
    fireEvent.click(backdropDismiss!);
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Correction rules" })).not.toBeInTheDocument());
  });
});

describe("SettingsPage config export path", () => {
  it("shows pending feedback and prevents duplicate dialogs until export settles", async () => {
    let finishExport!: (path: string | null) => void;
    tauriMock.exportUserProfile.mockReturnValueOnce(new Promise<string | null>((resolve) => {
      finishExport = resolve;
    }));
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);
    const button = await screen.findByRole("button", { name: "Export Config" });
    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    fireEvent.click(button);
    expect(tauriMock.exportUserProfile).toHaveBeenCalledTimes(1);
    finishExport(null);
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(button).toHaveAttribute("aria-busy", "false");
    expect(toastMock.success).not.toHaveBeenCalledWith("Config exported");
  });

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

  it("keeps the export path hidden when the native export is cancelled", async () => {
    tauriMock.exportUserProfile.mockResolvedValueOnce(null);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Export Config" }));

    await waitFor(() => expect(tauriMock.exportUserProfile).toHaveBeenCalledTimes(1));
    expect(screen.queryByText("Export path")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Copy export path" })).not.toBeInTheDocument();
    expect(toastMock.success).not.toHaveBeenCalledWith("Config exported");
  });

  it("reports an export failure without leaving a stale path or success toast", async () => {
    tauriMock.exportUserProfile.mockRejectedValueOnce(new Error("export failed"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Export Config" }));

    await waitFor(() => {
      expect(tauriMock.exportUserProfile).toHaveBeenCalledTimes(1);
      expect(toastMock.error).toHaveBeenCalledWith("Export failed");
    });
    expect(screen.queryByText("Export path")).not.toBeInTheDocument();
    expect(toastMock.success).not.toHaveBeenCalledWith("Config exported");
  });

  it("reports a copy failure after an export path has been shown", async () => {
    const exportPath = "C:\\Users\\sun\\Downloads\\light-whisper-profile.json";
    tauriMock.exportUserProfile.mockResolvedValueOnce(exportPath);
    tauriMock.copyToClipboard.mockRejectedValueOnce(new Error("copy failed"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Export Config" }));
    expect(await screen.findByText(exportPath)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Copy export path" }));

    await waitFor(() => {
      expect(tauriMock.copyToClipboard).toHaveBeenCalledWith(exportPath);
      expect(toastMock.error).toHaveBeenCalledWith("Copy failed");
    });
    expect(toastMock.success).toHaveBeenCalledWith("Config exported");
    expect(toastMock.success).not.toHaveBeenCalledWith("Copied to clipboard");
  });
});

describe("SettingsPage app updates", () => {
  it("reports when the installed version is already current", async () => {
    const updateInfo: AppUpdateInfo = {
      available: false,
      currentVersion: "1.5.9",
      latestVersion: null,
      releaseUrl: null,
    };
    tauriMock.checkAppUpdate.mockResolvedValueOnce(updateInfo);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    expect(await screen.findByText("Current version v1.3.10")).toBeInTheDocument();
    const action = await screen.findByRole("button", { name: "Check for Updates" });
    fireEvent.click(action);

    await waitFor(() => {
      expect(tauriMock.checkAppUpdate).toHaveBeenCalledTimes(1);
      expect(screen.getByText("Already up to date")).toBeInTheDocument();
      expect(toastMock.success).toHaveBeenCalledWith("Already up to date");
      expect(action).not.toBeDisabled();
    });
    expect(screen.queryByRole("button", { name: "Go to Download" })).not.toBeInTheDocument();
  });

  it("shows an available version and opens its release page on the follow-up action", async () => {
    const releaseUrl = "https://github.com/sypsyp97/light-whisper/releases/tag/v1.6.0";
    const updateInfo: AppUpdateInfo = {
      available: true,
      currentVersion: "1.5.9",
      latestVersion: "1.6.0",
      releaseUrl,
    };
    tauriMock.checkAppUpdate.mockResolvedValueOnce(updateInfo);
    tauriMock.openAppReleasePage.mockResolvedValueOnce("Opened download page");

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Check for Updates" }));

    await waitFor(() => {
      expect(screen.getByText("New version v1.6.0 available")).toBeInTheDocument();
      expect(screen.getByText("New version v1.6.0 available. Download from GitHub.")).toBeInTheDocument();
      expect(toastMock.info).toHaveBeenCalledWith("New version v1.6.0 available");
    });

    fireEvent.click(await screen.findByRole("button", { name: "Go to Download" }));

    await waitFor(() => {
      expect(tauriMock.openAppReleasePage).toHaveBeenCalledWith(releaseUrl);
      expect(toastMock.success).toHaveBeenCalledWith("Opened download page");
    });
  });

  it("keeps the update action disabled during a pending check and ignores a repeated click", async () => {
    let resolveCheck!: (value: AppUpdateInfo) => void;
    const checkPending = new Promise<AppUpdateInfo>((resolve) => {
      resolveCheck = resolve;
    });
    tauriMock.checkAppUpdate.mockReturnValueOnce(checkPending);

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const action = await screen.findByRole("button", { name: "Check for Updates" });
    fireEvent.click(action);

    await waitFor(() => {
      expect(action).toBeDisabled();
      expect(action).toHaveTextContent("Checking...");
    });
    fireEvent.click(action);
    expect(tauriMock.checkAppUpdate).toHaveBeenCalledTimes(1);

    resolveCheck({
      available: false,
      currentVersion: "1.5.9",
      latestVersion: null,
      releaseUrl: null,
    });
    await waitFor(() => {
      expect(action).not.toBeDisabled();
      expect(action).toHaveTextContent("Check for Updates");
    });
  });

  it("reports a release-page failure after a new version has been found", async () => {
    tauriMock.checkAppUpdate.mockResolvedValueOnce({
      available: true,
      currentVersion: "1.5.9",
      latestVersion: "1.6.0",
      releaseUrl: "https://github.com/sypsyp97/light-whisper/releases/tag/v1.6.0",
    } satisfies AppUpdateInfo);
    tauriMock.openAppReleasePage.mockRejectedValueOnce(new Error("browser unavailable"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    fireEvent.click(await screen.findByRole("button", { name: "Check for Updates" }));
    fireEvent.click(await screen.findByRole("button", { name: "Go to Download" }));

    await waitFor(() => {
      expect(screen.getByText("browser unavailable")).toBeInTheDocument();
      expect(toastMock.error).toHaveBeenCalledWith("browser unavailable");
    });
  });

  it("shows the backend error when checking for an update fails", async () => {
    tauriMock.checkAppUpdate.mockRejectedValueOnce(new Error("GitHub unavailable"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const action = await screen.findByRole("button", { name: "Check for Updates" });
    fireEvent.click(action);

    await waitFor(() => {
      expect(screen.getByText("GitHub unavailable")).toBeInTheDocument();
      expect(toastMock.error).toHaveBeenCalledWith("GitHub unavailable");
      expect(action).not.toBeDisabled();
    });
  });
});

describe("SettingsPage autostart", () => {
  it("reveals the tray option below autostart and hides it when autostart is disabled", async () => {
    tauriMock.isAutostartEnabled.mockReset()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false);
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    expect(screen.queryByRole("switch", { name: "Start in the system tray" })).not.toBeInTheDocument();
    fireEvent.click(toggle);
    const trayToggle = await screen.findByRole("switch", { name: "Start in the system tray" });
    await waitFor(() => expect(toggle).not.toBeDisabled());
    expect(trayToggle).toHaveAttribute("aria-checked", "false");
    expect(trayToggle.closest(".settings-reveal")).toHaveAttribute("data-open", "true");
    expect(toggle.compareDocumentPosition(trayToggle) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    fireEvent.click(toggle);
    await waitFor(() => {
      expect(toggle).not.toBeDisabled();
      expect(screen.queryByRole("switch", { name: "Start in the system tray" })).not.toBeInTheDocument();
    });
    expect(tauriMock.setAutostartMinimized).not.toHaveBeenCalled();
  });

  it("loads the saved tray preference and persists both toggle directions", async () => {
    tauriMock.isAutostartEnabled.mockResolvedValue(true);
    tauriMock.getUserProfile.mockResolvedValue({ ...profile, autostart_minimized: true });
    tauriMock.setAutostartMinimized.mockImplementation(async (enabled: boolean) => {
      tauriMock.getUserProfile.mockResolvedValue({ ...profile, autostart_minimized: enabled });
    });
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const trayToggle = await screen.findByRole("switch", { name: "Start in the system tray" });
    await waitFor(() => expect(trayToggle).toHaveAttribute("aria-checked", "true"));
    fireEvent.click(trayToggle);
    await waitFor(() => {
      expect(tauriMock.setAutostartMinimized).toHaveBeenLastCalledWith(false);
      expect(trayToggle).toHaveAttribute("aria-checked", "false");
      expect(trayToggle).not.toBeDisabled();
    });
    fireEvent.click(trayToggle);
    await waitFor(() => {
      expect(tauriMock.setAutostartMinimized).toHaveBeenLastCalledWith(true);
      expect(trayToggle).toHaveAttribute("aria-checked", "true");
      expect(trayToggle).not.toBeDisabled();
    });
    expect(tauriMock.setAutostartMinimized).toHaveBeenCalledTimes(2);
  });

  it("blocks repeated tray writes and retains the saved preference when saving fails", async () => {
    tauriMock.isAutostartEnabled.mockResolvedValue(true);
    let rejectSave!: (error: Error) => void;
    tauriMock.setAutostartMinimized.mockReturnValueOnce(new Promise<void>((_resolve, reject) => {
      rejectSave = reject;
    }));
    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const trayToggle = await screen.findByRole("switch", { name: "Start in the system tray" });
    await waitFor(() => expect(trayToggle).not.toBeDisabled());
    fireEvent.click(trayToggle);
    expect(trayToggle).toBeDisabled();
    expect(trayToggle).toHaveAttribute("aria-busy", "true");
    fireEvent.click(trayToggle);
    expect(tauriMock.setAutostartMinimized).toHaveBeenCalledTimes(1);
    rejectSave(new Error("disk unavailable"));
    await waitFor(() => {
      expect(trayToggle).not.toBeDisabled();
      expect(trayToggle).toHaveAttribute("aria-checked", "false");
      expect(toastMock.error).toHaveBeenCalledWith("toast.autostartMinimizedFailed");
    });
  });

  it("leaves the switch off and usable when the initial state cannot be read", async () => {
    tauriMock.isAutostartEnabled.mockRejectedValueOnce(new Error("autostart unavailable"));

    const { default: SettingsPage } = await import("@/pages/SettingsPage");
    render(<SettingsPage active onNavigate={vi.fn()} />);

    const toggle = await screen.findByRole("switch", { name: "Launch at Login" });
    await waitFor(() => {
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(toggle).toHaveAttribute("aria-busy", "false");
      expect(toggle).not.toBeDisabled();
    });
    expect(toastMock.error).not.toHaveBeenCalled();
  });

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
