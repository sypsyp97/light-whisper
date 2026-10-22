import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { INPUT_DEVICE_STORAGE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY } from "@/lib/constants";
import type { UserProfile } from "@/types";

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
  setAlibabaAsrModel: vi.fn(),
  setAssistantApiKey: vi.fn(),
  setAssistantHotkey: vi.fn(),
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

const eventDisposers = vi.hoisted(() => [] as Array<ReturnType<typeof vi.fn>>);

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
  getCurrentWindow: () => ({ minimize: vi.fn() }),
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
  useTheme: () => ({ isDark: false, setTheme: vi.fn(), theme: "light" }),
}));
vi.mock("@/lib/storage", () => storageMock);
vi.mock("sonner", () => ({ toast: toastMock }));

const labels: Record<string, string> = {
  "common.add": "Add",
  "common.cancel": "Cancel",
  "common.clear": "Clear",
  "common.close": "Close",
  "common.copy": "Copy",
  "common.change": "Change",
  "common.refresh": "Refresh",
  "common.retry": "Retry",
  "common.test": "Test",
  "settings.addCustomProvider": "Add Custom Provider",
  "settings.alsoSystemDefault": "Also the system default",
  "settings.alibabaApiKeyPlaceholder": "Alibaba API Key",
  "settings.alibabaAsrDesc": "Alibaba cloud speech recognition",
  "settings.alibabaAsrLabel": "Alibaba DashScope",
  "settings.apiFormatLabel": "API Format",
  "settings.apiKey": "API Key",
  "settings.assistantApiKey": "Assistant API Key",
  "settings.assistantModelLabel": "Assistant model name",
  "settings.assistantProvider": "Assistant Provider",
  "settings.assistantSeparateConfig": "Assistant uses separate config",
  "settings.baseUrlLabel": "Base URL",
  "settings.defaultModelLabel": "Provider default model",
  "settings.fetching": "Fetching...",
  "settings.fixedMic": "Use this microphone exclusively",
  "settings.followSystemMic": "Use system default microphone",
  "settings.glmApiKeyPlaceholder": "GLM API Key",
  "settings.glmAsrDesc": "GLM cloud speech recognition",
  "settings.engine": "ASR Engine",
  "settings.domestic": "Domestic",
  "settings.international": "International",
  "settings.currentDefault": "System default: {{name}}",
  "settings.micLevelMonitor": "Microphone Level Monitor",
  "settings.micMonitorOff": "Microphone level monitor is off",
  "settings.micNotStarted": "Microphone level monitor not started",
  "settings.micRecordingPaused": "Microphone monitoring is paused while recording",
  "settings.micSpeakToTest": "Speak into the microphone to see level changes",
  "settings.microphone": "Microphone",
  "settings.modelStorageDir": "Model directory",
  "settings.modelNameLabel": "Model name",
  "settings.autoUseDefault": "Automatically use system default input device",
  "settings.canSelect": "Select this microphone",
  "settings.qwen3Asr06Desc": "Fast local speech recognition",
  "settings.r2t2Desc": "Higher accuracy local speech recognition",
  "settings.restoreDefault": "Restore default",
  "settings.savedMicUnavailable": "Saved microphone is unavailable",
  "settings.selectMic": "Select Microphone",
  "settings.systemDefaultDevice": "System default",
  "settings.openAssistantModelList": "Open assistant model list",
  "settings.openModelList": "Open model list",
  "settings.providerBaseUrlLabel": "Provider Base URL",
  "settings.providerNameLabel": "Provider name",
  "settings.searchAssistantModel": "Search assistant model",
  "settings.searchAssistantProviderLabel": "Search assistant provider",
  "settings.searchModelLabel": "Search model",
  "settings.searchProviderLabel": "Search provider",
  "settings.selectProvider": "Select LLM Provider",
  "settings.showApiKey": "Show API Key",
  "settings.useSeparateConfig": "Use Separate Config",
  "toast.micSwitchFailed": "Failed to switch microphone",
  "toast.modelsDirUpdated": "Model directory updated",
  "toast.switchEngineFailed": "Failed to switch engine",
  "toast.switchedToEngine": "Switched to {{label}} engine",
};

function translate(key: string, options?: Record<string, unknown>) {
  const template = labels[key] ?? key;
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) => (
    options?.[name] === undefined ? `{{${name}}}` : String(options[name])
  ));
}

vi.mock("@/i18n", () => ({
  default: {
    changeLanguage: vi.fn(),
    language: "en",
    t: translate,
  },
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { init: vi.fn(), type: "3rdParty" },
  useTranslation: () => ({
    i18n: { changeLanguage: vi.fn(), language: "en" },
    t: translate,
  }),
}));

const baseProfile: UserProfile = {
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

function resetMocks(profile: UserProfile = baseProfile) {
  for (const mock of Object.values(tauriMock)) {
    mock.mockReset();
    mock.mockResolvedValue(undefined);
  }
  tauriMock.getUserProfile.mockResolvedValue(profile);
  tauriMock.getAiPolishApiKey.mockResolvedValue("");
  tauriMock.getAssistantApiKey.mockResolvedValue("");
  tauriMock.getAlibabaAsrConfig.mockResolvedValue({
    model: "qwen3-asr-flash",
    models: ["qwen3-asr-flash"],
    region: "international",
    url: "https://dashscope-intl.aliyuncs.com",
  });
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
  tauriMock.isAutostartEnabled.mockResolvedValue(false);
  tauriMock.listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  tauriMock.listAlibabaAsrModels.mockResolvedValue({
    models: ["qwen3-asr-flash"],
    source: "fallback",
  });
  tauriMock.listInputDevices.mockResolvedValue({ devices: [], selectedDeviceName: null });
  tauriMock.pickFolder.mockResolvedValue(null);
  tauriMock.setOnlineAsrEndpoint.mockResolvedValue({
    region: "international",
    url: "https://api.zhipuai.cn",
  });
  tauriMock.setAiPolishConfig.mockResolvedValue(undefined);
  tauriMock.setLlmProviderConfig.mockResolvedValue(undefined);
  tauriMock.addCustomProvider.mockResolvedValue("custom-provider");
  appMock.getVersion.mockReset();
  appMock.getVersion.mockResolvedValue("1.5.9");
  eventMock.listen.mockReset();
  eventDisposers.length = 0;
  eventMock.listen.mockImplementation(() => {
    const disposer = vi.fn();
    eventDisposers.push(disposer);
    return Promise.resolve(disposer);
  });
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
}

async function renderSettings(profile: UserProfile = baseProfile, active = true) {
  tauriMock.getUserProfile.mockReset();
  tauriMock.getUserProfile.mockResolvedValue(profile);
  const { default: SettingsPage } = await import("@/pages/SettingsPage");
  const rendered = render(<SettingsPage active={active} onNavigate={vi.fn()} />);
  await waitFor(() => expect(tauriMock.getUserProfile).toHaveBeenCalledWith());
  await waitFor(() => expect(tauriMock.getAiPolishApiKey).toHaveBeenCalledTimes(1));
  return rendered;
}

beforeEach(() => resetMocks());
afterEach(() => vi.clearAllMocks());

describe("SettingsPage ASR and device settings", () => {
  it("selects the final navigation section when manually scrolling to the bottom", async () => {
    const { container } = await renderSettings();
    const content = container.querySelector<HTMLElement>(".settings-content")!;
    Object.defineProperties(content, {
      scrollHeight: { configurable: true, value: 2400 },
      clientHeight: { configurable: true, value: 600 },
    });
    content.scrollTop = 1800;
    fireEvent.scroll(content);
    await waitFor(() => expect(container.querySelector('[data-nav-tab="startup"]')).toHaveAttribute("data-active", "true"));
  });
  it("loads the persisted engine, microphone, and model directory and applies a directory change", async () => {
    const defaultModelsDir = "C:\\light-whisper\\models";
    const customModelsDir = "D:\\light-whisper\\models";
    tauriMock.getEngine.mockResolvedValue("confucius4-r2t2");
    tauriMock.getModelsDir
      .mockResolvedValueOnce({ path: defaultModelsDir, is_custom: false })
      .mockResolvedValueOnce({ path: customModelsDir, is_custom: true });
    tauriMock.listInputDevices.mockResolvedValue({
      devices: [
        { name: "Built-in Microphone", isDefault: true },
        { name: "USB Microphone", isDefault: false },
      ],
      selectedDeviceName: "USB Microphone",
    });
    tauriMock.pickFolder.mockResolvedValue(customModelsDir);
    tauriMock.setModelsDir.mockResolvedValue({ runtimeWarning: null });

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).toHaveTextContent("Confucius4-R2T2 Q8"));
    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).toHaveTextContent("USB Microphone"));
    expect(screen.getByText(defaultModelsDir)).toBeInTheDocument();

    fireEvent.click(within(engineSection!).getByRole("button", { name: "Change" }));

    await waitFor(() => {
      expect(tauriMock.pickFolder).toHaveBeenCalledWith();
      expect(tauriMock.setModelsDir).toHaveBeenCalledWith(customModelsDir, true);
      expect(screen.getByText(customModelsDir)).toBeInTheDocument();
    });
    expect(tauriMock.getModelsDir).toHaveBeenCalledTimes(2);
    expect(toastMock.success).toHaveBeenCalledWith("Model directory updated");
  });

  it("switches to an online engine and refreshes its endpoint and key before retrying the model", async () => {
    tauriMock.getOnlineAsrApiKey
      .mockResolvedValueOnce("local-engine-key")
      .mockResolvedValue("glm-engine-key");
    tauriMock.getOnlineAsrEndpoint
      .mockResolvedValueOnce({ region: "international", url: "https://local.example" })
      .mockResolvedValue({ region: "international", url: "https://glm.example" });
    tauriMock.setEngine.mockResolvedValue("glm-asr");

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).not.toBeDisabled());
    fireEvent.click(engineTrigger);
    const engineList = screen.getByRole("listbox");
    fireEvent.click(within(engineList).getByRole("option", { name: /GLM-ASR/ }));

    await waitFor(() => {
      expect(tauriMock.setEngine).toHaveBeenCalledWith("glm-asr");
      expect(engineTrigger).toHaveTextContent("GLM-ASR");
      expect(recordingContextMock.retryModel).toHaveBeenCalledTimes(1);
    });
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(2);
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect(screen.getByPlaceholderText("GLM API Key")).toHaveValue("glm-engine-key");
    expect(screen.getByText("https://glm.example")).toBeInTheDocument();
    expect(toastMock.success).toHaveBeenCalledWith("Switched to GLM-ASR engine");
  });

  it("keeps the current engine and reports a failed engine switch", async () => {
    tauriMock.setEngine.mockRejectedValueOnce(new Error("engine unavailable"));

    await renderSettings();

    const engineSection = screen.getByRole("heading", { name: "ASR Engine" }).closest("section");
    expect(engineSection).not.toBeNull();
    const engineTrigger = within(engineSection!).getByRole("button", { name: "ASR Engine" });
    await waitFor(() => expect(engineTrigger).not.toBeDisabled());
    fireEvent.click(engineTrigger);
    const engineList = screen.getByRole("listbox");
    fireEvent.click(within(engineList).getByRole("option", { name: /Confucius4-R2T2 Q8/ }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("Failed to switch engine"));
    expect(tauriMock.setEngine).toHaveBeenCalledWith("confucius4-r2t2");
    expect(engineTrigger).toHaveTextContent("Qwen3-ASR 0.6B Q8");
    expect(recordingContextMock.retryModel).not.toHaveBeenCalled();
  });

  it("persists a selected microphone and reloads the device selection", async () => {
    const devices = [
      { name: "Built-in Microphone", isDefault: true },
      { name: "USB Microphone", isDefault: false },
      { name: "Headset Microphone", isDefault: false },
    ];
    const updatedDevices = devices;
    tauriMock.listInputDevices
      .mockResolvedValueOnce({ devices, selectedDeviceName: "USB Microphone" })
      .mockResolvedValue({ devices: updatedDevices, selectedDeviceName: "Headset Microphone" });

    await renderSettings();

    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).not.toBeDisabled());
    fireEvent.click(microphoneTrigger);
    const deviceList = screen.getByRole("listbox");
    fireEvent.click(within(deviceList).getByRole("option", { name: /^Headset Microphone/ }));

    await waitFor(() => {
      expect(tauriMock.setInputDevice).toHaveBeenCalledWith("Headset Microphone");
      expect(tauriMock.listInputDevices).toHaveBeenCalledTimes(2);
      expect(microphoneTrigger).toHaveTextContent("Headset Microphone");
    });
    expect(storageMock.writeLocalStorage).toHaveBeenCalledWith(
      INPUT_DEVICE_STORAGE_KEY,
      "Headset Microphone",
    );
  });

  it("reports a failed microphone change without replacing the current selection", async () => {
    tauriMock.listInputDevices.mockResolvedValue({
      devices: [
        { name: "Built-in Microphone", isDefault: true },
        { name: "USB Microphone", isDefault: false },
        { name: "Headset Microphone", isDefault: false },
      ],
      selectedDeviceName: "USB Microphone",
    });
    tauriMock.setInputDevice.mockRejectedValueOnce(new Error("device unavailable"));

    await renderSettings();

    const microphoneTrigger = screen.getByRole("button", { name: "Select Microphone" });
    await waitFor(() => expect(microphoneTrigger).not.toBeDisabled());
    fireEvent.click(microphoneTrigger);
    fireEvent.click(within(screen.getByRole("listbox")).getByRole("option", { name: /^Headset Microphone/ }));

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("Failed to switch microphone"));
    expect(tauriMock.setInputDevice).toHaveBeenCalledWith("Headset Microphone");
    expect(tauriMock.listInputDevices).toHaveBeenCalledTimes(1);
    expect(microphoneTrigger).toHaveTextContent("USB Microphone");
    expect(storageMock.writeLocalStorage).not.toHaveBeenCalledWith(
      INPUT_DEVICE_STORAGE_KEY,
      "Headset Microphone",
    );
  });

  it("starts and stops the level monitor with the toggle and on unmount", async () => {
    const rendered = await renderSettings();
    await waitFor(() => expect(tauriMock.stopMicrophoneLevelMonitor).toHaveBeenCalled());

    const monitorToggle = screen.getByRole("switch", { name: "Microphone Level Monitor" });
    await act(async () => {
      fireEvent.click(monitorToggle);
      await Promise.resolve();
    });
    await waitFor(() => expect(tauriMock.startMicrophoneLevelMonitor).toHaveBeenCalledTimes(1));
    expect(storageMock.writeLocalStorage).toHaveBeenCalledWith(
      MIC_LEVEL_MONITOR_ENABLED_KEY,
      "true",
    );

    await act(async () => {
      fireEvent.click(monitorToggle);
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(monitorToggle).toHaveAttribute("aria-checked", "false");
      expect(tauriMock.startMicrophoneLevelMonitor).toHaveBeenCalledTimes(1);
    });

    await waitFor(() => expect(eventDisposers.length).toBeGreaterThanOrEqual(4));
    const disposers = [...eventDisposers];
    const stopCallsBeforeUnmount = tauriMock.stopMicrophoneLevelMonitor.mock.calls.length;
    rendered.unmount();
    await waitFor(() => {
      expect(tauriMock.stopMicrophoneLevelMonitor.mock.calls.length).toBeGreaterThan(stopCallsBeforeUnmount);
      for (const disposer of disposers) {
        expect(disposer).toHaveBeenCalled();
      }
    });
  });

  it("does not start the level monitor while the settings page is inactive", async () => {
    storageMock.readLocalStorage.mockImplementation((key: string) => (
      key === MIC_LEVEL_MONITOR_ENABLED_KEY ? "true" : null
    ));
    const rendered = await renderSettings(baseProfile, false);

    await waitFor(() => expect(tauriMock.stopMicrophoneLevelMonitor).toHaveBeenCalled());
    await waitFor(() => expect(eventDisposers.length).toBeGreaterThanOrEqual(2));
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(tauriMock.startMicrophoneLevelMonitor).not.toHaveBeenCalled();

    const disposers = [...eventDisposers];
    const stopCallsBeforeUnmount = tauriMock.stopMicrophoneLevelMonitor.mock.calls.length;
    rendered.unmount();
    await waitFor(() => {
      expect(tauriMock.stopMicrophoneLevelMonitor.mock.calls.length).toBeGreaterThan(stopCallsBeforeUnmount);
      for (const disposer of disposers) {
        expect(disposer).toHaveBeenCalled();
      }
    });
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function asrSection() {
  return within(screen.getByRole("heading", { name: "ASR Engine" }).closest("section")!);
}

async function renderAsrSettings() {
  const rendered = await renderSettings();
  await act(async () => { await Promise.resolve(); });
  vi.useFakeTimers();
  return rendered;
}

async function selectAsrEngine(name: RegExp) {
  fireEvent.click(asrSection().getByRole("button", { name: "ASR Engine" }));
  await act(async () => {
    fireEvent.click(within(screen.getByRole("listbox")).getByRole("option", { name }));
  });
}

describe("SettingsPage ASR credential identity and recovery", () => {
  const domestic = { region: "domestic", url: "https://cn.example" };

  beforeEach(() => {
    tauriMock.setEngine.mockImplementation(async (engine: string) => engine);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("disables Alibaba key and region controls until the initial endpoint identifies the slot", async () => {
    tauriMock.getEngine.mockResolvedValue("alibaba-asr");
    tauriMock.getOnlineAsrApiKey.mockResolvedValue("FAKE_STORED_CN_KEY");
    const endpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const rendered = await renderAsrSettings();
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect.soft(key).toBeDisabled();
    expect.soft(asrSection().getByRole("button", { name: "Domestic" })).toBeDisabled();
    expect.soft(asrSection().getByRole("button", { name: "International" })).toBeDisabled();
    expect(tauriMock.setOnlineAsrApiKey).not.toHaveBeenCalled();

    await act(async () => { endpoint.resolve(domestic); });
    expect(key).not.toBeDisabled();
    expect(asrSection().getByText(domestic.url)).toBeInTheDocument();
    fireEvent.change(key, { target: { value: "FAKE_CONFIRMED_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(599); });
    expect(tauriMock.setOnlineAsrApiKey).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_CONFIRMED_CN_KEY", "alibaba-asr-cn-api-key");
    fireEvent.change(key, { target: { value: "" } });
    rendered.unmount();
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenNthCalledWith(2, "", "alibaba-asr-cn-api-key");
  });

  it("offers an inline retry after initial endpoint failure and restores domestic key editing", async () => {
    tauriMock.getEngine.mockResolvedValue("alibaba-asr");
    tauriMock.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("endpoint unavailable"));
    const retryEndpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(retryEndpoint.promise);
    await renderAsrSettings();
    const retry = asrSection().getByRole("button", { name: /retry/i });
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect(retry).not.toBeDisabled();
    expect(key).toBeDisabled();
    expect(asrSection().getByRole("button", { name: "Domestic" })).toBeDisabled();

    await act(async () => { fireEvent.click(retry); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect(key).toBeDisabled();
    await act(async () => { retryEndpoint.reject(new Error("endpoint still unavailable")); });
    const retryAgain = asrSection().getByRole("button", { name: /retry/i });
    expect(retryAgain).not.toBeDisabled();
    expect(key).toBeDisabled();
    tauriMock.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    await act(async () => { fireEvent.click(retryAgain); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(3);
    expect(key).not.toBeDisabled();
    expect(asrSection().queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
    fireEvent.change(key, { target: { value: "FAKE_RECOVERED_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_RECOVERED_CN_KEY", "alibaba-asr-cn-api-key");
    expect(tauriMock.setEngine).not.toHaveBeenCalled();
  });

  it("retains a successful engine switch and retries failed endpoint reads without repeating setEngine", async () => {
    tauriMock.getEngine.mockResolvedValueOnce("glm-asr").mockResolvedValue("alibaba-asr");
    tauriMock.getOnlineAsrApiKey.mockResolvedValueOnce("FAKE_GLM_KEY").mockResolvedValue("FAKE_CN_KEY");
    tauriMock.getOnlineAsrEndpoint
      .mockResolvedValueOnce({ region: "international", url: "https://glm.example" })
      .mockRejectedValueOnce(new Error("endpoint unavailable"));
    await renderAsrSettings();
    await selectAsrEngine(/Alibaba DashScope/);
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).toHaveTextContent("Alibaba DashScope");
    expect.soft(screen.getByPlaceholderText("Alibaba API Key")).toBeDisabled();
    const retry = asrSection().getByRole("button", { name: /retry/i });
    expect(retry).not.toBeDisabled();
    tauriMock.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    await act(async () => { fireEvent.click(retry); });
    expect(tauriMock.setEngine).toHaveBeenCalledExactlyOnceWith("alibaba-asr");
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(3);
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect(key).not.toBeDisabled();
    expect(asrSection().getByText(domestic.url)).toBeInTheDocument();
    fireEvent.change(key, { target: { value: "FAKE_RETRIED_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_RETRIED_CN_KEY", "alibaba-asr-cn-api-key");
  });

  it("recovers the actual engine through an inline retry after the initial engine read rejects", async () => {
    const initialEngine = deferred<string>();
    tauriMock.getEngine.mockReturnValueOnce(initialEngine.promise).mockResolvedValue("alibaba-asr");
    tauriMock.getOnlineAsrEndpoint.mockResolvedValue(domestic);
    await renderAsrSettings();
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).toBeDisabled();
    await act(async () => { initialEngine.reject(new Error("engine unavailable")); });
    const retry = asrSection().getByRole("button", { name: /retry/i });
    expect(retry).not.toBeDisabled();
    await act(async () => { fireEvent.click(retry); });
    expect(tauriMock.getEngine).toHaveBeenCalledTimes(2);
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).toHaveTextContent("Alibaba DashScope");
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect(key).not.toBeDisabled();
    fireEvent.change(key, { target: { value: "FAKE_ACTUAL_ENGINE_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_ACTUAL_ENGINE_CN_KEY", "alibaba-asr-cn-api-key");
    expect(tauriMock.setEngine).not.toHaveBeenCalled();
  });

  it("clears the old provider key while preserving a successful domestic endpoint when its key read fails", async () => {
    tauriMock.getEngine.mockResolvedValue("glm-asr");
    tauriMock.getOnlineAsrApiKey.mockResolvedValueOnce("FAKE_OLD_GLM_KEY").mockRejectedValueOnce(new Error("keyring unavailable"));
    tauriMock.getOnlineAsrEndpoint
      .mockResolvedValueOnce({ region: "international", url: "https://glm.example" })
      .mockResolvedValueOnce(domestic);
    await renderAsrSettings();
    await selectAsrEngine(/Alibaba DashScope/);
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect(key).toHaveValue("");
    expect(key).not.toBeDisabled();
    expect.soft(asrSection().queryByText(domestic.url)).toBeInTheDocument();
    fireEvent.change(key, { target: { value: "FAKE_MANUAL_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_MANUAL_CN_KEY", "alibaba-asr-cn-api-key");
  });

  it.each(["pending", "failed"] as const)("keeps GLM key editing available while its endpoint is %s", async (readState) => {
    tauriMock.getEngine.mockResolvedValue("glm-asr");
    const endpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    await renderAsrSettings();
    if (readState === "failed") {
      await act(async () => { endpoint.reject(new Error("endpoint unavailable")); });
    }
    const key = screen.getByPlaceholderText("GLM API Key");
    expect(key).not.toBeDisabled();
    expect.soft(asrSection().getByRole("button", { name: "Domestic" })).toBeDisabled();
    const retryKey = deferred<string>();
    const retryEndpoint = deferred<typeof domestic>();
    if (readState === "failed") {
      fireEvent.change(key, { target: { value: "FAKE_GLM_PENDING_DRAFT" } });
      tauriMock.getOnlineAsrApiKey.mockReturnValueOnce(retryKey.promise);
      tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(retryEndpoint.promise);
      await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
      expect(key).not.toBeDisabled();
      expect(key).toHaveValue("FAKE_GLM_PENDING_DRAFT");
    }
    fireEvent.change(key, { target: { value: "FAKE_GLM_SHARED_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_GLM_SHARED_KEY", "glm-asr-api-key");
    if (readState === "failed") {
      await act(async () => {
        retryKey.resolve("FAKE_OLD_GLM_KEY");
        retryEndpoint.resolve(domestic);
      });
      expect(key).toHaveValue("FAKE_GLM_SHARED_KEY");
      expect(key).not.toBeDisabled();
      expect(tauriMock.setEngine).not.toHaveBeenCalled();
    }
  });

  it("keeps a valid GLM draft when retry returns an old key without any subsequent user edit", async () => {
    tauriMock.getEngine.mockResolvedValue("glm-asr");
    tauriMock.getOnlineAsrApiKey.mockResolvedValueOnce("FAKE_STORED_GLM_KEY");
    tauriMock.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("endpoint unavailable"));
    await renderAsrSettings();
    const key = screen.getByPlaceholderText("GLM API Key");
    expect(key).not.toBeDisabled();
    fireEvent.change(key, { target: { value: "FAKE_PENDING_GLM_DRAFT" } });
    const retryKey = deferred<string>();
    const retryEndpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrApiKey.mockReturnValueOnce(retryKey.promise);
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(retryEndpoint.promise);
    await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
    expect(key).toHaveValue("FAKE_PENDING_GLM_DRAFT");
    expect(key).not.toBeDisabled();
    await act(async () => {
      retryKey.resolve("FAKE_STORED_GLM_KEY");
      retryEndpoint.resolve(domestic);
    });
    expect(key).toHaveValue("FAKE_PENDING_GLM_DRAFT");
    await act(async () => { await vi.advanceTimersByTimeAsync(599); });
    expect(tauriMock.setOnlineAsrApiKey).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_PENDING_GLM_DRAFT", "glm-asr-api-key");
    expect(tauriMock.setEngine).not.toHaveBeenCalled();
  });

  it.each(["pending", "failed"] as const)("keeps local-engine controls available while the unrelated endpoint is %s", async (readState) => {
    const endpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    await renderAsrSettings();
    if (readState === "failed") {
      await act(async () => { endpoint.reject(new Error("endpoint unavailable")); });
    }
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).not.toBeDisabled();
    expect(asrSection().getByRole("button", { name: "Change" })).not.toBeDisabled();
    await selectAsrEngine(/Confucius4-R2T2 Q8/);
    expect(tauriMock.setEngine).toHaveBeenCalledExactlyOnceWith("confucius4-r2t2");
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).toHaveTextContent("Confucius4-R2T2 Q8");
    expect(recordingContextMock.retryModel).toHaveBeenCalledTimes(1);
  });

  it("ignores an old retry endpoint that arrives after a successful engine switch", async () => {
    tauriMock.getEngine.mockResolvedValue("glm-asr");
    tauriMock.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("endpoint unavailable"));
    await renderAsrSettings();
    const oldEndpoint = deferred<typeof domestic>();
    tauriMock.getOnlineAsrEndpoint.mockReturnValueOnce(oldEndpoint.promise).mockResolvedValueOnce(domestic);
    await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).not.toBeDisabled();
    await selectAsrEngine(/Alibaba DashScope/);
    await act(async () => { oldEndpoint.resolve({ region: "international", url: "https://obsolete.example" }); });
    expect(asrSection().getByRole("button", { name: "ASR Engine" })).toHaveTextContent("Alibaba DashScope");
    expect(asrSection().getByText(domestic.url)).toBeInTheDocument();
    expect(asrSection().queryByText("https://obsolete.example")).not.toBeInTheDocument();
    const key = screen.getByPlaceholderText("Alibaba API Key");
    expect(key).not.toBeDisabled();
    fireEvent.change(key, { target: { value: "FAKE_CURRENT_CN_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_CURRENT_CN_KEY", "alibaba-asr-cn-api-key");
    expect(tauriMock.setEngine).toHaveBeenCalledExactlyOnceWith("alibaba-asr");
  });
});

describe("SettingsPage ASR retry progress ownership", () => {
  beforeEach(() => {
    tauriMock.setEngine.mockImplementation(async (engine: string) => engine);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it.each([
    { context: "the current config", supersededRetry: false },
    { context: "a new config while an older retry finishes", supersededRetry: true },
  ])("keeps Retry disabled during a pending retry for $context", async ({ supersededRetry }) => {
    const recoveredEndpoint = { region: "international", url: "https://glm-recovered.example" };
    const obsoleteEndpoint = deferred<typeof recoveredEndpoint>();
    tauriMock.getEngine.mockResolvedValue(supersededRetry ? "alibaba-asr" : "glm-asr");
    tauriMock.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("initial endpoint unavailable"));
    if (supersededRetry) {
      tauriMock.getOnlineAsrApiKey.mockResolvedValueOnce("FAKE_PREVIOUS_ALIBABA_KEY");
    } else {
      tauriMock.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("initial key unavailable"));
    }
    await renderAsrSettings();

    if (supersededRetry) {
      tauriMock.getOnlineAsrEndpoint
        .mockReturnValueOnce(obsoleteEndpoint.promise)
        .mockRejectedValueOnce(new Error("new config endpoint unavailable"));
      tauriMock.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("new config key unavailable"));
      await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
      await selectAsrEngine(/GLM-ASR/);
      expect(tauriMock.setEngine).toHaveBeenCalledExactlyOnceWith("glm-asr");
      expect(asrSection().getByRole("button", { name: /retry/i })).not.toBeDisabled();
    }

    const initialEndpointReads = supersededRetry ? 3 : 1;
    const initialKeyReads = supersededRetry ? 2 : 1;
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(initialEndpointReads);
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(initialKeyReads);
    const firstEndpoint = deferred<typeof recoveredEndpoint>();
    const firstKey = deferred<string>();
    const currentEndpoint = deferred<typeof recoveredEndpoint>();
    const recoveredKey = deferred<string>();
    tauriMock.getOnlineAsrEndpoint
      .mockReturnValueOnce(firstEndpoint.promise)
      .mockReturnValueOnce(currentEndpoint.promise);
    tauriMock.getOnlineAsrApiKey
      .mockReturnValueOnce(firstKey.promise)
      .mockReturnValueOnce(recoveredKey.promise);
    await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(initialEndpointReads + 1);
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(initialKeyReads + 1);

    // A rejected batch permits retrying its endpoint while its sibling key read is still pending.
    await act(async () => { firstEndpoint.reject(new Error("retry endpoint unavailable")); });
    expect(asrSection().getByRole("button", { name: /retry/i })).not.toBeDisabled();
    await act(async () => { fireEvent.click(asrSection().getByRole("button", { name: /retry/i })); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(initialEndpointReads + 2);
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(initialKeyReads + 1);
    await act(async () => { firstKey.reject(new Error("older retry key unavailable")); });

    const retry = asrSection().getByRole("button", { name: /retry/i });
    const key = screen.getByPlaceholderText("GLM API Key");
    const engineTrigger = asrSection().getByRole("button", { name: "ASR Engine" });
    expect(key).not.toBeDisabled();
    expect(engineTrigger).not.toBeDisabled();
    expect.soft(retry).toBeDisabled();
    if (supersededRetry) {
      await act(async () => {
        obsoleteEndpoint.resolve({ region: "domestic", url: "https://obsolete.example" });
      });
      expect.soft(retry).toBeDisabled();
      expect(asrSection().queryByText("https://obsolete.example")).not.toBeInTheDocument();
    }
    fireEvent.click(engineTrigger);
    expect(within(screen.getByRole("listbox")).getByRole("option", { name: /Alibaba DashScope/ })).toBeInTheDocument();
    fireEvent.click(engineTrigger);
    await act(async () => { fireEvent.click(retry); });
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(initialEndpointReads + 2);
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(initialKeyReads + 1);

    await act(async () => { currentEndpoint.resolve(recoveredEndpoint); });
    expect(retry).not.toBeDisabled();
    expect(asrSection().getByText(recoveredEndpoint.url)).toBeInTheDocument();
    await act(async () => { fireEvent.click(retry); });
    expect(tauriMock.getOnlineAsrApiKey).toHaveBeenCalledTimes(initialKeyReads + 2);
    expect(tauriMock.getOnlineAsrEndpoint).toHaveBeenCalledTimes(initialEndpointReads + 2);
    await act(async () => { recoveredKey.resolve("FAKE_RECOVERED_GLM_KEY"); });
    expect(key).toHaveValue("FAKE_RECOVERED_GLM_KEY");
    expect(asrSection().queryByRole("button", { name: /retry/i })).not.toBeInTheDocument();
    fireEvent.change(key, { target: { value: "FAKE_EDITED_GLM_KEY" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(tauriMock.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_EDITED_GLM_KEY", "glm-asr-api-key");
  });
});
