import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelInfo, UserProfile } from "@/types";

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
  getSelectionApiKey: vi.fn(),
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
  removeChatgptAccount: vi.fn(),
  openAppReleasePage: vi.fn(),
  pasteText: vi.fn(),
  pickFolder: vi.fn(),
  removeCorrection: vi.fn(),
  removeCustomProvider: vi.fn(),
  removeHotWord: vi.fn(),
  setAiPolishConfig: vi.fn(),
  saveAiPolishApiKey: vi.fn(),
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
  setSelectionApiKey: vi.fn(),
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
  "common.refresh": "Refresh",
  "settings.addCustomProvider": "Add Custom Provider",
  "settings.apiFormatLabel": "API Format",
  "settings.apiKey": "API Key",
  "settings.assistantApiKey": "Assistant API Key",
  "settings.assistantModelLabel": "Assistant model name",
  "settings.assistantProvider": "Assistant Provider",
  "settings.assistantSeparateConfig": "Assistant uses separate config",
  "settings.baseUrlLabel": "Base URL",
  "settings.defaultModelLabel": "Provider default model",
  "settings.fetching": "Fetching...",
  "settings.modelNameLabel": "Model name",
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
};

function translate(key: string) {
  return labels[key] ?? key;
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
  tauriMock.getSelectionApiKey.mockResolvedValue("");
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
  tauriMock.saveAiPolishApiKey.mockResolvedValue(undefined);
  tauriMock.setLlmProviderConfig.mockResolvedValue(undefined);
  tauriMock.addCustomProvider.mockResolvedValue("custom-provider");
  appMock.getVersion.mockReset();
  appMock.getVersion.mockResolvedValue("1.5.9");
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
}

async function renderSettings(profile: UserProfile = baseProfile) {
  tauriMock.getUserProfile.mockReset();
  tauriMock.getUserProfile.mockResolvedValue(profile);
  const { default: SettingsPage } = await import("@/pages/SettingsPage");
  render(<SettingsPage active onNavigate={vi.fn()} />);
  await waitFor(() => expect(tauriMock.getUserProfile).toHaveBeenCalledWith());
  await waitFor(() => expect(tauriMock.getAiPolishApiKey).toHaveBeenCalledTimes(1));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

function openProviderPicker() {
  fireEvent.click(screen.getByRole("button", { name: "Select LLM Provider" }));
  return screen.getByRole("listbox");
}

function openAssistantProviderPicker() {
  fireEvent.click(screen.getByRole("button", { name: "Assistant Provider" }));
  return screen.getByRole("listbox");
}

async function chooseProvider(listbox: HTMLElement, name: string) {
  await act(async () => {
    fireEvent.click(within(listbox).getByRole("option", { name: new RegExp(`^${name}`) }));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function modelRefreshButton(listboxName: string) {
  const listbox = screen.getByRole("listbox", { name: listboxName });
  const popover = listbox.closest<HTMLElement>(".picker-popover");
  expect(popover).not.toBeNull();
  return within(popover!).getByRole("button", { name: "Refresh" });
}

function modelOption(name: string) {
  const matcher = new RegExp(`^${name}`);
  const option = screen.queryByRole("option", { name: matcher });
  const button = screen.queryByRole("button", { name: matcher });
  if (option) return option;
  if (button) return button;
  throw new Error(`Model option ${name} is not rendered`);
}

function queryModelOption(name: string) {
  const matcher = new RegExp(`^${name}`);
  return screen.queryByRole("option", { name: matcher })
    ?? screen.queryByRole("button", { name: matcher });
}

function modelInfo(id: string, ownedBy = "test-owner"): AiModelInfo {
  return { id, ownedBy };
}

beforeEach(() => resetMocks());
afterEach(() => vi.clearAllMocks());

describe("SettingsPage provider configuration saves", () => {
  it("persists a normal polish model edit after its debounce", async () => {
    await renderSettings();

    fireEvent.change(screen.getByRole("textbox", { name: "Model name" }), {
      target: { value: "edited-polish-model" },
    });

    await waitFor(() => {
      expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
      expect(tauriMock.setLlmProviderConfig.mock.calls[0][0]).toBe("cerebras");
      expect(tauriMock.setLlmProviderConfig.mock.calls[0][2]).toBe("edited-polish-model");
    }, { timeout: 1500 });
  });

  it("persists a normal independent assistant model edit after its debounce", async () => {
    const profile: UserProfile = {
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        assistant_model: "deepseek-v4-pro",
        assistant_provider: "deepseek",
        assistant_use_separate_model: true,
      },
    };
    await renderSettings(profile);

    fireEvent.change(screen.getByRole("textbox", { name: "Assistant model name" }), {
      target: { value: "edited-assistant-model" },
    });

    await waitFor(() => {
      expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
      const args = tauriMock.setLlmProviderConfig.mock.calls[0];
      expect(args[0]).toBe("cerebras");
      expect(args[5]).toBe(true);
      expect(args[6]).toBe("edited-assistant-model");
      expect(args[7]).toBe("deepseek");
    }, { timeout: 1500 });
  });

  it("cancels a queued polish save before immediately persisting a provider switch", async () => {
    await renderSettings();

    fireEvent.change(screen.getByRole("textbox", { name: "Model name" }), {
      target: { value: "stale-polish-model" },
    });
    await chooseProvider(openProviderPicker(), "OpenAI");

    await waitFor(() => {
      expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
      expect(tauriMock.setLlmProviderConfig.mock.calls[0][0]).toBe("openai");
    });
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
    expect(tauriMock.setLlmProviderConfig.mock.calls[0][2]).toBe("gpt-4.1-mini");
  });

  it("cancels a queued assistant save before immediately persisting an assistant provider switch", async () => {
    const profile: UserProfile = {
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        assistant_model: "deepseek-v4-pro",
        assistant_provider: "deepseek",
        assistant_use_separate_model: true,
      },
    };
    await renderSettings(profile);

    fireEvent.change(screen.getByRole("textbox", { name: "Assistant model name" }), {
      target: { value: "stale-assistant-model" },
    });
    await chooseProvider(openAssistantProviderPicker(), "OpenAI");

    await waitFor(() => {
      expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
      expect(tauriMock.setLlmProviderConfig.mock.calls[0][7]).toBe("openai");
    });
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
    expect(tauriMock.setLlmProviderConfig.mock.calls[0][6]).toBe("gpt-4.1-mini");
  });

  it("cancels a queued save before adding and immediately selecting a custom provider", async () => {
    await renderSettings();

    fireEvent.change(screen.getByRole("textbox", { name: "Model name" }), {
      target: { value: "stale-before-custom-provider" },
    });
    const picker = openProviderPicker();
    fireEvent.click(within(picker).getByRole("option", { name: "Add Custom Provider" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Provider name" }), {
      target: { value: "Acme" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Provider Base URL" }), {
      target: { value: "https://api.acme.test" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Provider default model" }), {
      target: { value: "acme-model" },
    });
    tauriMock.addCustomProvider.mockResolvedValueOnce("acme");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Add" }));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    await waitFor(() => {
      expect(tauriMock.addCustomProvider).toHaveBeenCalledWith(
        "Acme",
        "https://api.acme.test",
        "acme-model",
        "openai_compat",
      );
      expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
      expect(tauriMock.setLlmProviderConfig.mock.calls[0][0]).toBe("acme");
    });
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(tauriMock.setLlmProviderConfig).toHaveBeenCalledTimes(1);
  });
});

describe("SettingsPage model discovery refresh", () => {
  it("automatically discovers polish models after the API key resolves", async () => {
    const keyRequest = deferred<string>();
    tauriMock.getAiPolishApiKey.mockReset().mockReturnValueOnce(keyRequest.promise);
    tauriMock.listAiModels.mockResolvedValue({
      models: [modelInfo("automatic-polish-model")],
      sourceUrl: "https://models.test/automatic-polish",
    });
    await renderSettings();

    await act(async () => {
      keyRequest.resolve("polish-key");
      await keyRequest.promise;
    });
    await waitFor(() => expect(screen.getByPlaceholderText("Cerebras API Key")).toHaveValue("polish-key"));
    await waitFor(() => {
      expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
      const args = tauriMock.listAiModels.mock.calls[0];
      expect(args[0]).toBe("cerebras");
      expect(args[1]).toBe("https://api.cerebras.ai");
      expect(args[2]).toBe("polish-key");
      expect(args[3]).toBe(false);
    }, { timeout: 1500 });

    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    await waitFor(() => expect(modelOption("automatic-polish-model")).toBeInTheDocument());
  });

  it("automatically discovers independent assistant models after the API key resolves", async () => {
    const profile: UserProfile = {
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        assistant_model: "deepseek-v4-pro",
        assistant_provider: "deepseek",
        assistant_use_separate_model: true,
      },
    };
    const assistantKeyRequest = deferred<string>();
    tauriMock.getAssistantApiKey.mockReset().mockReturnValueOnce(assistantKeyRequest.promise);
    tauriMock.listAiModels.mockResolvedValue({
      models: [modelInfo("automatic-assistant-model")],
      sourceUrl: "https://models.test/automatic-assistant",
    });
    await renderSettings(profile);

    await waitFor(() => expect(tauriMock.getAssistantApiKey).toHaveBeenCalledTimes(1));
    await act(async () => {
      assistantKeyRequest.resolve("assistant-key");
      await assistantKeyRequest.promise;
    });
    await waitFor(() => expect(screen.getByPlaceholderText("DeepSeek API Key")).toHaveValue("assistant-key"));
    await waitFor(() => {
      expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
      const args = tauriMock.listAiModels.mock.calls[0];
      expect(args[0]).toBe("deepseek");
      expect(args[1]).toBe("https://api.deepseek.com");
      expect(args[2]).toBe("assistant-key");
      expect(args[3]).toBe(false);
    }, { timeout: 1500 });

    fireEvent.click(screen.getByRole("button", { name: "Open assistant model list" }));
    await waitFor(() => expect(modelOption("automatic-assistant-model")).toBeInTheDocument());
  });

  it("cancels the queued polish discovery when Refresh is clicked", async () => {
    const keyRequest = deferred<string>();
    tauriMock.getAiPolishApiKey.mockReset().mockReturnValueOnce(keyRequest.promise);
    await renderSettings();

    await act(async () => {
      keyRequest.resolve("polish-key");
      await keyRequest.promise;
    });
    await waitFor(() => expect(screen.getByPlaceholderText("Cerebras API Key")).toHaveValue("polish-key"));
    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    tauriMock.listAiModels.mockResolvedValueOnce({
      models: [modelInfo("manual-polish-model")],
      sourceUrl: "https://models.test/polish",
    });
    fireEvent.click(modelRefreshButton("Open model list"));

    await waitFor(() => {
      expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
      expect(modelOption("manual-polish-model")).toBeInTheDocument();
    });
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
  });

  it("cancels the queued assistant discovery when Refresh is clicked", async () => {
    const profile: UserProfile = {
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        assistant_model: "deepseek-v4-pro",
        assistant_provider: "deepseek",
        assistant_use_separate_model: true,
      },
    };
    const assistantKeyRequest = deferred<string>();
    tauriMock.getAssistantApiKey.mockReset().mockReturnValueOnce(assistantKeyRequest.promise);
    await renderSettings(profile);

    await waitFor(() => expect(tauriMock.getAssistantApiKey).toHaveBeenCalledTimes(1));
    await act(async () => {
      assistantKeyRequest.resolve("assistant-key");
      await assistantKeyRequest.promise;
    });
    await waitFor(() => expect(screen.getByPlaceholderText("DeepSeek API Key")).toHaveValue("assistant-key"));
    fireEvent.click(screen.getByRole("button", { name: "Open assistant model list" }));
    tauriMock.listAiModels.mockResolvedValueOnce({
      models: [modelInfo("manual-assistant-model")],
      sourceUrl: "https://models.test/assistant",
    });
    fireEvent.click(modelRefreshButton("Open assistant model list"));

    await waitFor(() => {
      expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
      expect(modelOption("manual-assistant-model")).toBeInTheDocument();
    });
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
  });

  it("cancels the shared polish queue when Refresh is clicked from the assistant picker", async () => {
    const keyRequest = deferred<string>();
    tauriMock.getAiPolishApiKey.mockReset().mockReturnValueOnce(keyRequest.promise);
    await renderSettings();

    await act(async () => {
      keyRequest.resolve("polish-key");
      await keyRequest.promise;
    });
    await waitFor(() => expect(screen.getByPlaceholderText("Cerebras API Key")).toHaveValue("polish-key"));

    fireEvent.click(screen.getByRole("switch", { name: "Assistant uses separate config" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Open assistant model list" })).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Open assistant model list" }));
    tauriMock.listAiModels.mockResolvedValueOnce({
      models: [modelInfo("shared-manual-model")],
      sourceUrl: "https://models.test/shared",
    });
    fireEvent.click(modelRefreshButton("Open assistant model list"));

    await waitFor(() => {
      expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
      expect(modelOption("shared-manual-model")).toBeInTheDocument();
    });
    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1);
  });
});

describe("SettingsPage stale model responses", () => {
  it("does not let a late old-provider response replace the current provider model list", async () => {
    let resolveOld!: (payload: { models: AiModelInfo[]; sourceUrl: string }) => void;
    let resolveNew!: (payload: { models: AiModelInfo[]; sourceUrl: string }) => void;
    const oldRequest = new Promise<{ models: AiModelInfo[]; sourceUrl: string }>((resolve) => {
      resolveOld = resolve;
    });
    const newRequest = new Promise<{ models: AiModelInfo[]; sourceUrl: string }>((resolve) => {
      resolveNew = resolve;
    });
    tauriMock.getAiPolishApiKey
      .mockReset()
      .mockResolvedValueOnce("")
      .mockResolvedValue("polish-key");
    tauriMock.listAiModels.mockImplementation((provider: string) => (
      provider === "cerebras" ? oldRequest : newRequest
    ));
    await renderSettings();

    fireEvent.change(screen.getByPlaceholderText("Cerebras API Key"), {
      target: { value: "polish-key" },
    });
    await waitFor(() => expect(tauriMock.listAiModels).toHaveBeenCalledTimes(1), { timeout: 1500 });

    await chooseProvider(openProviderPicker(), "OpenAI");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Select LLM Provider" })).toHaveTextContent("OpenAI");
      expect(screen.getByPlaceholderText("OpenAI API Key")).toHaveValue("polish-key");
    });

    await waitFor(() => expect(tauriMock.listAiModels).toHaveBeenCalledTimes(2), { timeout: 1500 });

    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    await act(async () => {
      resolveNew({ models: [modelInfo("current-provider-model")], sourceUrl: "https://models.test/new" });
      await newRequest;
    });
    await waitFor(() => expect(modelOption("current-provider-model")).toBeInTheDocument());
    fireEvent.click(modelOption("current-provider-model"));

    await act(async () => {
      resolveOld({ models: [modelInfo("stale-provider-model")], sourceUrl: "https://models.test/old" });
      await oldRequest;
    });
    await waitFor(() => {
      expect(screen.getByRole("textbox", { name: "Model name" })).toHaveValue("current-provider-model");
      expect(screen.getByRole("button", { name: "Select LLM Provider" })).toHaveTextContent("OpenAI");
    });

    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    expect(modelOption("current-provider-model")).toBeInTheDocument();
    expect(queryModelOption("stale-provider-model")).not.toBeInTheDocument();
  });
});

describe("SettingsPage assistant model discovery errors", () => {
  it("keeps assistant discovery errors independent from polish discovery errors", async () => {
    const profile: UserProfile = {
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        assistant_model: "deepseek-v4-pro",
        assistant_provider: "deepseek",
        assistant_use_separate_model: true,
      },
    };
    tauriMock.getAiPolishApiKey.mockResolvedValue("polish-key");
    tauriMock.getAssistantApiKey.mockResolvedValue("assistant-key");
    tauriMock.listAiModels.mockImplementation((provider: string) => (
      Promise.reject(new Error(provider === "cerebras" ? "polish discovery failed" : "assistant discovery failed"))
    ));
    await renderSettings(profile);

    await waitFor(() => expect(tauriMock.listAiModels).toHaveBeenCalledTimes(2), { timeout: 1500 });

    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    await waitFor(() => expect(screen.getByText("polish discovery failed")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Open assistant model list" }));
    await waitFor(() => expect(screen.getByText("assistant discovery failed")).toBeInTheDocument());
  });
});

describe("SettingsPage ChatGPT plan authorization", () => {
  const accounts = [{ clientId: "oaiapp_first", email: "same@example.invalid" }, { clientId: "oaiapp_second", email: "same@example.invalid" }];
  const openaiProfile = { ...baseProfile, llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth" as const } };

  it("reuses the most recently used registration after signing out", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_second", savedAccounts: accounts });
    tauriMock.loginOpenaiCodexOauth.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_second", savedAccounts: accounts });
    await renderSettings(openaiProfile);
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: false, savedAccounts: accounts });
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthLogout" })[0]);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthLogin" })[0]).toBeEnabled());
    expect(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]).toHaveTextContent("p_second");
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthLogin" })[0]);
    await waitFor(() => expect(tauriMock.loginOpenaiCodexOauth).toHaveBeenCalledWith("oaiapp_second", false));
  });

  it.each([false, true])("removes only the chosen registration (active: %s)", async (active) => {
    const connected = { loggedIn: true, clientId: "oaiapp_first", savedAccounts: accounts };
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue(connected);
    const target = active ? "oaiapp_first" : "oaiapp_second";
    const remaining = { ...connected, loggedIn: !active, clientId: active ? null : "oaiapp_first", savedAccounts: accounts.filter((a) => a.clientId !== target) };
    tauriMock.removeChatgptAccount.mockImplementation(async () => {
      tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue(remaining);
      return remaining;
    });
    await renderSettings(openaiProfile);
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    const row = screen.getAllByText(target.slice(-8)).find((el) => el.closest(".picker-popover"))!.closest(".chatgpt-account-row");
    expect(row).not.toBeNull();
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "settings.chatgptAccountActions" }));
    expect(within(row as HTMLElement).getByText(active ? "settings.chatgptRemoveActiveHint" : "settings.chatgptRemoveHint")).toBeInTheDocument();
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: "settings.chatgptRemoveAccount" }));
    await waitFor(() => expect(tauriMock.removeChatgptAccount).toHaveBeenCalledWith(target));
    await waitFor(() => expect(screen.getAllByRole("button", { name: active ? "settings.codexOauthLogin" : "settings.codexOauthLogout" })[0]).toBeEnabled());
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    expect(screen.queryByText(target.slice(-8))).not.toBeInTheDocument();
    expect(screen.getAllByText((active ? "oaiapp_second" : "oaiapp_first").slice(-8)).length).toBeGreaterThan(0);
  });

  it("keeps a locally removed account gone when remote revocation fails", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_first", savedAccounts: accounts });
    tauriMock.removeChatgptAccount.mockImplementation(async () => {
      tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: false, savedAccounts: accounts.slice(1) });
      throw new Error("remote revocation unconfirmed");
    });
    await renderSettings(openaiProfile);
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    const row = screen.getByRole("button", { name: /same@example.invalid.*pp_first/ }).closest(".chatgpt-account-row") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "settings.chatgptAccountActions" }));
    fireEvent.click(within(row).getByRole("button", { name: "settings.chatgptRemoveAccount" }));
    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith("remote revocation unconfirmed"));
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthLogin" })[0]).toBeEnabled());
    expect(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]).toHaveTextContent("p_second");
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    expect(screen.queryByText("pp_first")).not.toBeInTheDocument();
  });

  it("returns focus to sign-in after removing the final record", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_first", savedAccounts: accounts.slice(0, 1) });
    tauriMock.removeChatgptAccount.mockImplementation(async () => {
      tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: false, savedAccounts: [] });
      return { loggedIn: false, savedAccounts: [] };
    });
    await renderSettings(openaiProfile);
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    fireEvent.click(screen.getByRole("button", { name: "settings.chatgptAccountActions" }));
    const remove = screen.getByRole("button", { name: "settings.chatgptRemoveAccount" });
    remove.focus();
    fireEvent.click(remove);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthLogin" })[0]).toHaveFocus());
  });

  it.each([true, false])("retries an issued registration after persistence failure (pending: %s)", async (pending) => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_first", savedAccounts: accounts.slice(0, 1) });
    tauriMock.loginOpenaiCodexOauth.mockImplementationOnce(async () => {
      tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_first", savedAccounts: [...accounts.slice(0, 1), { clientId: "oaiapp_pending", pending }] });
      throw new Error("token exchange failed");
    }).mockResolvedValue({ loggedIn: true, clientId: "oaiapp_pending", savedAccounts: [{ clientId: "oaiapp_pending" }] });
    await renderSettings(openaiProfile);
    fireEvent.click(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]);
    fireEvent.click(screen.getByRole("button", { name: "settings.chatgptAddAccount" }));
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthReauth" })[0]);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthReauth" })[0]).toBeEnabled());
    expect(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]).toHaveTextContent("oaiapp_pending");
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthReauth" })[0]);
    await waitFor(() => expect(tauriMock.loginOpenaiCodexOauth).toHaveBeenLastCalledWith("oaiapp_pending", false));
  });

  it.each(["option", "enter", "action"])("saves protocol IDs when selecting upstream model names via %s", async (method) => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ loggedIn: true, clientId: "oaiapp_first", planUsageEnabled: true });
    tauriMock.listAiModels.mockResolvedValue({
      models: [{ id: "gpt-6.1-sol", displayName: "GPT-6.1 Sol", ownedBy: "openai" }],
      sourceUrl: "https://api.openai.com/v1/models",
    });
    await renderSettings({ ...baseProfile, llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth" } });
    fireEvent.click(screen.getByRole("button", { name: "Open model list" }));
    const search = screen.getByRole("textbox", { name: "Search model" });
    fireEvent.change(search, { target: { value: "GPT-6.1 Sol" } });
    const option = await screen.findByRole("option", { name: /GPT-6\.1 Sol/ });
    if (method === "option") fireEvent.click(option);
    else if (method === "enter") fireEvent.keyDown(search, { key: "Enter" });
    else fireEvent.click(screen.getByRole("button", { name: /^settings\.useAsModel/ }));
    expect(screen.getByRole("textbox", { name: "Model name" })).toHaveValue("gpt-6.1-sol");
  });

  it("shows declined plan access and does not fetch a catalog with that identity", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({
      loggedIn: true, clientId: "oaiapp_denied", planUsageEnabled: false,
      savedAccounts: [{ clientId: "oaiapp_denied", email: "test@example.invalid" }],
    });
    await renderSettings({ ...baseProfile, llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth" } });
    await waitFor(() => expect(screen.getAllByText("settings.chatgptPlanPermissionMissing").length).toBeGreaterThan(0));
    expect(tauriMock.listAiModels).not.toHaveBeenCalled();
    expect(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })[0]).toHaveTextContent("test@example.invalid");
  });

  it("passes the selected saved registration to sign-in and refreshes logout state after revocation failure", async () => {
    const connected = { loggedIn: true, clientId: "oaiapp_first", planUsageEnabled: true,
      savedAccounts: [{clientId:"oaiapp_first",email:"same@example.invalid"},{clientId:"oaiapp_second",email:"same@example.invalid"}] };
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue(connected);
    tauriMock.loginOpenaiCodexOauth.mockResolvedValue({ ...connected, clientId: "oaiapp_second" });
    await renderSettings({ ...baseProfile, llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth" } });
    const [picker] = await screen.findAllByRole("button", { name: "settings.chatgptAccountPicker" });
    fireEvent.click(picker);
    fireEvent.click(screen.getByRole("button", { name: /same@example.invalid.*p_second/ }));
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthReauth" })[0]);
    await waitFor(() => expect(tauriMock.loginOpenaiCodexOauth).toHaveBeenCalledWith("oaiapp_second", false));
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthLogout" })[0]).toBeEnabled());
    tauriMock.logoutOpenaiCodexOauth.mockRejectedValue(new Error("remote revocation unconfirmed"));
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({ ...connected, loggedIn: false, clientId: null });
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthLogout" })[0]);
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.codexOauthLogin" })[0]).toBeInTheDocument());
    expect(toastMock.error).toHaveBeenCalledWith("remote revocation unconfirmed");
  });

  it("keeps the two account pickers exclusive and lets a new registration be selected", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({
      loggedIn: true, clientId: "oaiapp_first", planUsageEnabled: true,
      savedAccounts: [{ clientId: "oaiapp_first", email: "test@example.invalid" }],
    });
    tauriMock.loginOpenaiCodexOauth.mockResolvedValue({ loggedIn: false });
    await renderSettings({ ...baseProfile, llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth" } });
    const [polishPicker, assistantPicker] = await screen.findAllByRole("button", { name: "settings.chatgptAccountPicker" });
    fireEvent.click(polishPicker);
    expect(polishPicker).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(assistantPicker);
    expect(polishPicker).toHaveAttribute("aria-expanded", "false");
    expect(assistantPicker).toHaveAttribute("aria-expanded", "true");
    const addAccountOptions = screen.getAllByRole("button", { name: "settings.chatgptAddAccount" });
    fireEvent.click(addAccountOptions[addAccountOptions.length - 1]);
    expect(assistantPicker).toHaveTextContent("settings.chatgptAddAccount");
    fireEvent.click(screen.getAllByRole("button", { name: "settings.codexOauthReauth" })[0]);
    await waitFor(() => expect(tauriMock.loginOpenaiCodexOauth).toHaveBeenCalledWith(undefined, true));
  });

  it("keeps assistant and selection account popovers independent", async () => {
    tauriMock.getOpenaiCodexOauthStatus.mockResolvedValue({
      loggedIn: true, clientId: "oaiapp_first", planUsageEnabled: true,
      savedAccounts: [{ clientId: "oaiapp_first", email: "test@example.invalid" }],
    });
    await renderSettings({
      ...baseProfile,
      llm_provider: { ...baseProfile.llm_provider, active: "openai", openai_auth_mode: "oauth", selection_use_separate_model: true, selection_provider: "openai", selection_model: "gpt-6.1-sol" },
      selection_assistant: { enabled: true, translation_target: "English", excluded_apps: [], auto_screenshot: false },
    });
    await waitFor(() => expect(screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" })).toHaveLength(3));
    const pickers = screen.getAllByRole("button", { name: "settings.chatgptAccountPicker" });
    fireEvent.click(pickers[1]);
    expect(pickers[1]).toHaveAttribute("aria-expanded", "true");
    expect(pickers[2]).toHaveAttribute("aria-expanded", "false");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(pickers[1]).toHaveAttribute("aria-controls", screen.getByRole("dialog").id);
    fireEvent.click(pickers[2]);
    expect(pickers[1]).toHaveAttribute("aria-expanded", "false");
    expect(pickers[2]).toHaveAttribute("aria-expanded", "true");
  });
});
