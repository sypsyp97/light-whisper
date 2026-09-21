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
  tauriMock.getAlibabaAsrConfig.mockResolvedValue({
    model: "qwen3-asr-flash",
    models: ["qwen3-asr-flash"],
    region: "international",
    url: "https://dashscope-intl.aliyuncs.com",
  });
  tauriMock.getEngine.mockResolvedValue("qwen3-asr-0.6b");
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
