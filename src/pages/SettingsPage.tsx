import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power, Keyboard, ClipboardPaste, AudioLines, Zap, Sparkles, BookOpen, Plus, X, Download, Upload, Check, ChevronsUpDown, Languages, Globe, Trash2, FolderOpen, RotateCcw, HardDrive } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/hooks/useTheme";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";
import {
  checkAppUpdate,
  disableAutostart,
  enableAutostart,
  getEngine,
  isAutostartEnabled,
  openAppReleasePage,
  pasteText,
  setEngine,
  testMicrophone,
  setInputMethodCommand,
  setAiPolishConfig,
  setAiPolishScreenContextEnabled,
  getAiPolishApiKey,
  getUserProfile,
  addHotWord,
  listAiModels,
  removeHotWord,
  setLlmProviderConfig,
  exportUserProfile,
  listInputDevices,
  importUserProfile,
  setInputDevice,
  setSoundEnabled,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
  setTranslationTarget,
  setTranslationHotkey,
  setCustomPrompt,
  setRecordingMode,
  setOnlineAsrApiKey,
  getOnlineAsrApiKey,
  getOnlineAsrEndpoint,
  setOnlineAsrEndpoint,
  getModelsDir,
  pickFolder,
  setModelsDir,
  addCustomProvider,
  removeCustomProvider,
  setAssistantHotkey,
  setAssistantScreenContextEnabled,
  setAssistantSystemPrompt,
  getLlmReasoningSupport,
  setAssistantApiKey,
  getAssistantApiKey,
  setWebSearchConfig,
  setWebSearchApiKey,
  getWebSearchApiKey,
} from "@/api/tauri";
import type { AiModelInfo, CustomProvider, InputDeviceInfo, UserProfile, ApiFormat, LlmReasoningMode, LlmReasoningSupport, WebSearchProvider } from "@/types";
import { useRecordingContext } from "@/contexts/RecordingContext";
import SecretInput from "@/components/SecretInput";
import TitleBar from "@/components/TitleBar";
import { PADDING, INPUT_METHOD_KEY, INPUT_DEVICE_STORAGE_KEY, DEFAULT_HOTKEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY, RECORDING_MODE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY, LANGUAGE_STORAGE_KEY } from "@/lib/constants";
import { formatHotkeyForDisplay } from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { useTranslation } from "react-i18next";

const themeOptions = [
  { mode: "light" as const, icon: Sun, labelKey: "settings.themeLight" },
  { mode: "dark" as const, icon: Moon, labelKey: "settings.themeDark" },
  { mode: "system" as const, icon: Monitor, labelKey: "settings.themeSystem" },
] as const;

const engineOptions = [
  { key: "sensevoice", icon: AudioLines, label: "SenseVoice", descKey: "settings.sensevoiceDesc" },
  { key: "whisper", icon: Zap, label: "Faster Whisper", descKey: "settings.whisperDesc" },
  { key: "glm-asr", icon: Globe, label: "GLM-ASR", descKey: "settings.glmAsrDesc" },
] as const;

const inputOptions = [
  { key: "sendInput" as const, icon: Keyboard, labelKey: "settings.directInput", descKey: "settings.directInputDesc" },
  { key: "clipboard" as const, icon: ClipboardPaste, labelKey: "settings.clipboardPaste", descKey: "settings.clipboardPasteDesc" },
];

const llmProviderOptions: ReadonlyArray<{
  key: string;
  label: string;
  labelKey?: string;
  descKey: string;
  baseUrl: string;
  defaultModel: string;
  models: readonly string[];
}> = [
  {
    key: "openai",
    label: "OpenAI",
    descKey: "settings.openaiDesc",
    baseUrl: "https://api.openai.com",
    defaultModel: "gpt-4.1-mini",
    models: ["gpt-4.1-mini", "gpt-4o-mini", "gpt-4.1"],
  },
  {
    key: "deepseek",
    label: "DeepSeek",
    descKey: "settings.deepseekDesc",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-chat",
    models: ["deepseek-chat", "deepseek-reasoner"],
  },
  {
    key: "cerebras",
    label: "Cerebras",
    descKey: "settings.cerebrasDesc",
    baseUrl: "https://api.cerebras.ai",
    defaultModel: "gpt-oss-120b",
    models: ["gpt-oss-120b", "gpt-oss-20b"],
  },
  {
    key: "siliconflow",
    label: "SiliconFlow",
    descKey: "settings.siliconflowDesc",
    baseUrl: "https://api.siliconflow.cn",
    defaultModel: "Qwen/Qwen3-32B",
    models: ["Qwen/Qwen3-32B", "deepseek-ai/DeepSeek-V3", "Qwen/Qwen2.5-7B-Instruct"],
  },
  {
    key: "custom",
    label: "custom",
    labelKey: "settings.customCompatLabel",
    descKey: "settings.customCompatDesc",
    baseUrl: "http://127.0.0.1:8000",
    defaultModel: "gpt-4.1-mini",
    models: ["gpt-4.1-mini", "gpt-4o-mini", "deepseek-chat"],
  },
];

const LLM_PROVIDER_DRAFTS_KEY = "light-whisper-llm-provider-drafts";

const reasoningModeOptions: Array<{
  key: LlmReasoningMode;
  labelKey: string;
  descKey: string;
}> = [
  { key: "provider_default", labelKey: "settings.reasoningDefault", descKey: "settings.reasoningDefaultDesc" },
  { key: "off", labelKey: "settings.reasoningOff", descKey: "settings.reasoningOffDesc" },
  { key: "light", labelKey: "settings.reasoningLight", descKey: "settings.reasoningLightDesc" },
  { key: "balanced", labelKey: "settings.reasoningBalanced", descKey: "settings.reasoningBalancedDesc" },
  { key: "deep", labelKey: "settings.reasoningDeep", descKey: "settings.reasoningDeepDesc" },
];

const recordingModeOptions: Array<{
  key: "hold" | "toggle";
  labelKey: string;
  descKey: string;
}> = [
  { key: "hold", labelKey: "settings.holdToTalk", descKey: "settings.holdToTalkDesc" },
  { key: "toggle", labelKey: "settings.toggleMode", descKey: "settings.toggleModeDesc" },
];

const webSearchProviderOptions: Array<{
  key: WebSearchProvider;
  labelKey: string;
  descKey: string;
}> = [
  { key: "model_native", labelKey: "settings.webSearchModelNative", descKey: "settings.webSearchModelNativeDesc" },
  { key: "exa", labelKey: "settings.webSearchExa", descKey: "settings.webSearchExaDesc" },
  { key: "tavily", labelKey: "settings.webSearchTavily", descKey: "settings.webSearchTavilyDesc" },
];

const sourceLabels: Record<string, string> = {
  user: "settings.sourceManual",
  learned: "settings.sourceLearned",
};

const sourceColors: Record<string, string> = {
  user: "var(--color-accent)",
  learned: "var(--color-learned)",
};

interface MicrophoneLevelPayload {
  deviceName?: string;
  level?: number;
}

interface LlmProviderDraft {
  baseUrl: string;
  model: string;
}

type LlmProviderDraftMap = Record<string, LlmProviderDraft>;

function findLlmPreset(key: string) {
  return llmProviderOptions.find((option) => option.key === key) ?? llmProviderOptions[0];
}

function isBuiltinCustomPreset(key: string) {
  return key === "custom";
}

function isFixedPresetProvider(key: string) {
  return llmProviderOptions.some((option) => option.key === key) && !isBuiltinCustomPreset(key);
}

function resolveEffectiveProvider(key: string, customProviders: CustomProvider[]): string {
  if (llmProviderOptions.some((option) => option.key === key)) {
    return key;
  }
  if (customProviders.some((provider) => provider.id === key)) {
    return key;
  }
  return customProviders.length > 0
    ? customProviders[customProviders.length - 1].id
    : "cerebras";
}

function resolveLlmBaseUrl(key: string, customBaseUrl?: string | null): string {
  const preset = findLlmPreset(key);
  if (isFixedPresetProvider(key)) {
    return preset.baseUrl;
  }
  return customBaseUrl?.trim() || preset.baseUrl;
}

function resolveLlmModel(key: string, customModel?: string | null): string {
  const preset = findLlmPreset(key);
  const normalizedModel = customModel?.trim();
  if (!normalizedModel) return preset.defaultModel;
  return normalizedModel;
}

function findReasoningModeOption(mode: LlmReasoningMode) {
  return reasoningModeOptions.find((option) => option.key === mode) ?? reasoningModeOptions[0];
}

function findRecordingModeOption(mode: "hold" | "toggle") {
  return recordingModeOptions.find((option) => option.key === mode) ?? recordingModeOptions[0];
}

function readLlmProviderDrafts(): LlmProviderDraftMap {
  const raw = readLocalStorage(LLM_PROVIDER_DRAFTS_KEY);
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as LlmProviderDraftMap;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function writeLlmProviderDrafts(drafts: LlmProviderDraftMap): void {
  writeLocalStorage(LLM_PROVIDER_DRAFTS_KEY, JSON.stringify(drafts));
}

export default function SettingsPage({
  onNavigate,
  active,
}: {
  onNavigate: (v: "main" | "settings") => void;
  active: boolean;
}) {
  const { t, i18n } = useTranslation();
  const { isDark, theme, setTheme } = useTheme();
  const { isRecording, retryModel, hotkeyDisplay, setHotkey, hotkeyError, hotkeyDiagnostic } = useRecordingContext();

  // --- Settings nav sections ---
  const navSections = useMemo(() => [
    { id: "appearance", labelKey: "settings.appearance" },
    { id: "engine", labelKey: "settings.engine" },
    { id: "hotkey", labelKey: "settings.hotkeySection" },
    { id: "microphone", labelKey: "settings.microphone" },
    { id: "input", labelKey: "settings.inputMethod" },
    { id: "ai-polish", labelKey: "settings.aiPolish" },
    { id: "assistant", labelKey: "settings.assistant" },
    { id: "translation", labelKey: "settings.translation" },
    { id: "vocabulary", labelKey: "settings.vocabulary" },
    { id: "misc", labelKey: "settings.startup" },
  ] as const, []);
  const [activeNavSection, setActiveNavSection] = useState("appearance");
  const settingsContentRef = useRef<HTMLDivElement | null>(null);
  const navScrollRef = useRef<HTMLDivElement | null>(null);
  const isNavClickScrolling = useRef(false);

  // IntersectionObserver: track which section is in view
  useEffect(() => {
    const container = settingsContentRef.current;
    if (!container) return;
    const sectionEls = navSections
      .map(({ id }) => container.querySelector(`[data-nav-id="${id}"]`))
      .filter(Boolean) as Element[];
    if (sectionEls.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        if (isNavClickScrolling.current) return;
        // Pick the topmost visible section
        let topId = "";
        let topY = Infinity;
        for (const entry of entries) {
          if (entry.isIntersecting && entry.boundingClientRect.top < topY) {
            topY = entry.boundingClientRect.top;
            topId = (entry.target as HTMLElement).dataset.navId ?? "";
          }
        }
        if (topId) setActiveNavSection(topId);
      },
      { root: container, rootMargin: "-10% 0px -70% 0px", threshold: 0 },
    );
    for (const el of sectionEls) observer.observe(el);
    return () => observer.disconnect();
  }, [navSections]);

  const handleNavClick = useCallback((id: string) => {
    const container = settingsContentRef.current;
    if (!container) return;
    const target = container.querySelector(`[data-nav-id="${id}"]`) as HTMLElement | null;
    if (!target) return;
    setActiveNavSection(id);
    isNavClickScrolling.current = true;
    target.scrollIntoView({ behavior: "smooth", block: "start" });
    // Re-enable observer after scroll settles
    setTimeout(() => { isNavClickScrolling.current = false; }, 600);
  }, []);

  // Auto-scroll the nav bar to keep active tab visible
  useEffect(() => {
    const navEl = navScrollRef.current;
    if (!navEl) return;
    const activeBtn = navEl.querySelector(`[data-nav-tab="${activeNavSection}"]`) as HTMLElement | null;
    if (!activeBtn) return;
    const left = activeBtn.offsetLeft - navEl.offsetWidth / 2 + activeBtn.offsetWidth / 2;
    navEl.scrollTo({ left, behavior: "smooth" });
  }, [activeNavSection]);

  // --- Picker group (mutually exclusive dropdowns) ---
  type PickerId = "provider" | "model" | "assistantModel" | "assistantProvider" | "assistantReasoning" | "polishReasoning" | "recordingMode" | "microphone" | "webSearchProvider";
  const picker = useExclusivePicker<PickerId>();
  const providerSearchInputRef = useRef<HTMLInputElement | null>(null);
  const modelSearchInputRef = useRef<HTMLInputElement | null>(null);
  const assistantModelSearchInputRef = useRef<HTMLInputElement | null>(null);

  // --- Hotkey capture (3 instances share 1 hook) ---
  const [translationHotkeyDisplay, setTranslationHotkeyDisplay] = useState("");
  const [assistantHotkeyDisplay, setAssistantHotkeyDisplay] = useState("");
  const mainHotkeyCapture = useHotkeyCapture({
    save: async (shortcut) => { await setHotkey(shortcut); },
    label: t("settings.hotkeyLabel"),
  });
  const translationHotkeyCapture = useHotkeyCapture({
    save: async (shortcut) => {
      await setTranslationHotkey(shortcut);
      setTranslationHotkeyDisplay(formatHotkeyForDisplay(shortcut));
    },
    label: t("settings.translationHotkeyLabel"),
  });
  const assistantHotkeyCapture = useHotkeyCapture({
    save: async (shortcut) => {
      await setAssistantHotkey(shortcut);
      setAssistantHotkeyDisplay(formatHotkeyForDisplay(shortcut));
    },
    label: t("settings.assistantHotkeyLabel"),
  });

  // --- Core state ---
  const [engine, setEngineState] = useState<string>("sensevoice");
  const [engineLoading, setEngineLoading] = useState(true);
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [recordingMode, setRecordingModeState] = useState<"hold" | "toggle">(() => {
    return readLocalStorage(RECORDING_MODE_KEY) === "toggle" ? "toggle" : "hold";
  });
  const [inputDevices, setInputDevices] = useState<InputDeviceInfo[]>([]);
  const [selectedInputDeviceName, setSelectedInputDeviceName] = useState<string>("");
  const [deviceListLoading, setDeviceListLoading] = useState(true);
  const [micLevel, setMicLevel] = useState(0);
  const [micMonitorReady, setMicMonitorReady] = useState(false);
  const [micLevelMonitorEnabled, setMicLevelMonitorEnabled] = useState(() => readLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY) === "true");
  const [inputMethod, setInputMethod] = useState<"sendInput" | "clipboard">(() => {
    return readLocalStorage(INPUT_METHOD_KEY) === "clipboard" ? "clipboard" : "sendInput";
  });
  const [soundEnabled, setSoundEnabledState] = useState(() => readLocalStorage(SOUND_ENABLED_KEY) !== "false");
  const [aiPolishEnabled, setAiPolishEnabled] = useState(() => readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  const [aiPolishApiKey, setAiPolishApiKey] = useState("");
  const [onlineAsrApiKey, setOnlineAsrApiKeyState] = useState("");
  const [onlineAsrRegion, setOnlineAsrRegion] = useState("international");
  const [onlineAsrUrl, setOnlineAsrUrl] = useState("");
  const [modelsDir, setModelsDirState] = useState("");
  const [modelsDirCustom, setModelsDirCustom] = useState(false);
  const [modelsDirMigrating, setModelsDirMigrating] = useState(false);
  const [modelsMigrateMsg, setModelsMigrateMsg] = useState("");

  // --- AI models ---
  const [aiModels, setAiModels] = useState<AiModelInfo[]>([]);
  const [assistantModels, setAssistantModels] = useState<AiModelInfo[]>([]);
  const [assistantModelsLoading, setAssistantModelsLoading] = useState(false);
  const [aiModelSearch, setAiModelSearch] = useState("");
  const [assistantModelSearch, setAssistantModelSearch] = useState("");
  const [aiModelsLoading, setAiModelsLoading] = useState(false);
  const [aiModelsError, setAiModelsError] = useState("");
  const [aiModelsSourceUrl, setAiModelsSourceUrl] = useState("");
  const [providerSearch, setProviderSearch] = useState("");
  const [assistantProviderSearch, setAssistantProviderSearch] = useState("");

  // --- LLM provider ---
  const [providerDrafts, setProviderDrafts] = useState<LlmProviderDraftMap>(() => readLlmProviderDrafts());
  const [llmProvider, setLlmProvider] = useState("cerebras");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customModel, setCustomModel] = useState("");
  const [assistantUseSeparateModel, setAssistantUseSeparateModel] = useState(false);
  const [assistantModel, setAssistantModel] = useState("");
  const [assistantProvider, setAssistantProviderState] = useState("");
  const [assistantApiKeyState, setAssistantApiKeyState] = useState("");
  const [polishReasoningMode, setPolishReasoningMode] = useState<LlmReasoningMode>("provider_default");
  const [assistantReasoningMode, setAssistantReasoningMode] = useState<LlmReasoningMode>("provider_default");
  const defaultReasoningSupport: LlmReasoningSupport = { supported: false, strategy: null, summary: t("model.reasoningDetecting") };
  const [polishReasoningSupport, setPolishReasoningSupportState] = useState<LlmReasoningSupport>(defaultReasoningSupport);
  const [assistantReasoningSupport, setAssistantReasoningSupportState] = useState<LlmReasoningSupport>(defaultReasoningSupport);
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [addingProvider, setAddingProvider] = useState(false);
  const [newProviderName, setNewProviderName] = useState("");
  const [newProviderBaseUrl, setNewProviderBaseUrl] = useState("");
  const [newProviderModel, setNewProviderModel] = useState("");
  const [newProviderFormat, setNewProviderFormat] = useState<ApiFormat>("openai_compat");
  const providerSupportsCustomEndpoint = llmProvider === "custom" || customProviders.some((p) => p.id === llmProvider);

  // --- Profile & misc ---
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [newHotWord, setNewHotWord] = useState("");
  const [translationTarget, setTranslationTargetState] = useState<string | null>(null);
  const [translationPickerOpen, setTranslationPickerOpen] = useState(false);
  const [customLangInput, setCustomLangInput] = useState("");
  const [showCustomLangInput, setShowCustomLangInput] = useState(false);
  const [customPromptState, setCustomPromptState] = useState<string>("");
  const [assistantPromptState, setAssistantPromptState] = useState<string>("");
  const [assistantScreenContextEnabled, setAssistantScreenContextEnabledState] = useState(false);
  const [aiPolishScreenContextEnabled, setAiPolishScreenContextEnabledState] = useState(false);
  const [webSearchEnabled, setWebSearchEnabledState] = useState(false);
  const [webSearchProvider, setWebSearchProviderState] = useState<WebSearchProvider>("model_native");
  const [webSearchMaxResults, setWebSearchMaxResultsState] = useState(5);
  const [webSearchApiKey, setWebSearchApiKeyState] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateStatusText, setUpdateStatusText] = useState("");
  const [latestAvailableVersion, setLatestAvailableVersion] = useState<string | null>(null);
  const [latestReleaseUrl, setLatestReleaseUrl] = useState<string | null>(null);

  const aiPolishKeySave = useDebouncedCallback((value: string, enabled: boolean) => {
    setAiPolishConfig(enabled, value).catch(() => {});
  }, 600, { onUnmount: "flush" });

  const assistantKeySave = useDebouncedCallback((value: string) => {
    setAssistantApiKey(value).catch(() => {});
  }, 600, { onUnmount: "flush" });

  const webSearchKeySave = useDebouncedCallback((value: string) => {
    setWebSearchApiKey(value).catch(() => {});
  }, 600, { onUnmount: "flush" });

  const webSearchConfigSave = useDebouncedCallback((
    enabled: boolean,
    provider: WebSearchProvider,
    maxResults: number,
  ) => {
    setWebSearchConfig(enabled, provider, maxResults).catch(() => {});
  }, 400, { onUnmount: "flush" });

  const llmConfigSave = useDebouncedCallback((
    provider: string,
    baseUrl: string,
    model: string,
    nextPolishReasoningMode: LlmReasoningMode,
    nextAssistantReasoningMode: LlmReasoningMode,
    nextAssistantUseSeparateModel: boolean,
    nextAssistantModel: string,
    nextAssistantProvider?: string | null,
  ) => {
    setLlmProviderConfig(
      provider,
      baseUrl || undefined,
      model || undefined,
      nextPolishReasoningMode,
      nextAssistantReasoningMode,
      nextAssistantUseSeparateModel,
      nextAssistantModel || undefined,
      nextAssistantProvider,
    ).catch(() => {});
  }, 400, { onUnmount: "flush" });

  const onlineAsrKeySave = useDebouncedCallback((value: string) => {
    setOnlineAsrApiKey(value).catch(() => {});
  }, 600, { onUnmount: "flush" });

  const customPromptSave = useDebouncedCallback((value: string) => {
    setCustomPrompt(value.trim() || null).catch(() => {
      toast.error(t("toast.customPromptSaveFailed"));
    });
  }, 800, { onUnmount: "flush" });

  const assistantPromptSave = useDebouncedCallback((value: string) => {
    setAssistantSystemPrompt(value.trim() || null).catch(() => {
      toast.error(t("toast.assistantPromptSaveFailed"));
    });
  }, 800, { onUnmount: "flush" });

  const updateProviderDraft = useCallback((provider: string, baseUrl: string, model: string) => {
    setProviderDrafts((prev) => {
      const next = {
        ...prev,
        [provider]: {
          baseUrl,
          model,
        },
      };
      writeLlmProviderDrafts(next);
      return next;
    });
  }, []);

  const resolveProviderDraft = useCallback((provider: string) => {
    const draft = providerDrafts[provider];
    // 先查自定义 provider
    const cp = customProviders.find((p) => p.id === provider);
    if (cp) {
      return {
        baseUrl: draft?.baseUrl ?? cp.base_url,
        model: draft?.model ?? cp.model,
      };
    }
    const preset = findLlmPreset(provider);
    return {
      baseUrl: isFixedPresetProvider(provider) ? preset.baseUrl : (draft?.baseUrl ?? preset.baseUrl),
      model: resolveLlmModel(provider, draft?.model),
    };
  }, [providerDrafts, customProviders]);

  const refreshAiPolishKey = useCallback(async (enabled = aiPolishEnabled) => {
    try {
      const key = (await getAiPolishApiKey()) || "";
      setAiPolishApiKey(key);
      await setAiPolishConfig(enabled, key).catch(() => {});
      return key;
    } catch {
      setAiPolishApiKey("");
      await setAiPolishConfig(enabled, "").catch(() => {});
      return "";
    }
  }, [aiPolishEnabled]);

  const refreshAssistantKey = useCallback(async () => {
    try {
      const key = (await getAssistantApiKey()) || "";
      setAssistantApiKeyState(key);
      return key;
    } catch {
      setAssistantApiKeyState("");
      return "";
    }
  }, []);

  // 从系统密钥环加载 API Key，并同步 enabled 状态到后端
  useEffect(() => {
    void refreshAiPolishKey(readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  }, [refreshAiPolishKey]);

  // 加载用户画像
  const refreshProfile = useCallback(async () => {
    try {
      const p = await getUserProfile();
      setProfile(p);
      const cps = p.llm_provider.custom_providers ?? [];
      setCustomProviders(cps);
      const nextProvider = resolveEffectiveProvider(p.llm_provider.active || "cerebras", cps);
      // 查自定义 provider
      const cp = cps.find((c) => c.id === nextProvider);
      const preset = findLlmPreset(nextProvider);
      const nextBaseUrl = cp
        ? cp.base_url
        : resolveLlmBaseUrl(nextProvider, p.llm_provider.custom_base_url ?? preset.baseUrl);
      const nextModel = cp
        ? cp.model
        : resolveLlmModel(nextProvider, p.llm_provider.custom_model ?? preset.defaultModel);
      setLlmProvider(nextProvider);
      setCustomBaseUrl(nextBaseUrl);
      setCustomModel(nextModel);
      setAssistantUseSeparateModel(Boolean(p.llm_provider.assistant_use_separate_model));
      setAssistantModel((p.llm_provider.assistant_model ?? nextModel).trim() || nextModel);
      setAssistantProviderState(p.llm_provider.assistant_provider ?? nextProvider);
      setPolishReasoningMode(p.llm_provider.polish_reasoning_mode ?? p.llm_provider.reasoning_mode ?? "provider_default");
      setAssistantReasoningMode(p.llm_provider.assistant_reasoning_mode ?? p.llm_provider.reasoning_mode ?? "provider_default");
      updateProviderDraft(nextProvider, nextBaseUrl, nextModel);
      setTranslationTargetState(p.translation_target ?? null);
      setTranslationHotkeyDisplay(p.translation_hotkey ? formatHotkeyForDisplay(p.translation_hotkey) : "");
      setCustomPromptState(p.custom_prompt ?? "");
      setAssistantHotkeyDisplay(p.assistant_hotkey ? formatHotkeyForDisplay(p.assistant_hotkey) : "");
      setAssistantPromptState(p.assistant_system_prompt ?? "");
      setAssistantScreenContextEnabledState(Boolean(p.assistant_screen_context_enabled));
      setAiPolishScreenContextEnabledState(Boolean(p.ai_polish_screen_context_enabled));
      setWebSearchEnabledState(Boolean(p.web_search?.enabled));
      setWebSearchProviderState(p.web_search?.provider ?? "model_native");
      setWebSearchMaxResultsState(p.web_search?.max_results ?? 5);
    } catch { /* ignore */ }
  }, [updateProviderDraft]);

  useEffect(() => {
    refreshProfile().then(() => {
      refreshAssistantKey();
      getWebSearchApiKey().then(setWebSearchApiKeyState).catch(() => {});
    });
  }, [refreshProfile, refreshAssistantKey]);

  useEffect(() => { getVersion().then(setAppVersion).catch(() => {}); }, []);

  useEffect(() => {
    getEngine().then(e => {
      setEngineState(e);
      setEngineLoading(false);
    }).catch(() => setEngineLoading(false));
    getOnlineAsrApiKey().then(k => setOnlineAsrApiKeyState(k || "")).catch(() => {});
    getOnlineAsrEndpoint().then(ep => {
      setOnlineAsrRegion(ep.region);
      setOnlineAsrUrl(ep.url);
    }).catch(() => {});
    getModelsDir().then(info => {
      setModelsDirState(info.path);
      setModelsDirCustom(info.is_custom);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    const unlisten = listen<{ status: string; message?: string; progress?: number }>(
      "models-migrate-status",
      (event) => {
        const { status, message } = event.payload;
        if (status === "migrating" && message) {
          setModelsMigrateMsg(message);
        } else if (status === "completed") {
          setModelsMigrateMsg("");
        }
      },
    );
    return () => { unlisten.then(fn => fn()); };
  }, []);

  const handleEngineSwitch = async (newEngine: string) => {
    if (engineLoading || newEngine === engine) return;
    setEngineLoading(true);
    try {
      await setEngine(newEngine);
      setEngineState(newEngine);
      const label = engineOptions.find((o) => o.key === newEngine)?.label ?? newEngine;
      toast.success(t("toast.switchedToEngine", { label }));
      retryModel();
    } catch {
      toast.error(t("toast.switchEngineFailed"));
    } finally {
      setEngineLoading(false);
    }
  };

  const handleCheckForUpdates = useCallback(async () => {
    if (updateChecking) return;

    setUpdateChecking(true);
    setLatestAvailableVersion(null);
    setLatestReleaseUrl(null);
    setUpdateStatusText(t("toast.checkingGitHub"));

    try {
      const updateInfo = await checkAppUpdate();
      setLatestReleaseUrl(updateInfo.releaseUrl ?? null);
      if (!updateInfo.available || !updateInfo.latestVersion) {
        setUpdateStatusText(t("toast.alreadyLatest"));
        toast.success(t("toast.alreadyLatest"));
        return;
      }

      setLatestAvailableVersion(updateInfo.latestVersion);
      setUpdateStatusText(t("toast.newVersionFound", { version: updateInfo.latestVersion }));
      toast.info(t("toast.newVersionToast", { version: updateInfo.latestVersion }));
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toast.checkUpdateFailed");
      setUpdateStatusText(message);
      toast.error(message);
    } finally {
      setUpdateChecking(false);
    }
  }, [updateChecking]);

  const handleOpenReleasePage = useCallback(async () => {
    try {
      const message = await openAppReleasePage(latestReleaseUrl);
      toast.success(message);
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toast.openReleaseFailed");
      setUpdateStatusText(message);
      toast.error(message);
    }
  }, [latestReleaseUrl]);

  useEffect(() => {
    isAutostartEnabled().then(enabled => {
      setAutostart(enabled);
      setAutostartLoading(false);
    }).catch(() => setAutostartLoading(false));
  }, []);

  const refreshInputDevices = useCallback(async () => {
    setDeviceListLoading(true);
    try {
      const payload = await listInputDevices();
      setInputDevices(payload.devices);
      setSelectedInputDeviceName(payload.selectedDeviceName ?? "");
    } catch {
      toast.error(t("toast.micListFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const stored = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
      if (stored) {
        await setInputDevice(stored).catch(() => {});
      }
      await refreshInputDevices();
    })();
  }, [refreshInputDevices]);

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    const startMonitor = async () => {
      try {
        await stopMicrophoneLevelMonitor().catch(() => undefined);
        if (!active || !micLevelMonitorEnabled || isRecording) {
          if (!disposed) {
            setMicMonitorReady(false);
            setMicLevel(0);
          }
          return;
        }
        await startMicrophoneLevelMonitor();
        if (!disposed) setMicMonitorReady(true);
      } catch {
        if (!disposed) {
          setMicMonitorReady(false);
          setMicLevel(0);
        }
      }
    };

    void (async () => {
      try {
        unlisten = await listen<MicrophoneLevelPayload>("microphone-level", (event) => {
          if (disposed) return;
          const level = typeof event.payload?.level === "number" ? event.payload.level : 0;
          setMicLevel(Math.max(0, Math.min(1, level)));
        });
      } catch {
        // ignore
      }

      await startMonitor();

      if (disposed && unlisten) {
        unlisten();
        unlisten = null;
      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
      void stopMicrophoneLevelMonitor().catch(() => undefined);
    };
  }, [active, isRecording, micLevelMonitorEnabled, selectedInputDeviceName]);

  const handleInputDeviceChange = async (name: string) => {
    setDeviceListLoading(true);
    try {
      await setInputDevice(name || null);
      if (name) {
        writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, name);
      } else {
        writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, "");
      }
      setSelectedInputDeviceName(name);
      await refreshInputDevices();
    } catch {
      toast.error(t("toast.micSwitchFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  };

  const handleMicLevelMonitorToggle = useCallback((enabled: boolean) => {
    setMicLevelMonitorEnabled(enabled);
    writeLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY, enabled ? "true" : "false");
    if (!enabled) {
      setMicMonitorReady(false);
      setMicLevel(0);
    }
  }, []);

  const handleAutostartToggle = async () => {
    if (autostartLoading) return;
    const prev = autostart;
    // Optimistic update: toggle immediately, revert on failure
    setAutostart(!prev);
    setAutostartLoading(true);
    try {
      if (prev) {
        await disableAutostart();
        toast.success(t("toast.autostartDisabled"), { duration: 1100 });
      } else {
        await enableAutostart();
        toast.success(t("toast.autostartEnabled"), { duration: 1100 });
      }
    } catch {
      setAutostart(prev); // revert
      toast.error(t("toast.autostartFailed"));
    } finally {
      setAutostartLoading(false);
    }
  };

  // (hotkey capture effects are now in useHotkeyCapture hook)

  const handleResetHotkey = async () => {
    if (mainHotkeyCapture.saving) return;
    mainHotkeyCapture.cancelCapture();
    try {
      await setHotkey(DEFAULT_HOTKEY);
      toast.success(t("toast.hotkeyReset"));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.hotkeyResetFailed"));
    }
  };

  const handleClearHotkey = async (
    saveFn: (v: string | null) => Promise<unknown>,
    setDisplay: (v: string) => void,
    label: string,
    cancelCapture?: () => void,
  ) => {
    cancelCapture?.();
    try {
      await saveFn(null);
      setDisplay("");
      toast.success(t("toast.hotkeyCleared", { label }));
    } catch (err) {
      toast.error(err instanceof Error ? err.message : t("toast.hotkeyClearFailed", { label }));
    }
  };

  // Unified picker focus/clear-search effect (replaces 6 individual effects)
  useEffect(() => {
    setProviderSearch("");
    setAiModelSearch("");
    setAssistantModelSearch("");
    setAssistantProviderSearch("");
    if (picker.active === "provider") {
      providerSearchInputRef.current?.focus();
    } else if (picker.active === "model") {
      modelSearchInputRef.current?.focus();
    } else if (picker.active === "assistantModel") {
      assistantModelSearchInputRef.current?.focus();
    }
  }, [picker.active]);

  const refreshAiModels = useCallback(async (silent = false) => {
    const apiKey = aiPolishApiKey.trim();
    const baseUrl = customBaseUrl.trim();
    if (!apiKey) {
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError(t("settings.apiKeyMissing"));
      return;
    }

    setAiModelsLoading(true);
    if (!silent) {
      setAiModelsError("");
    }

    try {
      const payload = await listAiModels(llmProvider, baseUrl || undefined, apiKey);
      setAiModels(payload.models);
      setAiModelsSourceUrl(payload.sourceUrl);
      setAiModelsError(payload.models.length === 0 ? t("settings.modelListEmpty") : "");
    } catch (err) {
      const message = err instanceof Error ? err.message : t("settings.fetchModelsFailed");
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError(message);
    } finally {
      setAiModelsLoading(false);
    }
  }, [aiPolishApiKey, customBaseUrl, llmProvider]);

  const aiModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAiModels(silent);
  }, 700);

  // 助手独立模型列表：provider 不同时独立拉取，相同时复用润色列表
  const refreshAssistantModels = useCallback(async (silent = false) => {
    const effectiveProvider = assistantProvider || llmProvider;
    // 同 provider 时复用润色模型列表
    if (effectiveProvider === llmProvider) {
      setAssistantModels(aiModels);
      return;
    }
    const apiKey = assistantApiKeyState.trim();
    if (!apiKey) {
      setAssistantModels([]);
      return;
    }
    // 解析助手 provider 的 base_url
    const cp = customProviders.find((p) => p.id === effectiveProvider);
    const baseUrl = cp ? cp.base_url : findLlmPreset(effectiveProvider).baseUrl;

    setAssistantModelsLoading(true);
    try {
      const payload = await listAiModels(effectiveProvider, baseUrl || undefined, apiKey);
      setAssistantModels(payload.models);
    } catch {
      if (!silent) setAssistantModels([]);
    } finally {
      setAssistantModelsLoading(false);
    }
  }, [aiModels, assistantApiKeyState, assistantProvider, customProviders, llmProvider]);

  const assistantModelsFetch = useDebouncedCallback((silent: boolean) => {
    void refreshAssistantModels(silent);
  }, 700);

  useEffect(() => {
    if (!aiPolishApiKey.trim()) {
      aiModelsFetch.cancel();
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("");
      setAiModelsLoading(false);
      return;
    }

    aiModelsFetch.schedule(true);

    return () => {
      aiModelsFetch.cancel();
    };
  }, [aiModelsFetch, aiPolishApiKey, customBaseUrl, llmProvider]);

  // 助手独立模型列表自动刷新
  useEffect(() => {
    if (!assistantUseSeparateModel) {
      return;
    }
    const effectiveProvider = assistantProvider || llmProvider;
    if (effectiveProvider === llmProvider) {
      // 同 provider 时直接同步润色列表
      setAssistantModels(aiModels);
      return;
    }
    if (!assistantApiKeyState.trim()) {
      assistantModelsFetch.cancel();
      setAssistantModels([]);
      return;
    }
    assistantModelsFetch.schedule(true);
    return () => { assistantModelsFetch.cancel(); };
  }, [aiModels, assistantApiKeyState, assistantModelsFetch, assistantProvider, assistantUseSeparateModel, llmProvider]);

  const handleAddHotWord = useCallback(() => {
    const word = newHotWord.trim();
    if (!word) return;

    addHotWord(word, 3).then(() => {
      setNewHotWord("");
      refreshProfile();
      toast.success(t("toast.hotWordAdded", { word }));
    }).catch(() => toast.error(t("toast.hotWordAddFailed")));
  }, [newHotWord, refreshProfile]);

  const hotkeyStatusError = hotkeyError || hotkeyDiagnostic?.lastError || null;
  const selectedDeviceMissing = Boolean(selectedInputDeviceName)
    && !inputDevices.some((device) => device.name === selectedInputDeviceName);
  const currentLlmPreset = useMemo(() => {
    const effectiveProvider = resolveEffectiveProvider(llmProvider, customProviders);
    const cp = customProviders.find((p) => p.id === effectiveProvider);
    if (cp) return { key: cp.id, label: cp.name, descKey: undefined as string | undefined, baseUrl: cp.base_url, defaultModel: cp.model, models: [] as string[], desc: cp.api_format === "anthropic" ? "Anthropic" : t("settings.openaiCompat") };
    const preset = findLlmPreset(effectiveProvider);
    return { ...preset, label: preset.labelKey ? t(preset.labelKey) : preset.label, desc: t(preset.descKey) };
  }, [llmProvider, customProviders, t]);
  const currentAssistantPreset = useMemo(() => {
    const p = assistantProvider || llmProvider;
    const cp = customProviders.find((c) => c.id === p);
    if (cp) return { key: cp.id, label: cp.name, baseUrl: cp.base_url, defaultModel: cp.model, desc: cp.api_format === "anthropic" ? "Anthropic" : t("settings.openaiCompat") };
    const preset = findLlmPreset(p);
    return { ...preset, label: preset.labelKey ? t(preset.labelKey) : preset.label, desc: t(preset.descKey) };
  }, [assistantProvider, llmProvider, customProviders, t]);
  const assistantProviderDiffers = assistantUseSeparateModel && assistantProvider && assistantProvider !== llmProvider;
  const allProviderOptions = useMemo(() => {
    const presets = llmProviderOptions.map((opt) => ({ key: opt.key, label: opt.labelKey ? t(opt.labelKey) : opt.label, desc: t(opt.descKey), baseUrl: opt.baseUrl, isCustom: false as const }));
    const customs = customProviders.map((cp) => ({
      key: cp.id,
      label: cp.name,
      desc: cp.api_format === "anthropic" ? "Anthropic" : t("settings.openaiCompat"),
      baseUrl: cp.base_url,
      isCustom: true as const,
    }));
    return [...presets, ...customs];
  }, [customProviders, t]);
  const filteredProviderOptions = useMemo(() => allProviderOptions.filter(({ label, desc, baseUrl }) => {
    const keyword = providerSearch.trim().toLowerCase();
    if (!keyword) return true;
    return label.toLowerCase().includes(keyword)
      || desc.toLowerCase().includes(keyword)
      || baseUrl.toLowerCase().includes(keyword);
  }), [allProviderOptions, providerSearch]);
  const filteredAssistantProviderOptions = useMemo(() => allProviderOptions.filter(({ label, desc, baseUrl }) => {
    const keyword = assistantProviderSearch.trim().toLowerCase();
    if (!keyword) return true;
    return label.toLowerCase().includes(keyword)
      || desc.toLowerCase().includes(keyword)
      || baseUrl.toLowerCase().includes(keyword);
  }), [allProviderOptions, assistantProviderSearch]);
  const filteredAiModels = useMemo(() => aiModels.filter((model) => {
    const keyword = aiModelSearch.trim().toLowerCase();
    if (!keyword) return true;
    return model.id.toLowerCase().includes(keyword) || (model.ownedBy ?? "").toLowerCase().includes(keyword);
  }), [aiModels, aiModelSearch]);
  const effectiveAssistantModels = assistantUseSeparateModel && assistantProvider && assistantProvider !== llmProvider
    ? assistantModels
    : aiModels;
  const filteredAssistantModels = useMemo(() => effectiveAssistantModels.filter((model) => {
    const keyword = assistantModelSearch.trim().toLowerCase();
    if (!keyword) return true;
    return model.id.toLowerCase().includes(keyword) || (model.ownedBy ?? "").toLowerCase().includes(keyword);
  }), [effectiveAssistantModels, assistantModelSearch]);
  const selectedAiModel = aiModels.find((model) => model.id === customModel);
  const selectedAssistantAiModel = effectiveAssistantModels.find((model) => model.id === assistantModel);

  const handleProviderSelect = useCallback(async (nextProvider: string) => {
    if (nextProvider === llmProvider) {
      picker.close();
      setProviderSearch("");
      return;
    }

    updateProviderDraft(llmProvider, customBaseUrl, customModel);
    aiPolishKeySave.cancel();
    llmConfigSave.cancel();
    await setAiPolishConfig(aiPolishEnabled, aiPolishApiKey).catch(() => {});

    const nextDraft = resolveProviderDraft(nextProvider);
    const nextAssistantModel = assistantUseSeparateModel
      ? assistantModel.trim() || nextDraft.model
      : nextDraft.model;
    setLlmProvider(nextProvider);
    setCustomBaseUrl(nextDraft.baseUrl);
    setCustomModel(nextDraft.model);
    setAssistantModel(nextAssistantModel);
    // 若未开启独立模式，助手 provider 跟随润色
    if (!assistantUseSeparateModel) {
      setAssistantProviderState(nextProvider);
    }
    updateProviderDraft(nextProvider, nextDraft.baseUrl, nextDraft.model);
    picker.close();
    setProviderSearch("");
    setAiModelSearch("");
    setAssistantModelSearch("");
    await setLlmProviderConfig(
      nextProvider,
      nextDraft.baseUrl || undefined,
      nextDraft.model || undefined,
      polishReasoningMode,
      assistantReasoningMode,
      assistantUseSeparateModel,
      nextAssistantModel,
      assistantUseSeparateModel ? assistantProvider : undefined,
    ).catch(() => {});
    await refreshAiPolishKey();
    if (!assistantUseSeparateModel) {
      await refreshAssistantKey();
    }
  }, [
    aiPolishApiKey,
    aiPolishEnabled,
    assistantProvider,
    customBaseUrl,
    customModel,
    llmProvider,
    polishReasoningMode,
    assistantReasoningMode,
    assistantUseSeparateModel,
    assistantModel,
    aiPolishKeySave,
    llmConfigSave,
    refreshAiPolishKey,
    refreshAssistantKey,
    resolveProviderDraft,
    updateProviderDraft,
  ]);

  const handleModelSelect = useCallback((nextModel: string) => {
    const normalizedModel = nextModel.trim();
    if (!normalizedModel) return;
    setCustomModel(normalizedModel);
    updateProviderDraft(llmProvider, customBaseUrl, normalizedModel);
    llmConfigSave.schedule(
      llmProvider,
      customBaseUrl,
      normalizedModel,
      polishReasoningMode,
      assistantReasoningMode,
      assistantUseSeparateModel,
      assistantModel,
    );
    picker.close();
    setAiModelSearch("");
    if (!assistantUseSeparateModel) {
      setAssistantModel(normalizedModel);
    }
  }, [assistantModel, assistantReasoningMode, assistantUseSeparateModel, customBaseUrl, llmProvider, picker, polishReasoningMode, llmConfigSave, updateProviderDraft]);

  const handleAssistantModelToggle = useCallback((enabled: boolean) => {
    setAssistantUseSeparateModel(enabled);
    picker.close();
    if (!enabled) {
      setAssistantModel(customModel);
    } else {
      if (!assistantModel.trim()) {
        setAssistantModel(customModel);
      }
      // 开启时默认跟随润色 provider（若尚未独立设定）
      if (!assistantProvider || assistantProvider === llmProvider) {
        setAssistantProviderState(llmProvider);
      }
    }
    llmConfigSave.schedule(
      llmProvider,
      customBaseUrl,
      customModel,
      polishReasoningMode,
      assistantReasoningMode,
      enabled,
      (enabled ? assistantModel : customModel).trim() || customModel,
      enabled ? (assistantProvider || llmProvider) : undefined,
    );
    if (enabled) {
      void refreshAssistantKey();
    }
  }, [assistantModel, assistantProvider, assistantReasoningMode, customBaseUrl, customModel, llmProvider, picker, polishReasoningMode, refreshAssistantKey, llmConfigSave]);

  const handleAssistantProviderSelect = useCallback(async (nextProvider: string) => {
    if (nextProvider === assistantProvider) {
      picker.close();
      setAssistantProviderSearch("");
      return;
    }
    setAssistantProviderState(nextProvider);
    picker.close();
    setAssistantProviderSearch("");
    // 先保存 config（含新 assistantProvider），再加载对应 key
    await setLlmProviderConfig(
      llmProvider,
      customBaseUrl || undefined,
      customModel || undefined,
      polishReasoningMode,
      assistantReasoningMode,
      true,
      assistantModel || undefined,
      nextProvider,
    ).catch(() => {});
    await refreshAssistantKey();
  }, [assistantModel, assistantProvider, assistantReasoningMode, customBaseUrl, customModel, llmProvider, polishReasoningMode, refreshAssistantKey]);

  const handleAssistantModelSelect = useCallback((nextModel: string) => {
    const normalizedModel = nextModel.trim();
    if (!normalizedModel) return;
    setAssistantModel(normalizedModel);
    picker.close();
    setAssistantModelSearch("");
    llmConfigSave.schedule(
      llmProvider,
      customBaseUrl,
      customModel,
      polishReasoningMode,
      assistantReasoningMode,
      true,
      normalizedModel,
      assistantProvider,
    );
  }, [assistantProvider, assistantReasoningMode, customBaseUrl, customModel, llmProvider, picker, polishReasoningMode, llmConfigSave]);

  const handleTranslationSelect = useCallback(async (target: string | null) => {
    setTranslationTargetState(target);
    setTranslationPickerOpen(false);
    setShowCustomLangInput(false);
    setCustomLangInput("");
    try {
      const autoEnabled = await setTranslationTarget(target);
      if (autoEnabled) {
        aiPolishKeySave.cancel();
        setAiPolishEnabled(true);
        writeLocalStorage(AI_POLISH_ENABLED_KEY, "true");
        await setAiPolishConfig(true, aiPolishApiKey).catch(() => {});
        toast.success(t("toast.translationAutoPolish"));
      }
    } catch {
      toast.error(t("toast.translationSaveFailed"));
    }
  }, [aiPolishApiKey, aiPolishKeySave]);

  const handleCustomPromptChange = useCallback((value: string) => {
    setCustomPromptState(value);
    customPromptSave.schedule(value);
  }, [customPromptSave]);

  const handleAssistantPromptChange = useCallback((value: string) => {
    setAssistantPromptState(value);
    assistantPromptSave.schedule(value);
  }, [assistantPromptSave]);

  const currentProviderFormat: ApiFormat = useMemo(() => {
    return customProviders.find((provider) => provider.id === llmProvider)?.api_format ?? "openai_compat";
  }, [customProviders, llmProvider]);

  const effectiveReasoningBaseUrl = useMemo(() => {
    return resolveLlmBaseUrl(llmProvider, customBaseUrl);
  }, [customBaseUrl, llmProvider]);

  const effectivePolishReasoningModel = useMemo(() => {
    return resolveLlmModel(llmProvider, customModel);
  }, [customModel, llmProvider]);

  const effectiveAssistantReasoningModel = useMemo(() => {
    if (!assistantUseSeparateModel) {
      return effectivePolishReasoningModel;
    }
    return assistantModel.trim() || effectivePolishReasoningModel;
  }, [assistantModel, assistantUseSeparateModel, effectivePolishReasoningModel]);

  useEffect(() => {
    let cancelled = false;
    setPolishReasoningSupportState({
      supported: false,
      strategy: null,
      summary: t("model.reasoningDetecting"),
    });

    void getLlmReasoningSupport(
      llmProvider,
      effectiveReasoningBaseUrl || undefined,
      effectivePolishReasoningModel || undefined,
      currentProviderFormat,
    ).then((support) => {
      if (!cancelled) {
        setPolishReasoningSupportState(support);
      }
    }).catch(() => {
      if (!cancelled) {
        setPolishReasoningSupportState({
          supported: false,
          strategy: null,
          summary: t("model.reasoningUnavailable"),
        });
      }
    });

    return () => {
      cancelled = true;
    };
  }, [currentProviderFormat, effectivePolishReasoningModel, effectiveReasoningBaseUrl, llmProvider]);

  useEffect(() => {
    let cancelled = false;
    setAssistantReasoningSupportState({
      supported: false,
      strategy: null,
      summary: t("model.reasoningDetecting"),
    });

    void getLlmReasoningSupport(
      llmProvider,
      effectiveReasoningBaseUrl || undefined,
      effectiveAssistantReasoningModel || undefined,
      currentProviderFormat,
    ).then((support) => {
      if (!cancelled) {
        setAssistantReasoningSupportState(support);
      }
    }).catch(() => {
      if (!cancelled) {
        setAssistantReasoningSupportState({
          supported: false,
          strategy: null,
          summary: t("model.reasoningUnavailable"),
        });
      }
    });

    return () => {
      cancelled = true;
    };
  }, [currentProviderFormat, effectiveAssistantReasoningModel, effectiveReasoningBaseUrl, llmProvider]);

  const buildReasoningModeHint = useCallback((support: LlmReasoningSupport, selectedMode: LlmReasoningMode) => {
    if (support.supported) {
      return support.summary;
    }
    if (selectedMode !== "provider_default") {
      return support.summary + t("model.reasoningFallback");
    }
    return support.summary;
  }, [t]);

  const polishReasoningModeDisabled = !polishReasoningSupport.supported;
  const assistantReasoningModeDisabled = !assistantReasoningSupport.supported;
  const selectedAssistantReasoningOption = useMemo(
    () => findReasoningModeOption(assistantReasoningMode),
    [assistantReasoningMode],
  );
  const selectedPolishReasoningOption = useMemo(
    () => findReasoningModeOption(polishReasoningMode),
    [polishReasoningMode],
  );
  const selectedRecordingModeOption = useMemo(
    () => findRecordingModeOption(recordingMode),
    [recordingMode],
  );
  const selectedWebSearchProviderOption = useMemo(
    () => webSearchProviderOptions.find((o) => o.key === webSearchProvider) ?? webSearchProviderOptions[0],
    [webSearchProvider],
  );
  const selectedInputDeviceOption = useMemo(() => {
    if (!selectedInputDeviceName) {
      const systemDefaultDevice = inputDevices.find((device) => device.isDefault);
      return {
        label: t("settings.followSystemMic"),
        desc: systemDefaultDevice ? t("settings.currentDefault", { name: systemDefaultDevice.name }) : t("settings.autoUseDefault"),
      };
    }

    const activeDevice = inputDevices.find((device) => device.name === selectedInputDeviceName);
    if (activeDevice) {
      return {
        label: activeDevice.name,
        desc: activeDevice.isDefault ? t("settings.alsoSystemDefault") : t("settings.fixedMic"),
      };
    }

    return {
      label: selectedInputDeviceName,
      desc: t("settings.deviceUnavailable"),
    };
  }, [inputDevices, selectedInputDeviceName, t]);
  const polishReasoningModeHint = useMemo(
    () => buildReasoningModeHint(polishReasoningSupport, polishReasoningMode),
    [buildReasoningModeHint, polishReasoningMode, polishReasoningSupport],
  );
  const assistantReasoningModeHint = useMemo(
    () => buildReasoningModeHint(assistantReasoningSupport, assistantReasoningMode),
    [assistantReasoningMode, assistantReasoningSupport, buildReasoningModeHint],
  );

  const handlePolishReasoningModeChange = useCallback((mode: LlmReasoningMode) => {
    if (polishReasoningModeDisabled) return;
    setPolishReasoningMode(mode);
    picker.close();
    llmConfigSave.schedule(
      llmProvider,
      customBaseUrl,
      customModel,
      mode,
      assistantReasoningMode,
      assistantUseSeparateModel,
      assistantModel,
    );
  }, [assistantModel, assistantReasoningMode, assistantUseSeparateModel, customBaseUrl, customModel, llmProvider, picker, polishReasoningModeDisabled, llmConfigSave]);

  const handleAssistantReasoningModeChange = useCallback((mode: LlmReasoningMode) => {
    if (assistantReasoningModeDisabled) return;
    setAssistantReasoningMode(mode);
    picker.close();
    llmConfigSave.schedule(
      llmProvider,
      customBaseUrl,
      customModel,
      polishReasoningMode,
      mode,
      assistantUseSeparateModel,
      assistantModel,
    );
  }, [assistantModel, assistantReasoningModeDisabled, assistantUseSeparateModel, customBaseUrl, customModel, llmProvider, picker, polishReasoningMode, llmConfigSave]);

  const handleRecordingModeChange = useCallback((mode: "hold" | "toggle") => {
    setRecordingModeState(mode);
    picker.close();
    writeLocalStorage(RECORDING_MODE_KEY, mode);
    setRecordingMode(mode === "toggle").catch(() => {});
  }, []);

  const handleAssistantScreenContextToggle = useCallback((enabled: boolean) => {
    setAssistantScreenContextEnabledState(enabled);
    setAssistantScreenContextEnabled(enabled).catch(() => {
      setAssistantScreenContextEnabledState(!enabled);
      toast.error(t("toast.assistantScreenContextFailed"));
    });
  }, []);

  const handleAiPolishScreenContextToggle = useCallback((enabled: boolean) => {
    setAiPolishScreenContextEnabledState(enabled);
    setAiPolishScreenContextEnabled(enabled).catch(() => {
      setAiPolishScreenContextEnabledState(!enabled);
      toast.error(t("toast.polishScreenContextFailed"));
    });
  }, []);

  const handleWebSearchToggle = useCallback((enabled: boolean) => {
    setWebSearchEnabledState(enabled);
    webSearchConfigSave.schedule(enabled, webSearchProvider, webSearchMaxResults);
  }, [webSearchProvider, webSearchMaxResults, webSearchConfigSave]);

  const handleWebSearchProviderChange = useCallback((provider: WebSearchProvider) => {
    setWebSearchProviderState(provider);
    picker.close();
    webSearchConfigSave.schedule(webSearchEnabled, provider, webSearchMaxResults);
  }, [webSearchEnabled, webSearchMaxResults, webSearchConfigSave, picker]);

  const handleWebSearchMaxResultsChange = useCallback((value: number) => {
    setWebSearchMaxResultsState(value);
    webSearchConfigSave.schedule(webSearchEnabled, webSearchProvider, value);
  }, [webSearchEnabled, webSearchProvider, webSearchConfigSave]);

  return (
    <div className="page-root">

      <TitleBar
        title={t("settings.title")}
        leftAction={
          <button aria-label={t("common.back")} className="icon-btn plain" onClick={() => onNavigate("main")}>
            <ArrowLeft size={14} strokeWidth={1.5} />
          </button>
        }
      />

      {/* Settings nav */}
      <nav className="settings-nav" ref={navScrollRef}>
        {navSections.map(({ id, labelKey }) => (
          <button
            key={id}
            type="button"
            className="settings-nav-tab"
            data-active={activeNavSection === id}
            data-nav-tab={id}
            onClick={() => handleNavClick(id)}
          >
            {t(labelKey)}
          </button>
        ))}
      </nav>

      {/* Content */}
      <div className="settings-content" ref={settingsContentRef} style={{ padding: `16px ${PADDING}px 16px` }}>
        <div className="settings-sections">

          {/* Appearance */}
          <section className="settings-card" data-nav-id="appearance" style={{ animationDelay: "0ms" }}>
            <div className="settings-section-header">
              {isDark ? <Moon size={15} className="icon-accent" /> : <Sun size={15} className="icon-accent" />}
              <h2 className="settings-section-title">{t("settings.appearance")}</h2>
            </div>
            <div className="settings-grid-3">
              {themeOptions.map(({ mode, icon: Icon, labelKey }) => (
                <button
                  key={mode}
                  className="theme-btn settings-option-btn theme-option"
                  aria-label={t("settings.switchToTheme", { label: t(labelKey) })}
                  aria-pressed={theme === mode}
                  onClick={() => setTheme(mode)}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{t(labelKey)}</span>
                </button>
              ))}
            </div>
            <div className="settings-row" style={{ marginTop: 6, gap: 8, alignItems: "center" }}>
              <Languages size={13} className="icon-tertiary" style={{ flexShrink: 0 }} />
              <span className="settings-option-desc" style={{ marginRight: "auto" }}>{t("settings.language")}</span>
              <div className="settings-lang-switcher">
                {([
                  { lang: "zh", label: "中文" },
                  { lang: "en", label: "EN" },
                ] as const).map(({ lang, label }) => (
                  <button
                    key={lang}
                    className="settings-lang-btn"
                    data-active={i18n.language.startsWith(lang)}
                    onClick={() => {
                      i18n.changeLanguage(lang);
                      writeLocalStorage(LANGUAGE_STORAGE_KEY, lang);
                    }}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </section>

          {/* Engine */}
          <section className="settings-card" data-nav-id="engine" style={{ animationDelay: "50ms" }}>
            <div className="settings-section-header">
              <AudioLines size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.engine")}</h2>
            </div>
            <div className="settings-grid-3">
              {engineOptions.map(({ key, icon: Icon, label, descKey }) => (
                <button
                  key={key}
                  className="theme-btn settings-option-btn"
                  aria-label={label}
                  aria-pressed={engine === key}
                  disabled={engineLoading}
                  onClick={() => handleEngineSwitch(key)}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                  <span className="settings-option-desc">{t(descKey)}</span>
                </button>
              ))}
            </div>
            {engine === "glm-asr" && (
              <div className="settings-column" style={{ gap: 8, marginTop: 8 }}>
                <div className="settings-column" style={{ gap: 4 }}>
                  <span className="settings-option-desc">{t("settings.apiEndpoint")}</span>
                  <div className="settings-row" style={{ gap: 6 }}>
                    {([
                      { region: "international", labelKey: "settings.international" },
                      { region: "domestic", labelKey: "settings.domestic" },
                    ] as const).map(({ region, labelKey }) => (
                      <button
                        key={region}
                        className={`theme-btn${onlineAsrRegion === region ? " active" : ""}`}
                        onClick={async () => {
                          try {
                            const ep = await setOnlineAsrEndpoint(region);
                            setOnlineAsrRegion(ep.region);
                            setOnlineAsrUrl(ep.url);
                          } catch {}
                        }}
                        style={{ flex: 1 }}
                      >
                        {t(labelKey)}
                      </button>
                    ))}
                  </div>
                  {onlineAsrUrl && (
                    <span className="settings-option-desc" style={{ fontSize: 11, opacity: 0.6 }}>
                      {onlineAsrUrl}
                    </span>
                  )}
                </div>
                <div className="settings-column" style={{ gap: 4 }}>
                  <span className="settings-option-desc">{t("settings.apiKey")}</span>
                  <SecretInput
                    value={onlineAsrApiKey}
                    placeholder={t("settings.glmApiKeyPlaceholder")}
                    ariaLabelShow={t("settings.showApiKey")}
                    ariaLabelHide={t("settings.hideApiKey")}
                    onChange={(value) => {
                      setOnlineAsrApiKeyState(value);
                      onlineAsrKeySave.schedule(value);
                    }}
                  />
                </div>
              </div>
            )}
            {/* Model Directory */}
            {engine !== "glm-asr" && (
              <div className="settings-column" style={{ gap: 6, marginTop: 8 }}>
                <div className="settings-row" style={{ gap: 6, alignItems: "center" }}>
                  <HardDrive size={13} style={{ opacity: 0.6, flexShrink: 0 }} />
                  <span className="settings-option-desc" style={{ flex: 1 }}>{t("settings.modelStorageDir")}</span>
                  {modelsDirCustom && (
                    <button
                      className="theme-btn"
                      style={{ fontSize: 11, padding: "2px 8px", gap: 4 }}
                      disabled={modelsDirMigrating}
                      onClick={async () => {
                        try {
                          setModelsDirMigrating(true);
                          await setModelsDir(null, false);
                          const info = await getModelsDir();
                          setModelsDirState(info.path);
                          setModelsDirCustom(info.is_custom);
                          toast.success(t("toast.modelsDirResetDefault"));
                        } catch (e) {
                          toast.error(e instanceof Error ? e.message : t("toast.modelsDirResetFailed"));
                        } finally {
                          setModelsDirMigrating(false);
                        }
                      }}
                    >
                      <RotateCcw size={11} />
                      {t("settings.restoreDefault")}
                    </button>
                  )}
                  <button
                    className="theme-btn"
                    style={{ fontSize: 11, padding: "2px 8px", gap: 4 }}
                    disabled={modelsDirMigrating}
                    onClick={async () => {
                      try {
                        const folder = await pickFolder();
                        if (!folder) return;
                        setModelsDirMigrating(true);
                        await setModelsDir(folder, true);
                        const info = await getModelsDir();
                        setModelsDirState(info.path);
                        setModelsDirCustom(info.is_custom);
                        toast.success(t("toast.modelsDirUpdated"));
                        retryModel();
                      } catch (e) {
                        toast.error(e instanceof Error ? e.message : t("toast.modelsDirChangeFailed"));
                        retryModel();
                      } finally {
                        setModelsDirMigrating(false);
                        setModelsMigrateMsg("");
                      }
                    }}
                  >
                    <FolderOpen size={11} />
                    {modelsDirMigrating ? (modelsMigrateMsg || t("settings.migrating")) : t("common.change")}
                  </button>
                </div>
                <span
                  className="settings-option-desc"
                  style={{ fontSize: 11, opacity: 0.5, wordBreak: "break-all", userSelect: "text" }}
                >
                  {modelsDir || t("common.loading")}
                </span>
              </div>
            )}
          </section>

          {/* Hotkey */}
          <section
            className="settings-card"
            data-nav-id="hotkey"
            style={{
              animationDelay: "100ms",
              position: "relative",
              zIndex: picker.isOpen("recordingMode") ? 9 : 1,
            }}
          >
            <div className="settings-section-header">
              <Keyboard size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.hotkeySection")}</h2>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => mainHotkeyCapture.startCapture()}
                  disabled={mainHotkeyCapture.saving}
                  data-capturing={mainHotkeyCapture.capturing}
                  style={{
                    cursor: mainHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: mainHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {mainHotkeyCapture.capturing ? t("settings.pressCombo") : hotkeyDisplay}
                </button>
                <button
                  className="btn-ghost"
                  onClick={handleResetHotkey}
                  disabled={mainHotkeyCapture.saving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: mainHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: mainHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {t("settings.resetF2")}
                </button>
              </div>
              <p className="settings-hint">
                {t("settings.hotkeyHint")}
              </p>
              <div className="settings-column" style={{ gap: 6, marginTop: 8 }}>
                <span className="settings-option-desc">{t("settings.recordingMode")}</span>
                <div
                  ref={picker.setRef("recordingMode")}
                  style={{
                    position: "relative",
                    zIndex: picker.isOpen("recordingMode") ? 2 : 1,
                  }}
                >
                  <button
                    type="button"
                    className="picker-trigger"
                    data-open={picker.isOpen("recordingMode")}
                    aria-haspopup="listbox"
                    aria-expanded={picker.isOpen("recordingMode")}
                    aria-label={t("settings.recordingModeLabel")}
                    onClick={() => {
                      picker.toggle("recordingMode");
                    }}
                  >
                    <span className="picker-trigger-copy">
                      <strong>{t(selectedRecordingModeOption.labelKey)}</strong>
                      <span>{t(selectedRecordingModeOption.descKey)}</span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" />
                  </button>
                  {picker.isOpen("recordingMode") && (
                    <div className={picker.popoverClass("recordingMode")}>
                      <div className="picker-list" role="listbox">
                        {recordingModeOptions.map((option) => (
                          <button
                            key={`recording-mode-${option.key}`}
                            type="button"
                            className="picker-option"
                            data-active={recordingMode === option.key}
                            onClick={() => handleRecordingModeChange(option.key)}
                          >
                            <span className="picker-option-copy">
                              <strong>{t(option.labelKey)}</strong>
                              <span>{t(option.descKey)}</span>
                            </span>
                            {recordingMode === option.key ? <Check size={14} className="icon-accent" /> : null}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              </div>
              {hotkeyDiagnostic?.systemConflict && (
                <p className="settings-error" style={{ opacity: 0.85 }}>
                  ⚠ {hotkeyDiagnostic.systemConflict}
                </p>
              )}
              {hotkeyDiagnostic?.warning && <p className="settings-hint">{hotkeyDiagnostic.warning}</p>}
              {hotkeyStatusError && <p className="settings-error">{hotkeyStatusError}</p>}
            </div>
          </section>

          {/* Microphone */}
          <section
            className="settings-card"
            data-nav-id="microphone"
            style={{
              animationDelay: "125ms",
              position: "relative",
              zIndex: picker.isOpen("microphone") ? 8 : 1,
            }}
          >
            <div className="settings-section-header">
              <Mic size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.microphone")}</h2>
              <div className="settings-row" style={{ marginLeft: "auto", gap: 8, flex: "0 0 auto" }}>
                <span className="settings-option-desc" style={{ whiteSpace: "nowrap" }}>{t("settings.levelMonitor")}</span>
                <button
                  role="switch"
                  aria-checked={micLevelMonitorEnabled}
                  aria-label={t("settings.micLevelMonitor")}
                  onClick={() => handleMicLevelMonitorToggle(!micLevelMonitorEnabled)}
                  className="toggle-switch"
                  style={{
                    background: micLevelMonitorEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                  }}
                >
                  <div className="toggle-knob" style={{ transform: micLevelMonitorEnabled ? "translateX(20px)" : "translateX(0)" }} />
                </button>
              </div>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <div ref={picker.setRef("microphone")} style={{ position: "relative", flex: 1, minWidth: 0 }}>
                  <button
                    type="button"
                    className="picker-trigger microphone-select"
                    data-open={picker.isOpen("microphone")}
                    aria-haspopup="listbox"
                    aria-expanded={picker.isOpen("microphone")}
                    aria-label={t("settings.selectMic")}
                    disabled={deviceListLoading}
                    onClick={() => {
                      if (deviceListLoading) return;
                      picker.toggle("microphone");
                    }}
                    style={{
                      opacity: deviceListLoading ? 0.7 : 1,
                      cursor: deviceListLoading ? "wait" : "pointer",
                    }}
                  >
                    <span className="picker-trigger-copy">
                      <strong>{selectedInputDeviceOption.label}</strong>
                      <span>{selectedInputDeviceOption.desc}</span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" />
                  </button>
                  {picker.isOpen("microphone") && (
                    <div className={picker.popoverClass("microphone")}>
                      <div className="picker-list" role="listbox">
                        <button
                          type="button"
                          className="picker-option"
                          data-active={!selectedInputDeviceName}
                          onClick={() => { void handleInputDeviceChange(""); }}
                        >
                          <span className="picker-option-copy">
                            <strong>{t("settings.followSystemMic")}</strong>
                            <span>
                              {inputDevices.find((device) => device.isDefault)?.name ?? t("settings.autoUseDefault")}
                            </span>
                          </span>
                          {!selectedInputDeviceName ? <Check size={14} className="icon-accent" /> : null}
                        </button>
                        {inputDevices.map((device) => (
                          <button
                            key={device.name}
                            type="button"
                            className="picker-option"
                            data-active={selectedInputDeviceName === device.name}
                            onClick={() => { void handleInputDeviceChange(device.name); }}
                          >
                            <span className="picker-option-copy">
                              <strong>{device.name}</strong>
                              <span>{device.isDefault ? t("settings.systemDefaultDevice") : t("settings.canSelect")}</span>
                            </span>
                            {selectedInputDeviceName === device.name ? <Check size={14} className="icon-accent" /> : null}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
                <button
                  className="btn-ghost"
                  disabled={deviceListLoading}
                  onClick={() => { void refreshInputDevices(); }}
                  style={{ fontSize: 12, padding: "8px 10px", opacity: deviceListLoading ? 0.7 : 1 }}
                >
                  {t("common.refresh")}
                </button>
                <button className="test-btn" onClick={async () => {
                  try {
                    setMicMonitorReady(false);
                    setMicLevel(0);
                    await stopMicrophoneLevelMonitor().catch(() => undefined);
                    const msg = await testMicrophone();
                    toast.success(msg);
                    if (micLevelMonitorEnabled && !isRecording) {
                      await startMicrophoneLevelMonitor();
                      setMicMonitorReady(true);
                    }
                  } catch {
                    toast.error(t("toast.micTestFailed"));
                  }
                }}>{t("common.test")}</button>
              </div>
              <div className="mic-level-shell" aria-label={t("settings.micLevelPreview")}>
                <div className="mic-level-fill" style={{ width: `${Math.round(micLevel * 100)}%` }} />
              </div>
              <div className="settings-row" style={{ gap: 10 }}>
                <span className="settings-hint">
                  {!micLevelMonitorEnabled
                    ? t("settings.micMonitorOff")
                    : isRecording
                    ? t("settings.micRecordingPaused")
                    : micMonitorReady
                      ? t("settings.micSpeakToTest")
                      : t("settings.micNotStarted")}
                </span>
                <span className="settings-option-desc">{Math.round(micLevel * 100)}%</span>
              </div>
              {selectedDeviceMissing && (
                <p className="settings-error">
                  {t("settings.savedMicUnavailable")}
                </p>
              )}
            </div>
          </section>

          {/* Input Method */}
          <section className="settings-card" data-nav-id="input" style={{ animationDelay: "150ms" }}>
            <div className="settings-section-header">
              <ClipboardPaste size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.inputMethod")}</h2>
            </div>
            <div className="settings-grid-2">
              {inputOptions.map(({ key, icon: Icon, labelKey, descKey }) => (
                <button
                  key={key}
                  className="theme-btn settings-option-btn"
                  aria-label={t(labelKey)}
                  aria-pressed={inputMethod === key}
                  onClick={() => {
                    setInputMethod(key);
                    writeLocalStorage(INPUT_METHOD_KEY, key);
                    setInputMethodCommand(key).catch(() => {});
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{t(labelKey)}</span>
                  <span className="settings-option-desc">{t(descKey)}</span>
                </button>
              ))}
            </div>
            <div className="settings-row" style={{ marginTop: 6 }}>
              <span className="permission-label">{t("settings.recordingSound")}</span>
              <button
                role="switch"
                aria-checked={soundEnabled}
                aria-label={t("settings.recordingSound")}
                onClick={() => {
                  const next = !soundEnabled;
                  setSoundEnabledState(next);
                  writeLocalStorage(SOUND_ENABLED_KEY, String(next));
                  setSoundEnabled(next).catch(() => {});
                }}
                className="toggle-switch"
                style={{
                  background: soundEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                }}
              >
                <div className="toggle-knob" style={{ transform: soundEnabled ? "translateX(20px)" : "translateX(0)" }} />
              </button>
            </div>
          </section>

          {/* AI Polish + LLM Backend */}
          <section
            className="settings-card"
            data-nav-id="ai-polish"
            style={{
              animationDelay: "200ms",
              position: "relative",
              zIndex: picker.isOpen("provider") || picker.isOpen("model") || picker.isOpen("polishReasoning") ? 8 : 1,
            }}
          >
            <div className="settings-section-header">
              <Sparkles size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.aiPolish")}</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row">
                <span className="permission-label">{t("settings.enableAiPolish")}</span>
                <button
                  role="switch"
                  aria-checked={aiPolishEnabled}
                  aria-label={t("settings.enableAiPolish")}
                  onClick={() => {
                    const next = !aiPolishEnabled;
                    aiPolishKeySave.cancel();
                    setAiPolishEnabled(next);
                    writeLocalStorage(AI_POLISH_ENABLED_KEY, String(next));
                    setAiPolishConfig(next, aiPolishApiKey).catch(() => {});
                  }}
                  className="toggle-switch"
                  style={{
                    background: aiPolishEnabled ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                  }}
                >
                  <div className="toggle-knob" style={{ transform: aiPolishEnabled ? "translateX(20px)" : "translateX(0)" }} />
                </button>
              </div>

              <div className="settings-row">
                <div className="permission-item" style={{ gap: 8 }}>
                  <Monitor size={14} className="icon-tertiary" />
                  <div className="settings-column" style={{ gap: 2 }}>
                    <span className="permission-label">{t("settings.screenContext")}</span>
                    <span className="settings-hint" style={{ margin: 0 }}>
                      {t("settings.screenContextPolishHint")}
                    </span>
                  </div>
                </div>
                <button
                  role="switch"
                  aria-checked={aiPolishScreenContextEnabled}
                  aria-label={t("settings.screenContext")}
                  onClick={() => handleAiPolishScreenContextToggle(!aiPolishScreenContextEnabled)}
                  className="toggle-switch"
                  style={{
                    background: aiPolishScreenContextEnabled
                      ? "var(--color-accent)"
                      : "var(--color-bg-tertiary)",
                    flexShrink: 0,
                  }}
                >
                  <div
                    className="toggle-knob"
                    style={{
                      transform: aiPolishScreenContextEnabled
                        ? "translateX(20px)"
                        : "translateX(0)",
                    }}
                  />
                </button>
              </div>

              <div className="settings-column" style={{ gap: 10 }}>
                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">{t("settings.provider")}</span>
                  <div className="picker-shell" ref={picker.setRef("provider")}>
                    <button
                      type="button"
                      className="picker-trigger"
                      data-open={picker.isOpen("provider")}
                      aria-haspopup="listbox"
                      aria-expanded={picker.isOpen("provider")}
                      aria-label={t("settings.selectProvider")}
                      onClick={() => {
                        picker.toggle("provider");
                      }}
                    >
                      <span className="picker-trigger-copy">
                        <strong>{currentLlmPreset.label}</strong>
                        <span>{customBaseUrl || currentLlmPreset.baseUrl}</span>
                      </span>
                      <ChevronsUpDown size={14} className="icon-tertiary" />
                    </button>
                    {picker.isOpen("provider") && (
                      <div className={picker.popoverClass("provider")}>
                        <input
                          ref={providerSearchInputRef}
                          type="text"
                          className="settings-input picker-search-input"
                          placeholder={t("settings.searchProvider")}
                          aria-label={t("settings.searchProviderLabel")}
                          value={providerSearch}
                          onChange={(e) => setProviderSearch(e.target.value)}
                        />
                        <div className="picker-list" role="listbox">
                          {filteredProviderOptions.length > 0 ? filteredProviderOptions.map(({ key, label, desc, baseUrl, isCustom }) => (
                            <button
                              key={key}
                              type="button"
                              className="picker-option"
                              data-active={llmProvider === key}
                              onClick={() => { void handleProviderSelect(key); }}
                            >
                              <span className="picker-option-copy">
                                <strong>{label}</strong>
                                <span>{desc}</span>
                                <code>{baseUrl}</code>
                              </span>
                              <span style={{ display: "flex", alignItems: "center", gap: 4, flexShrink: 0 }}>
                                {llmProvider === key ? <Check size={14} className="icon-accent" /> : null}
                                {isCustom && (
                                  <span
                                    role="button"
                                    tabIndex={0}
                                    style={{ padding: 2, cursor: "pointer", opacity: 0.5 }}
                                    title={t("settings.deleteProvider")}
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      void removeCustomProvider(key).then(async () => { await refreshProfile(); await refreshAiPolishKey(); });
                                    }}
                                  >
                                    <Trash2 size={12} />
                                  </span>
                                )}
                              </span>
                            </button>
                          )) : (
                            <div className="picker-empty">{t("settings.noMatchingProvider")}</div>
                          )}
                          {/* 添加自定义服务商 */}
                          {!addingProvider ? (
                            <button
                              type="button"
                              className="picker-option"
                              style={{ borderTop: "1px solid var(--color-border)", opacity: 0.8 }}
                              onClick={(e) => { e.stopPropagation(); setAddingProvider(true); }}
                            >
                              <span className="picker-option-copy">
                                <strong><Plus size={12} style={{ verticalAlign: -1, marginRight: 4 }} />{t("settings.addCustomProvider")}</strong>
                              </span>
                            </button>
                          ) : (
                            <div
                              style={{ padding: "8px 10px", display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--color-border)" }}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <input className="settings-input" placeholder={t("settings.providerName")} aria-label={t("settings.providerNameLabel")} value={newProviderName} onChange={(e) => setNewProviderName(e.target.value)} style={{ fontSize: 12 }} />
                              <input className="settings-input" placeholder="Base URL" aria-label={t("settings.providerBaseUrlLabel")} value={newProviderBaseUrl} onChange={(e) => setNewProviderBaseUrl(e.target.value)} style={{ fontSize: 12 }} />
                              <input className="settings-input" placeholder={t("settings.defaultModel")} aria-label={t("settings.defaultModelLabel")} value={newProviderModel} onChange={(e) => setNewProviderModel(e.target.value)} style={{ fontSize: 12 }} />
                              <select
                                className="settings-input"
                                aria-label={t("settings.apiFormatLabel")}
                                value={newProviderFormat}
                                onChange={(e) => setNewProviderFormat(e.target.value as ApiFormat)}
                                style={{ fontSize: 12 }}
                              >
                                <option value="openai_compat">{t("settings.openaiCompat")}</option>
                                <option value="anthropic">Anthropic</option>
                              </select>
                              <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                                <button className="btn-ghost" style={{ fontSize: 11, padding: "4px 8px" }} onClick={() => { setAddingProvider(false); setNewProviderName(""); setNewProviderBaseUrl(""); setNewProviderModel(""); setNewProviderFormat("openai_compat"); }}>{t("common.cancel")}</button>
                                <button
                                  className="btn-ghost"
                                  style={{ fontSize: 11, padding: "4px 8px" }}
                                  disabled={!newProviderName.trim() || !newProviderBaseUrl.trim()}
                                  onClick={() => {
                                    void addCustomProvider(newProviderName.trim(), newProviderBaseUrl.trim(), newProviderModel.trim(), newProviderFormat).then(async (id) => {
                                      setAddingProvider(false);
                                      const baseUrl = newProviderBaseUrl.trim();
                                      const model = newProviderModel.trim();
                                      setNewProviderName("");
                                      setNewProviderBaseUrl("");
                                      setNewProviderModel("");
                                      setNewProviderFormat("openai_compat");
                                      // 先刷新 profile 拿到最新 customProviders，再切换
                                      await refreshProfile();
                                      // 直接设置状态，不依赖 resolveProviderDraft（避免竞态）
                                      setLlmProvider(id);
                                      setCustomBaseUrl(baseUrl);
                                      setCustomModel(model);
                                      setAssistantModel(model);
                                      updateProviderDraft(id, baseUrl, model);
                                      await setLlmProviderConfig(
                                        id,
                                        baseUrl || undefined,
                                        model || undefined,
                                        polishReasoningMode,
                                        assistantReasoningMode,
                                        assistantUseSeparateModel,
                                        model,
                                      ).catch(() => {});
                                      await refreshAiPolishKey();
                                    });
                                  }}
                                >{t("common.add")}</button>
                              </div>
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">{t("settings.baseUrl")}</span>
                  <input
                    type="text"
                    className="settings-input"
                    placeholder={t("settings.baseUrlPlaceholder")}
                    aria-label={t("settings.baseUrlLabel")}
                    value={customBaseUrl}
                    readOnly={!providerSupportsCustomEndpoint}
                    onChange={(e) => {
                      if (!providerSupportsCustomEndpoint) return;
                      const nextBaseUrl = e.target.value;
                      setCustomBaseUrl(nextBaseUrl);
                      updateProviderDraft(llmProvider, nextBaseUrl, customModel);
                      llmConfigSave.schedule(
                        llmProvider,
                        nextBaseUrl,
                        customModel,
                        polishReasoningMode,
                        assistantReasoningMode,
                        assistantUseSeparateModel,
                        assistantModel,
                      );
                    }}
                  />
                  <p className="settings-hint">
                    {providerSupportsCustomEndpoint
                      ? t("settings.baseUrlCustomHint")
                      : t("settings.baseUrlFixedHint")}
                  </p>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">{t("settings.apiKey")}</span>
                  <SecretInput
                    value={aiPolishApiKey}
                    placeholder={`${currentLlmPreset.label} API Key`}
                    ariaLabelShow={t("settings.showApiKey")}
                    ariaLabelHide={t("settings.hideApiKey")}
                    onChange={(value) => {
                      setAiPolishApiKey(value);
                      aiPolishKeySave.schedule(value, aiPolishEnabled);
                    }}
                  />
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <div className="settings-row">
                    <span className="settings-option-desc">{t("settings.modelLabel")}</span>
                    <span className="settings-option-desc">{filteredAiModels.length}/{aiModels.length}</span>
                  </div>
                  <div className="picker-shell" ref={picker.setRef("model")}>
                    <div className="picker-inline-row">
                      <input
                        type="text"
                        className="settings-input"
                        placeholder={t("settings.modelNamePlaceholder")}
                        aria-label={t("settings.modelNameLabel")}
                        value={customModel}
                        onChange={(e) => {
                          const nextModel = e.target.value;
                          setCustomModel(nextModel);
                          updateProviderDraft(llmProvider, customBaseUrl, nextModel);
                          llmConfigSave.schedule(
                            llmProvider,
                            customBaseUrl,
                            nextModel,
                            polishReasoningMode,
                            assistantReasoningMode,
                            assistantUseSeparateModel,
                            assistantModel,
                          );
                          if (!assistantUseSeparateModel) {
                            setAssistantModel(nextModel);
                          }
                        }}
                      />
                      <button
                        type="button"
                        className="picker-inline-button"
                        data-open={picker.isOpen("model")}
                        aria-haspopup="listbox"
                        aria-expanded={picker.isOpen("model")}
                        onClick={() => {
                          picker.toggle("model");
                        }}
                        aria-label={t("settings.openModelList")}
                        title={t("settings.openModelList")}
                      >
                        <ChevronsUpDown size={14} className="icon-tertiary" />
                      </button>
                    </div>
                    <p className="settings-hint" style={{ margin: 0 }}>
                      {selectedAiModel?.ownedBy || (aiModels.length > 0 ? t("settings.availableModels", { count: aiModels.length }) : t("settings.canInputModelName"))}
                    </p>
                    {picker.isOpen("model") && (
                      <div className={picker.popoverClass("model")}>
                        <div className="picker-toolbar">
                          <input
                            ref={modelSearchInputRef}
                            type="text"
                            className="settings-input picker-search-input"
                            placeholder={t("settings.searchModelPlaceholder")}
                            aria-label={t("settings.searchModelLabel")}
                            value={aiModelSearch}
                            onChange={(e) => setAiModelSearch(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && aiModelSearch.trim()) {
                                e.preventDefault();
                                handleModelSelect(aiModelSearch);
                              }
                            }}
                          />
                          <button
                            type="button"
                            className="btn-ghost"
                            onClick={() => { void refreshAiModels(); }}
                            disabled={aiModelsLoading}
                            style={{ fontSize: 12, padding: "8px 10px", opacity: aiModelsLoading ? 0.7 : 1 }}
                          >
                            {aiModelsLoading ? t("settings.fetching") : t("common.refresh")}
                          </button>
                        </div>
                        <p className="settings-hint" style={{ margin: 0 }}>
                          {aiModelsSourceUrl ? t("settings.modelSourceUrl", { url: aiModelsSourceUrl }) : t("settings.autoFetchHint")}
                        </p>
                        {aiModelSearch.trim() ? (
                          <button
                            type="button"
                            className="picker-option picker-option-action"
                            onClick={() => handleModelSelect(aiModelSearch)}
                          >
                            <span className="picker-option-copy">
                              <strong>{t("settings.useAsModel", { name: aiModelSearch.trim() })}</strong>
                              <span>{t("settings.asCurrentModelName")}</span>
                            </span>
                          </button>
                        ) : null}
                        <div className="picker-list" role="listbox">
                          {filteredAiModels.length > 0 ? filteredAiModels.map((model) => (
                            <button
                              key={model.id}
                              type="button"
                              className="picker-option"
                              data-active={customModel === model.id}
                              onClick={() => handleModelSelect(model.id)}
                            >
                              <span className="picker-option-copy">
                                <strong>{model.id}</strong>
                                <span>{model.ownedBy || currentLlmPreset.label}</span>
                              </span>
                              {customModel === model.id ? <Check size={14} className="icon-accent" /> : null}
                            </button>
                          )) : (
                            <div className="picker-empty">
                              {aiModelsLoading
                                ? t("settings.fetchModelsFromApi")
                                : aiModelsError || t("settings.fillApiKeyToLoadModels")}
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">{t("settings.polishReasoningMode")}</span>
                  <div ref={picker.setRef("polishReasoning")} style={{ position: "relative" }}>
                    <button
                      type="button"
                      className="picker-trigger"
                      data-open={picker.isOpen("polishReasoning")}
                      aria-haspopup="listbox"
                      aria-expanded={picker.isOpen("polishReasoning")}
                      aria-label={t("settings.polishReasoningLabel")}
                      disabled={polishReasoningModeDisabled}
                      onClick={() => {
                        if (polishReasoningModeDisabled) return;
                        picker.toggle("polishReasoning");
                      }}
                      title={polishReasoningModeHint}
                      style={{
                        opacity: polishReasoningModeDisabled ? 0.55 : 1,
                        cursor: polishReasoningModeDisabled ? "not-allowed" : "pointer",
                      }}
                    >
                      <span className="picker-trigger-copy">
                        <strong>{t(selectedPolishReasoningOption.labelKey)}</strong>
                        <span>{t(selectedPolishReasoningOption.descKey)}</span>
                      </span>
                      <ChevronsUpDown size={14} className="icon-tertiary" />
                    </button>
                    {picker.isOpen("polishReasoning") && (
                      <div className={picker.popoverClass("polishReasoning")}>
                        <div className="picker-list" role="listbox">
                          {reasoningModeOptions.map((option) => (
                            <button
                              key={option.key}
                              type="button"
                              className="picker-option"
                              data-active={polishReasoningMode === option.key}
                              onClick={() => handlePolishReasoningModeChange(option.key)}
                            >
                              <span className="picker-option-copy">
                                <strong>{t(option.labelKey)}</strong>
                                <span>{t(option.descKey)}</span>
                              </span>
                              {polishReasoningMode === option.key ? <Check size={14} className="icon-accent" /> : null}
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                  <p className="settings-hint" style={{ margin: 0 }}>
                    {polishReasoningModeHint}
                  </p>
                </div>
              </div>

              <div className="settings-column" style={{ gap: 6 }}>
                <span className="settings-option-desc">{t("settings.customPrompt")}</span>
                <textarea
                  className="settings-input"
                  placeholder={t("settings.customPromptPlaceholder")}
                  aria-label={t("settings.customPromptLabel")}
                  value={customPromptState}
                  onChange={(e) => handleCustomPromptChange(e.target.value)}
                  rows={3}
                  style={{ resize: "vertical", minHeight: 60, fontFamily: "inherit" }}
                />
                <p className="settings-hint" style={{ margin: 0 }}>
                  {t("settings.customPromptHint")}
                </p>
              </div>

              <p className="settings-hint">
                {t("settings.aiPolishLearnHint")}
              </p>
            </div>
          </section>

          <section
            className="settings-card"
            data-nav-id="assistant"
            style={{
              animationDelay: "206ms",
              position: "relative",
              zIndex: picker.isOpen("assistantModel") || picker.isOpen("assistantReasoning") || picker.isOpen("webSearchProvider") ? 8 : 1,
            }}
          >
            <div className="settings-section-header">
              <Sparkles size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.assistant")}</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => assistantHotkeyCapture.startCapture()}
                  disabled={assistantHotkeyCapture.saving}
                  data-capturing={assistantHotkeyCapture.capturing}
                  style={{
                    cursor: assistantHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: assistantHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {assistantHotkeyCapture.capturing
                    ? t("settings.pressAssistantHotkey")
                    : assistantHotkeyDisplay || t("settings.noAssistantHotkey")}
                </button>
                <button
                  className="btn-ghost"
                  onClick={() => handleClearHotkey(setAssistantHotkey, setAssistantHotkeyDisplay, t("settings.assistantHotkeyLabel"), assistantHotkeyCapture.cancelCapture)}
                  disabled={assistantHotkeyCapture.saving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: assistantHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: assistantHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {t("common.clear")}
                </button>
              </div>
              <p className="settings-hint" style={{ margin: 0 }}>
                {t("settings.assistantHint")}
              </p>
              <div className="settings-row">
                <div className="permission-item" style={{ gap: 8 }}>
                  <Monitor size={14} className="icon-tertiary" />
                  <div className="settings-column" style={{ gap: 2 }}>
                    <span className="permission-label">{t("settings.screenContext")}</span>
                    <span className="settings-hint" style={{ margin: 0 }}>
                      {t("settings.screenContextAssistantHint")}
                    </span>
                  </div>
                </div>
                <button
                  role="switch"
                  aria-checked={assistantScreenContextEnabled}
                  aria-label={t("settings.assistantScreenContext")}
                  onClick={() => handleAssistantScreenContextToggle(!assistantScreenContextEnabled)}
                  className="toggle-switch"
                  style={{
                    background: assistantScreenContextEnabled
                      ? "var(--color-accent)"
                      : "var(--color-bg-tertiary)",
                    flexShrink: 0,
                  }}
                >
                  <div
                    className="toggle-knob"
                    style={{
                      transform: assistantScreenContextEnabled
                        ? "translateX(20px)"
                        : "translateX(0)",
                    }}
                  />
                </button>
              </div>
              <div className="settings-row">
                <div className="permission-item" style={{ gap: 8 }}>
                  <Sparkles size={14} className="icon-tertiary" />
                  <div className="settings-column" style={{ gap: 2 }}>
                    <span className="permission-label">{t("settings.useSeparateConfig")}</span>
                    <span className="settings-hint" style={{ margin: 0 }}>
                      {t("settings.separateConfigHint")}
                    </span>
                  </div>
                </div>
                <button
                  role="switch"
                  aria-checked={assistantUseSeparateModel}
                  aria-label={t("settings.assistantSeparateConfig")}
                  onClick={() => handleAssistantModelToggle(!assistantUseSeparateModel)}
                  className="toggle-switch"
                  style={{
                    background: assistantUseSeparateModel
                      ? "var(--color-accent)"
                      : "var(--color-bg-tertiary)",
                    flexShrink: 0,
                  }}
                >
                  <div
                    className="toggle-knob"
                    style={{
                      transform: assistantUseSeparateModel
                        ? "translateX(20px)"
                        : "translateX(0)",
                    }}
                  />
                </button>
              </div>

              {assistantUseSeparateModel ? (
                <div className="settings-column" style={{ gap: 6 }}>
                  {/* 助手供应商选择器 */}
                  <span className="settings-option-desc">{t("settings.assistantProvider")}</span>
                  <div className="picker-shell" ref={picker.setRef("assistantProvider")}>
                    <button
                      type="button"
                      className="picker-trigger"
                      onClick={() => {
                        picker.toggle("assistantProvider");
                      }}
                      aria-haspopup="listbox"
                      aria-expanded={picker.isOpen("assistantProvider")}
                    >
                      <span className="picker-trigger-copy">
                        <strong>{currentAssistantPreset.label}</strong>
                        <span>{currentAssistantPreset.baseUrl}</span>
                      </span>
                      <ChevronsUpDown size={14} className="icon-tertiary" />
                    </button>
                    {picker.isOpen("assistantProvider") && (
                      <div className={picker.popoverClass("assistantProvider")}>
                        <div className="picker-toolbar">
                          <input
                            type="text"
                            className="settings-input picker-search-input"
                            placeholder={t("settings.searchAssistantProvider")}
                            aria-label={t("settings.searchAssistantProviderLabel")}
                            value={assistantProviderSearch}
                            onChange={(e) => setAssistantProviderSearch(e.target.value)}
                            autoFocus
                          />
                        </div>
                        <div className="picker-list" role="listbox">
                          {filteredAssistantProviderOptions.map((opt) => (
                            <button
                              key={`assistant-provider-${opt.key}`}
                              type="button"
                              className="picker-option"
                              data-active={assistantProvider === opt.key}
                              onClick={() => handleAssistantProviderSelect(opt.key)}
                            >
                              <span className="picker-option-copy">
                                <strong>{opt.label}</strong>
                                <span>{opt.desc}</span>
                              </span>
                              {assistantProvider === opt.key ? <Check size={14} className="icon-accent" /> : null}
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>

                  {/* 助手独立 API Key（仅当 provider 与润色不同时显示） */}
                  {assistantProviderDiffers ? (
                    <div className="settings-column" style={{ gap: 4 }}>
                      <span className="settings-option-desc">{currentAssistantPreset.label} API Key</span>
                      <input
                        type="password"
                        className="settings-input"
                        placeholder={`${currentAssistantPreset.label} API Key`}
                        aria-label={t("settings.assistantApiKey")}
                        value={assistantApiKeyState}
                        autoComplete="off"
                        onChange={(e) => {
                          const value = e.target.value;
                          setAssistantApiKeyState(value);
                          assistantKeySave.schedule(value);
                        }}
                      />
                    </div>
                  ) : null}

                  {/* 助手模型选择器 */}
                  <div className="settings-row">
                    <span className="settings-option-desc">{t("settings.assistantModel")}</span>
                    <span className="settings-option-desc">{filteredAssistantModels.length}/{effectiveAssistantModels.length}</span>
                  </div>
                  <div className="picker-shell" ref={picker.setRef("assistantModel")}>
                    <div className="picker-inline-row">
                      <input
                        type="text"
                        className="settings-input"
                        placeholder={t("settings.assistantModelPlaceholder")}
                        aria-label={t("settings.assistantModelLabel")}
                        value={assistantModel}
                        onChange={(e) => {
                          const nextModel = e.target.value;
                          setAssistantModel(nextModel);
                          llmConfigSave.schedule(
                            llmProvider,
                            customBaseUrl,
                            customModel,
                            polishReasoningMode,
                            assistantReasoningMode,
                            true,
                            nextModel,
                            assistantProvider,
                          );
                        }}
                      />
                      <button
                        type="button"
                        className="picker-inline-button"
                        data-open={picker.isOpen("assistantModel")}
                        aria-haspopup="listbox"
                        aria-expanded={picker.isOpen("assistantModel")}
                        onClick={() => {
                          picker.toggle("assistantModel");
                        }}
                        aria-label={t("settings.openAssistantModelList")}
                        title={t("settings.openAssistantModelList")}
                      >
                        <ChevronsUpDown size={14} className="icon-tertiary" />
                      </button>
                    </div>
                    <p className="settings-hint" style={{ margin: 0 }}>
                      {selectedAssistantAiModel?.ownedBy || (effectiveAssistantModels.length > 0 ? t("settings.availableModels", { count: effectiveAssistantModels.length }) : t("settings.canInputModelName"))}
                    </p>
                    {picker.isOpen("assistantModel") && (
                      <div className={picker.popoverClass("assistantModel")}>
                        <div className="picker-toolbar">
                          <input
                            ref={assistantModelSearchInputRef}
                            type="text"
                            className="settings-input picker-search-input"
                            placeholder={t("settings.searchModelPlaceholder")}
                            aria-label={t("settings.searchAssistantModel")}
                            value={assistantModelSearch}
                            onChange={(e) => setAssistantModelSearch(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && assistantModelSearch.trim()) {
                                e.preventDefault();
                                handleAssistantModelSelect(assistantModelSearch);
                              }
                            }}
                          />
                          <button
                            type="button"
                            className="btn-ghost"
                            onClick={() => { void (assistantProviderDiffers ? refreshAssistantModels() : refreshAiModels()); }}
                            disabled={assistantProviderDiffers ? assistantModelsLoading : aiModelsLoading}
                            style={{ fontSize: 12, padding: "8px 10px", opacity: (assistantProviderDiffers ? assistantModelsLoading : aiModelsLoading) ? 0.7 : 1 }}
                          >
                            {(assistantProviderDiffers ? assistantModelsLoading : aiModelsLoading) ? t("settings.fetching") : t("common.refresh")}
                          </button>
                        </div>
                        {assistantModelSearch.trim() ? (
                          <button
                            type="button"
                            className="picker-option picker-option-action"
                            onClick={() => handleAssistantModelSelect(assistantModelSearch)}
                          >
                            <span className="picker-option-copy">
                              <strong>{t("settings.useAsModel", { name: assistantModelSearch.trim() })}</strong>
                              <span>{t("settings.asAssistantModelName")}</span>
                            </span>
                          </button>
                        ) : null}
                        <div className="picker-list" role="listbox">
                          {filteredAssistantModels.length > 0 ? filteredAssistantModels.map((model) => (
                            <button
                              key={`assistant-model-${model.id}`}
                              type="button"
                              className="picker-option"
                              data-active={assistantModel === model.id}
                              onClick={() => handleAssistantModelSelect(model.id)}
                            >
                              <span className="picker-option-copy">
                                <strong>{model.id}</strong>
                                <span>{model.ownedBy || currentAssistantPreset.label}</span>
                              </span>
                              {assistantModel === model.id ? <Check size={14} className="icon-accent" /> : null}
                            </button>
                          )) : (
                            <div className="picker-empty">
                              {(assistantProviderDiffers ? assistantModelsLoading : aiModelsLoading)
                                ? t("settings.fetchModelsFromApi")
                                : (assistantProviderDiffers && !assistantApiKeyState.trim())
                                  ? t("settings.fillAssistantApiKey")
                                  : aiModelsError || t("settings.fillApiKeyToLoadModels")}
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              ) : (
                <p className="settings-hint" style={{ margin: 0 }}>
                  {t("settings.sharedProviderAndModel", { provider: currentLlmPreset.label, model: customModel || currentLlmPreset.defaultModel })}
                </p>
              )}

              <div className="settings-column" style={{ gap: 6 }}>
                <span className="settings-option-desc">{t("settings.assistantReasoningMode")}</span>
                <div ref={picker.setRef("assistantReasoning")} style={{ position: "relative" }}>
                  <button
                    type="button"
                    className="picker-trigger"
                    data-open={picker.isOpen("assistantReasoning")}
                    aria-haspopup="listbox"
                    aria-expanded={picker.isOpen("assistantReasoning")}
                    aria-label={t("settings.assistantReasoningLabel")}
                    disabled={assistantReasoningModeDisabled}
                    onClick={() => {
                      if (assistantReasoningModeDisabled) return;
                      picker.toggle("assistantReasoning");
                    }}
                    title={assistantReasoningModeHint}
                    style={{
                      opacity: assistantReasoningModeDisabled ? 0.55 : 1,
                      cursor: assistantReasoningModeDisabled ? "not-allowed" : "pointer",
                    }}
                  >
                    <span className="picker-trigger-copy">
                      <strong>{t(selectedAssistantReasoningOption.labelKey)}</strong>
                      <span>{t(selectedAssistantReasoningOption.descKey)}</span>
                    </span>
                    <ChevronsUpDown size={14} className="icon-tertiary" />
                  </button>
                  {picker.isOpen("assistantReasoning") && (
                    <div className={picker.popoverClass("assistantReasoning")}>
                      <div className="picker-list" role="listbox">
                        {reasoningModeOptions.map((option) => (
                          <button
                            key={`assistant-${option.key}`}
                            type="button"
                            className="picker-option"
                            data-active={assistantReasoningMode === option.key}
                            onClick={() => handleAssistantReasoningModeChange(option.key)}
                          >
                            <span className="picker-option-copy">
                              <strong>{t(option.labelKey)}</strong>
                              <span>{t(option.descKey)}</span>
                            </span>
                            {assistantReasoningMode === option.key ? <Check size={14} className="icon-accent" /> : null}
                          </button>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
                <p className="settings-hint" style={{ margin: 0 }}>
                  {assistantReasoningModeHint}
                </p>
              </div>

              <div className="settings-column" style={{ gap: 6 }}>
                <span className="settings-option-desc">{t("settings.customAssistantPrompt")}</span>
                <textarea
                  className="settings-input"
                  placeholder={t("settings.assistantPromptPlaceholder")}
                  aria-label={t("settings.assistantPromptLabel")}
                  value={assistantPromptState}
                  onChange={(e) => handleAssistantPromptChange(e.target.value)}
                  rows={4}
                  style={{ resize: "vertical", minHeight: 84, fontFamily: "inherit" }}
                />
                <p className="settings-hint" style={{ margin: 0 }}>
                  {t("settings.assistantPromptHint")}
                </p>
              </div>

              {/* 联网搜索 */}
              <div className="settings-row" style={{ marginTop: 4 }}>
                <span className="permission-label">{t("settings.webSearchDesc")}</span>
                <button
                  role="switch"
                  aria-checked={webSearchEnabled}
                  aria-label={t("settings.webSearch")}
                  onClick={() => handleWebSearchToggle(!webSearchEnabled)}
                  className="toggle-switch"
                  style={{
                    background: webSearchEnabled
                      ? "var(--color-accent)"
                      : "var(--color-bg-tertiary)",
                  }}
                >
                  <div
                    className="toggle-knob"
                    style={{
                      transform: webSearchEnabled
                        ? "translateX(20px)"
                        : "translateX(0)",
                    }}
                  />
                </button>
              </div>
              <p className="settings-hint" style={{ margin: 0 }}>
                {t("settings.webSearchHint")}
              </p>

              {webSearchEnabled && (
                <div className="settings-column" style={{ gap: 10 }}>
                  {/* 搜索方式（下拉列表） */}
                  <div className="settings-column" style={{ gap: 6 }}>
                    <span className="settings-option-desc">{t("settings.webSearchProvider")}</span>
                    <div ref={picker.setRef("webSearchProvider")} style={{ position: "relative" }}>
                      <button
                        type="button"
                        className="picker-trigger"
                        data-open={picker.isOpen("webSearchProvider")}
                        aria-haspopup="listbox"
                        aria-expanded={picker.isOpen("webSearchProvider")}
                        aria-label={t("settings.webSearchProvider")}
                        onClick={() => picker.toggle("webSearchProvider")}
                      >
                        <span className="picker-trigger-copy">
                          <strong>{t(selectedWebSearchProviderOption.labelKey)}</strong>
                          <span>{t(selectedWebSearchProviderOption.descKey)}</span>
                        </span>
                        <ChevronsUpDown size={14} className="icon-tertiary" />
                      </button>
                      {picker.isOpen("webSearchProvider") && (
                        <div className={picker.popoverClass("webSearchProvider")}>
                          <div className="picker-list" role="listbox">
                            {webSearchProviderOptions.map((option) => (
                              <button
                                key={option.key}
                                type="button"
                                className="picker-option"
                                data-active={webSearchProvider === option.key}
                                onClick={() => handleWebSearchProviderChange(option.key)}
                              >
                                <span className="picker-option-copy">
                                  <strong>{t(option.labelKey)}</strong>
                                  <span>{t(option.descKey)}</span>
                                </span>
                                {webSearchProvider === option.key ? <Check size={14} className="icon-accent" /> : null}
                              </button>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  </div>

                  {/* 搜索结果条数（Exa / Tavily） */}
                  {webSearchProvider !== "model_native" && (
                    <div className="settings-column" style={{ gap: 6 }}>
                      <div className="settings-row">
                        <span className="settings-option-desc">{t("settings.webSearchMaxResults")}</span>
                        <span style={{ fontSize: 12, opacity: 0.7, minWidth: 16, textAlign: "right" }}>{webSearchMaxResults}</span>
                      </div>
                      <input
                        type="range"
                        min={1}
                        max={10}
                        step={1}
                        value={webSearchMaxResults}
                        onChange={(e) => handleWebSearchMaxResultsChange(Number(e.target.value))}
                        style={{ width: "100%" }}
                      />
                    </div>
                  )}

                  {/* Tavily API Key */}
                  {webSearchProvider === "tavily" && (
                    <div className="settings-column" style={{ gap: 6 }}>
                      <span className="settings-option-desc">Tavily API Key</span>
                      <SecretInput
                        value={webSearchApiKey}
                        onChange={(val) => {
                          setWebSearchApiKeyState(val);
                          webSearchKeySave.schedule(val);
                        }}
                        placeholder={t("settings.webSearchTavilyKeyPlaceholder")}
                        aria-label="Tavily API Key"
                      />
                    </div>
                  )}
                </div>
              )}
            </div>
          </section>

          {/* Translation */}
          <section className="settings-card" data-nav-id="translation" style={{ animationDelay: "220ms" }}>
            <div className="settings-section-header">
              <Languages size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.translation")}</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => translationHotkeyCapture.startCapture()}
                  disabled={translationHotkeyCapture.saving}
                  data-capturing={translationHotkeyCapture.capturing}
                  style={{
                    cursor: translationHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: translationHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {translationHotkeyCapture.capturing
                    ? t("settings.pressTranslationHotkey")
                    : translationHotkeyDisplay || t("settings.noTranslationHotkey")}
                </button>
                <button
                  className="btn-ghost"
                  onClick={() => handleClearHotkey(setTranslationHotkey, setTranslationHotkeyDisplay, t("settings.translationHotkeyLabel"), translationHotkeyCapture.cancelCapture)}
                  disabled={translationHotkeyCapture.saving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: translationHotkeyCapture.saving ? "wait" : "pointer",
                    opacity: translationHotkeyCapture.saving ? 0.7 : 1,
                  }}
                >
                  {t("common.clear")}
                </button>
              </div>
              <p className="settings-hint" style={{ margin: 0 }}>
                {t("settings.translationHint")}
              </p>
              <div className="settings-row">
                <span className="permission-label">{translationTarget ? t("settings.targetLanguage", { language: translationTarget }) : t("settings.notEnabled")}</span>
                <button
                  className="btn-ghost"
                  onClick={() => {
                    setTranslationPickerOpen(v => !v);
                    if (!translationPickerOpen) {
                      setShowCustomLangInput(false);
                      setCustomLangInput("");
                    }
                  }}
                  style={{ fontSize: 12, padding: "6px 10px" }}
                >
                  {translationPickerOpen ? t("settings.collapse") : translationTarget ? t("settings.changeTarget") : t("settings.selectLanguage")}
                </button>
              </div>
              {translationPickerOpen && (
                <div className="settings-column" style={{ gap: 8 }}>
                  <p className="settings-hint" style={{ margin: 0 }}>
                    {t("settings.translationSelectHint")}
                  </p>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                    <button
                      type="button"
                      className="picker-option"
                      data-active={!translationTarget}
                      onClick={() => void handleTranslationSelect(null)}
                      style={{ padding: "5px 12px", borderRadius: 6, fontSize: 12 }}
                    >
                      {t("settings.off")}
                    </button>
                    {["English", "日本語", "한국어", "Français", "Deutsch", "Español", "Русский", "Português"].map(lang => (
                      <button
                        key={lang}
                        type="button"
                        className="picker-option"
                        data-active={translationTarget === lang}
                        onClick={() => void handleTranslationSelect(lang)}
                        style={{ padding: "5px 12px", borderRadius: 6, fontSize: 12 }}
                      >
                        {lang}
                      </button>
                    ))}
                    {translationTarget && !["English", "日本語", "한국어", "Français", "Deutsch", "Español", "Русский", "Português"].includes(translationTarget) && (
                      <button
                        type="button"
                        className="picker-option"
                        data-active={true}
                        style={{ padding: "5px 12px", borderRadius: 6, fontSize: 12 }}
                      >
                        {translationTarget}
                      </button>
                    )}
                    <button
                      type="button"
                      className="picker-option"
                      data-active={showCustomLangInput}
                      onClick={() => setShowCustomLangInput(v => !v)}
                      style={{ padding: "5px 12px", borderRadius: 6, fontSize: 12 }}
                    >
                      {t("settings.customLang")}
                    </button>
                  </div>
                  {showCustomLangInput && (
                    <div style={{ display: "flex", gap: 6 }}>
                      <input
                        type="text"
                        className="settings-input"
                        placeholder={t("settings.customLangPlaceholder")}
                        aria-label={t("settings.customLangLabel")}
                        value={customLangInput}
                        onChange={e => setCustomLangInput(e.target.value)}
                        onKeyDown={e => {
                          if (e.key === "Enter" && customLangInput.trim()) {
                            void handleTranslationSelect(customLangInput.trim());
                          }
                        }}
                        style={{ flex: 1 }}
                        autoFocus
                      />
                      <button
                        className="test-btn"
                        disabled={!customLangInput.trim()}
                        onClick={() => {
                          if (customLangInput.trim()) {
                            void handleTranslationSelect(customLangInput.trim());
                          }
                        }}
                        style={{ padding: "7px 12px" }}
                      >
                        <Check size={14} />
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          </section>

          {/* Smart Vocabulary */}
          <section className="settings-card" data-nav-id="vocabulary" style={{ animationDelay: "225ms" }}>
            <div className="settings-section-header">
              <BookOpen size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.vocabulary")}</h2>
              {profile && (
                <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-text-tertiary)" }}>
                  {t("settings.hotWordsCount", { count: profile.hot_words.length, transcriptions: profile.total_transcriptions })}
                </span>
              )}
            </div>
            <div className="settings-column" style={{ gap: 8 }}>
              {/* Add hot word */}
              <div style={{ display: "flex", gap: 6 }}>
                <input
                  type="text"
                  placeholder={t("settings.addHotWordPlaceholder")}
                  aria-label={t("settings.addHotWordLabel")}
                  value={newHotWord}
                  onChange={(e) => setNewHotWord(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && newHotWord.trim()) {
                      handleAddHotWord();
                    }
                  }}
                  style={{
                    flex: 1, padding: "7px 10px", borderRadius: 8,
                    border: "1px solid var(--color-border)",
                    background: "var(--color-bg-secondary)",
                    color: "var(--color-text-primary)", fontSize: 13, outline: "none",
                  }}
                />
                <button
                  className="test-btn"
                  onClick={() => {
                    handleAddHotWord();
                  }}
                  style={{ padding: "7px 12px" }}
                >
                  <Plus size={14} />
                </button>
              </div>

              {/* Hot word list */}
              {profile && profile.hot_words.length > 0 && (
                <div style={{
                  display: "flex", flexWrap: "wrap", gap: 4,
                  maxHeight: 120, overflow: "auto",
                  padding: "4px 0",
                }}>
                  {[...profile.hot_words]
                    .sort((a, b) => b.weight - a.weight || b.use_count - a.use_count)
                    .map((hw) => (
                    <span
                      key={hw.text}
                      style={{
                        display: "inline-flex", alignItems: "center", gap: 4,
                        padding: "3px 8px", borderRadius: 12,
                        background: "var(--color-bg-secondary)",
                        border: `1px solid ${sourceColors[hw.source] ?? "var(--color-border)"}`,
                        fontSize: 12, color: "var(--color-text-secondary)",
                      }}
                    >
                      <span style={{
                        width: 6, height: 6, borderRadius: "50%",
                        background: sourceColors[hw.source] ?? "var(--color-border)",
                        flexShrink: 0,
                      }} />
                      {hw.text}
                      <button
                        onClick={() => {
                          removeHotWord(hw.text).then(() => refreshProfile()).catch(() => {});
                        }}
                        style={{
                          background: "none", border: "none", cursor: "pointer",
                          color: "var(--color-text-tertiary)", padding: 0,
                          display: "flex", alignItems: "center",
                        }}
                      >
                        <X size={10} />
                      </button>
                    </span>
                  ))}
                </div>
              )}

              {/* Legend + actions */}
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                {Object.entries(sourceLabels).map(([key, label]) => (
                  <span key={key} style={{ display: "flex", alignItems: "center", gap: 3, fontSize: 11, color: "var(--color-text-tertiary)" }}>
                    <span style={{ width: 6, height: 6, borderRadius: "50%", background: sourceColors[key] }} />
                    {t(label)}
                  </span>
                ))}
                <span style={{ flex: 1 }} />
              </div>
            </div>
          </section>

          {/* Profile Export/Import */}
          <section className="settings-card" data-nav-id="misc" style={{ animationDelay: "255ms" }}>
            <div className="settings-section-header">
              <Download size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.data")}</h2>
            </div>
            <div style={{ display: "flex", gap: 6 }}>
              <button
                className="btn-ghost"
                onClick={async () => {
                  try {
                    const data = await exportUserProfile();
                    const blob = new Blob([data], { type: "application/json" });
                    const url = URL.createObjectURL(blob);
                    const a = document.createElement("a");
                    a.href = url;
                    a.download = "light-whisper-profile.json";
                    a.click();
                    setTimeout(() => URL.revokeObjectURL(url), 200);
                    toast.success(t("toast.configExported"));
                  } catch { toast.error(t("toast.configExportFailed")); }
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Download size={13} style={{ marginRight: 4 }} />{t("settings.exportConfig")}
              </button>
              <button
                className="btn-ghost"
                onClick={() => {
                  const input = document.createElement("input");
                  input.type = "file";
                  input.accept = ".json";
                  input.onchange = async (e) => {
                    const file = (e.target as HTMLInputElement).files?.[0];
                    if (!file) return;
                    try {
                      const text = await file.text();
                      await importUserProfile(text);
                      refreshProfile();
                      await refreshAiPolishKey();
                      toast.success(t("toast.configImported"));
                    } catch { toast.error(t("toast.configImportFailed")); }
                  };
                  input.click();
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Upload size={13} style={{ marginRight: 4 }} />{t("settings.importConfig")}
              </button>
            </div>
          </section>

          {/* Permissions */}
          <section className="settings-card" style={{ animationDelay: "250ms" }}>
            <div className="settings-section-header">
              <Accessibility size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.permissions")}</h2>
            </div>
            <div className="permission-list">
              <div className="settings-row">
                <div className="permission-item">
                  <Accessibility size={14} className="icon-tertiary" />
                  <span className="permission-label">{t("settings.accessibilityPaste")}</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    await pasteText(t("settings.testPasteContent"), inputMethod);
                    toast.success(t("toast.pasteOk"));
                  } catch { toast.error(t("toast.pasteFailed")); }
                }}>{t("common.test")}</button>
              </div>
            </div>
          </section>

          {/* Startup */}
          <section className="settings-card" style={{ animationDelay: "300ms" }}>
            <div className="settings-section-header">
              <Power size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.startup")}</h2>
            </div>
            <div className="settings-row">
              <span className="permission-label">{t("settings.autostart")}</span>
              <button
                role="switch"
                aria-checked={autostart}
                aria-label={t("settings.autostart")}
                onClick={handleAutostartToggle}
                className="toggle-switch"
                style={{
                  background: autostart ? "var(--color-accent)" : "var(--color-bg-tertiary)",
                }}
              >
                <div className="toggle-knob" style={{ transform: autostart ? "translateX(20px)" : "translateX(0)" }} />
              </button>
            </div>
          </section>

          <section className="settings-card" style={{ animationDelay: "325ms" }}>
            <div className="settings-section-header">
              <Download size={15} className="icon-accent" />
              <h2 className="settings-section-title">{t("settings.update")}</h2>
            </div>
            <div className="settings-row" style={{ gap: 12 }}>
              <div className="permission-item" style={{ alignItems: "flex-start", flex: 1, minWidth: 0 }}>
                <Download size={14} className="icon-tertiary" />
                <div className="settings-column" style={{ gap: 4, minWidth: 0 }}>
                  <span className="permission-label">{t("settings.checkAppUpdate")}</span>
                  <p className="settings-hint">
                    {updateStatusText || t("settings.currentVersion", { version: appVersion || "..." })}
                  </p>
                  {latestAvailableVersion ? (
                    <p className="settings-hint">
                      {t("settings.newVersionAvailable", { version: latestAvailableVersion })}
                    </p>
                  ) : null}
                </div>
              </div>
              <button
                className="test-btn"
                onClick={() => { void (latestAvailableVersion ? handleOpenReleasePage() : handleCheckForUpdates()); }}
                disabled={updateChecking}
                style={{
                  flexShrink: 0,
                  minWidth: 88,
                  opacity: updateChecking ? 0.7 : 1,
                  cursor: updateChecking ? "wait" : "pointer",
                }}
              >
                {updateChecking ? t("settings.checking") : latestAvailableVersion ? t("settings.goToDownload") : t("settings.checkUpdate")}
              </button>
            </div>
            {latestAvailableVersion ? (
              <div className="settings-column" style={{ marginTop: 8, gap: 0 }}>
                <p className="settings-hint" style={{ marginLeft: 24 }}>
                  {t("settings.updateSource")}
                </p>
              </div>
            ) : null}
          </section>
        </div>
      </div>

      {/* Footer */}
      <div className="settings-footer" style={{ padding: `10px ${PADDING}px` }}>
        <p className="settings-footer-text">
          {t("settings.footer")} <span className="settings-footer-version">v{appVersion}</span>
          <span style={{ margin: "0 6px" }}>·</span>
          {t("settings.footerSubtitle")}
        </p>
      </div>
    </div>
  );
}
