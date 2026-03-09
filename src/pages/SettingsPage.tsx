import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { ArrowLeft, Mic, Accessibility, Sun, Moon, Monitor, Power, Keyboard, ClipboardPaste, AudioLines, Zap, Sparkles, Eye, EyeOff, BookOpen, Plus, X, Download, Upload, Check, ChevronsUpDown, Languages, Globe, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useTheme } from "@/hooks/useTheme";
import {
  disableAutostart,
  enableAutostart,
  getEngine,
  isAutostartEnabled,
  pasteText,
  setEngine,
  testMicrophone,
  setInputMethodCommand,
  setAiPolishConfig,
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
  setCustomPrompt,
  setRecordingMode,
  setOnlineAsrApiKey,
  getOnlineAsrApiKey,
  getOnlineAsrEndpoint,
  setOnlineAsrEndpoint,
  addCustomProvider,
  removeCustomProvider,
  setAssistantHotkey,
  setAssistantSystemPrompt,
} from "@/api/tauri";
import type { AiModelInfo, CustomProvider, InputDeviceInfo, UserProfile, ApiFormat } from "@/types";
import { useRecordingContext } from "@/contexts/RecordingContext";
import TitleBar from "@/components/TitleBar";
import { PADDING, INPUT_METHOD_KEY, INPUT_DEVICE_STORAGE_KEY, DEFAULT_HOTKEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import {
  HOTKEY_MODIFIER_ORDER,
  type HotkeyModifier,
  formatHotkeyForDisplay,
  keyboardEventToHotkey,
  modifierFromKeyboardEvent,
} from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

const themeOptions = [
  { mode: "light" as const, icon: Sun, label: "浅色" },
  { mode: "dark" as const, icon: Moon, label: "深色" },
  { mode: "system" as const, icon: Monitor, label: "跟随系统" },
] as const;

const engineOptions = [
  { key: "sensevoice", icon: AudioLines, label: "SenseVoice", desc: "中英日韩粤语" },
  { key: "whisper", icon: Zap, label: "Faster Whisper", desc: "99+语言，速度快" },
  { key: "glm-asr", icon: Globe, label: "GLM-ASR", desc: "智谱在线语音识别" },
] as const;

const inputOptions = [
  { key: "sendInput" as const, icon: Keyboard, label: "直接输入", desc: "不占用剪贴板" },
  { key: "clipboard" as const, icon: ClipboardPaste, label: "剪贴板粘贴", desc: "兼容中文输入法" },
];

const llmProviderOptions = [
  {
    key: "openai",
    label: "OpenAI",
    desc: "通用 Chat Completions",
    baseUrl: "https://api.openai.com",
    defaultModel: "gpt-4.1-mini",
    models: ["gpt-4.1-mini", "gpt-4o-mini", "gpt-4.1"],
  },
  {
    key: "deepseek",
    label: "DeepSeek",
    desc: "官方兼容接口",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-chat",
    models: ["deepseek-chat", "deepseek-reasoner"],
  },
  {
    key: "cerebras",
    label: "Cerebras",
    desc: "极速推理",
    baseUrl: "https://api.cerebras.ai",
    defaultModel: "gpt-oss-120b",
    models: ["gpt-oss-120b", "gpt-oss-20b"],
  },
  {
    key: "siliconflow",
    label: "SiliconFlow",
    desc: "OpenAI 兼容",
    baseUrl: "https://api.siliconflow.cn",
    defaultModel: "Qwen/Qwen3-32B",
    models: ["Qwen/Qwen3-32B", "deepseek-ai/DeepSeek-V3", "Qwen/Qwen2.5-7B-Instruct"],
  },
  {
    key: "custom",
    label: "自定义兼容",
    desc: "vLLM / OneAPI / New API",
    baseUrl: "http://127.0.0.1:8000",
    defaultModel: "gpt-4.1-mini",
    models: ["gpt-4.1-mini", "gpt-4o-mini", "deepseek-chat"],
  },
] as const;

const LLM_PROVIDER_DRAFTS_KEY = "light-whisper-llm-provider-drafts";

const sourceLabels: Record<string, string> = {
  user: "手动",
  learned: "学习",
};

const sourceColors: Record<string, string> = {
  user: "var(--color-accent)",
  learned: "#10b981",
};

const hotkeyBackendLabels: Record<string, string> = {
  none: "未注册",
  lowLevelHook: "低层键盘钩子",
};

const hotkeyEventLabels: Record<string, string> = {
  registered: "已注册",
  unregistered: "已注销",
  pressed: "收到按下",
  released: "收到松开",
  error: "错误",
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

function formatDiagnosticTime(timestampMs?: number | null): string {
  if (!timestampMs) return "未收到";
  return new Date(timestampMs).toLocaleTimeString();
}

function findLlmPreset(key: string) {
  return llmProviderOptions.find((option) => option.key === key) ?? llmProviderOptions[0];
}

function resolveLlmBaseUrl(key: string, customBaseUrl?: string | null): string {
  return customBaseUrl?.trim() || findLlmPreset(key).baseUrl;
}

function resolveLlmModel(key: string, customModel?: string | null): string {
  return customModel?.trim() || findLlmPreset(key).defaultModel;
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

export default function SettingsPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const { isDark, theme, setTheme } = useTheme();
  const { isRecording, retryModel, hotkeyDisplay, setHotkey, hotkeyError, hotkeyDiagnostic } = useRecordingContext();
  const [engine, setEngineState] = useState<string>("sensevoice");
  const [engineLoading, setEngineLoading] = useState(true);
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [capturingHotkey, setCapturingHotkey] = useState(false);
  const [hotkeySaving, setHotkeySaving] = useState(false);
  const [assistantHotkey, setAssistantHotkeyState] = useState("");
  const [capturingAssistantHotkey, setCapturingAssistantHotkey] = useState(false);
  const [assistantHotkeySaving, setAssistantHotkeySaving] = useState(false);
  const [recordingMode, setRecordingModeState] = useState<"hold" | "toggle">(() => {
    return readLocalStorage(RECORDING_MODE_KEY) === "toggle" ? "toggle" : "hold";
  });
  const [inputDevices, setInputDevices] = useState<InputDeviceInfo[]>([]);
  const [selectedInputDeviceName, setSelectedInputDeviceName] = useState<string>("");
  const [deviceListLoading, setDeviceListLoading] = useState(true);
  const [micLevel, setMicLevel] = useState(0);
  const [micMonitorReady, setMicMonitorReady] = useState(false);
  const [inputMethod, setInputMethod] = useState<"sendInput" | "clipboard">(() => {
    return readLocalStorage(INPUT_METHOD_KEY) === "clipboard"
      ? "clipboard"
      : "sendInput";
  });
  const [soundEnabled, setSoundEnabledState] = useState(() => readLocalStorage(SOUND_ENABLED_KEY) !== "false");
  const [aiPolishEnabled, setAiPolishEnabled] = useState(() => readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  const [aiPolishApiKey, setAiPolishApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [onlineAsrApiKey, setOnlineAsrApiKeyState] = useState("");
  const [showOnlineAsrKey, setShowOnlineAsrKey] = useState(false);
  const onlineAsrKeySaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [onlineAsrRegion, setOnlineAsrRegion] = useState("international");
  const [onlineAsrUrl, setOnlineAsrUrl] = useState("");
  const [aiModels, setAiModels] = useState<AiModelInfo[]>([]);
  const [aiModelSearch, setAiModelSearch] = useState("");
  const [aiModelsLoading, setAiModelsLoading] = useState(false);
  const [aiModelsError, setAiModelsError] = useState("");
  const [aiModelsSourceUrl, setAiModelsSourceUrl] = useState("");
  const [providerDrafts, setProviderDrafts] = useState<LlmProviderDraftMap>(() => readLlmProviderDrafts());
  const [providerPickerOpen, setProviderPickerOpen] = useState(false);
  const [providerSearch, setProviderSearch] = useState("");
  const [modelPickerOpen, setModelPickerOpen] = useState(false);
  const apiKeySaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const llmConfigSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const aiModelsFetchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const providerPickerRef = useRef<HTMLDivElement | null>(null);
  const providerSearchInputRef = useRef<HTMLInputElement | null>(null);
  const modelPickerRef = useRef<HTMLDivElement | null>(null);
  const modelSearchInputRef = useRef<HTMLInputElement | null>(null);

  // Agent profile state
  const [profile, setProfile] = useState<UserProfile | null>(null);
  const [newHotWord, setNewHotWord] = useState("");

  // Translation state — translation_target 是唯一真相，非空即开启
  const [translationTarget, setTranslationTargetState] = useState<string | null>(null);
  const [translationPickerOpen, setTranslationPickerOpen] = useState(false);
  const [customLangInput, setCustomLangInput] = useState("");
  const [showCustomLangInput, setShowCustomLangInput] = useState(false);
  const translationSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [customPromptState, setCustomPromptState] = useState<string>("");
  const customPromptSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [assistantPromptState, setAssistantPromptState] = useState<string>("");
  const assistantPromptSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [appVersion, setAppVersion] = useState("");
  const [llmProvider, setLlmProvider] = useState("cerebras");
  const [customBaseUrl, setCustomBaseUrl] = useState("");
  const [customModel, setCustomModel] = useState("");
  // 自定义 provider 相关
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [addingProvider, setAddingProvider] = useState(false);
  const [newProviderName, setNewProviderName] = useState("");
  const [newProviderBaseUrl, setNewProviderBaseUrl] = useState("");
  const [newProviderModel, setNewProviderModel] = useState("");
  const [newProviderFormat, setNewProviderFormat] = useState<ApiFormat>("openai_compat");

  const clearPendingApiKeySave = useCallback(() => {
    if (apiKeySaveTimer.current) {
      clearTimeout(apiKeySaveTimer.current);
      apiKeySaveTimer.current = null;
    }
  }, []);

  const clearPendingLlmConfigSave = useCallback(() => {
    if (llmConfigSaveTimer.current) {
      clearTimeout(llmConfigSaveTimer.current);
      llmConfigSaveTimer.current = null;
    }
  }, []);

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
      baseUrl: draft?.baseUrl ?? preset.baseUrl,
      model: draft?.model ?? preset.defaultModel,
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

  // 从系统密钥环加载 API Key，并同步 enabled 状态到后端
  useEffect(() => {
    void refreshAiPolishKey(readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  }, [refreshAiPolishKey]);

  useEffect(() => {
    return () => {
      if (apiKeySaveTimer.current) {
        clearTimeout(apiKeySaveTimer.current);
        apiKeySaveTimer.current = null;
      }
      if (llmConfigSaveTimer.current) {
        clearTimeout(llmConfigSaveTimer.current);
        llmConfigSaveTimer.current = null;
      }
      if (aiModelsFetchTimer.current) {
        clearTimeout(aiModelsFetchTimer.current);
        aiModelsFetchTimer.current = null;
      }
      if (translationSaveTimer.current) {
        clearTimeout(translationSaveTimer.current);
        translationSaveTimer.current = null;
      }
      if (customPromptSaveTimer.current) {
        clearTimeout(customPromptSaveTimer.current);
        customPromptSaveTimer.current = null;
      }
      if (assistantPromptSaveTimer.current) {
        clearTimeout(assistantPromptSaveTimer.current);
        assistantPromptSaveTimer.current = null;
      }
    };
  }, []);

  // 加载用户画像
  const refreshProfile = useCallback(async () => {
    try {
      const p = await getUserProfile();
      setProfile(p);
      const cps = p.llm_provider.custom_providers ?? [];
      setCustomProviders(cps);
      const nextProvider = p.llm_provider.active || "cerebras";
      // 查自定义 provider
      const cp = cps.find((c) => c.id === nextProvider);
      const nextBaseUrl = cp
        ? cp.base_url
        : resolveLlmBaseUrl(nextProvider, p.llm_provider.custom_base_url);
      const nextModel = cp
        ? cp.model
        : resolveLlmModel(nextProvider, p.llm_provider.custom_model);
      setLlmProvider(nextProvider);
      setCustomBaseUrl(nextBaseUrl);
      setCustomModel(nextModel);
      updateProviderDraft(nextProvider, nextBaseUrl, nextModel);
      setTranslationTargetState(p.translation_target ?? null);
      setCustomPromptState(p.custom_prompt ?? "");
      setAssistantHotkeyState(p.assistant_hotkey ? formatHotkeyForDisplay(p.assistant_hotkey) : "");
      setAssistantPromptState(p.assistant_system_prompt ?? "");
    } catch { /* ignore */ }
  }, [updateProviderDraft]);

  useEffect(() => {
    refreshProfile();
  }, [refreshProfile]);

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
  }, []);

  const handleEngineSwitch = async (newEngine: string) => {
    if (engineLoading || newEngine === engine) return;
    setEngineLoading(true);
    try {
      await setEngine(newEngine);
      setEngineState(newEngine);
      const label = engineOptions.find((o) => o.key === newEngine)?.label ?? newEngine;
      toast.success(`已切换为 ${label} 引擎`);
      retryModel();
    } catch {
      toast.error("切换引擎失败");
    } finally {
      setEngineLoading(false);
    }
  };

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
      toast.error("读取麦克风列表失败");
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
        if (isRecording) {
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
  }, [isRecording, selectedInputDeviceName]);

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
      toast.error("切换麦克风失败");
    } finally {
      setDeviceListLoading(false);
    }
  };

  const handleAutostartToggle = async () => {
    if (autostartLoading) return;
    const prev = autostart;
    // Optimistic update: toggle immediately, revert on failure
    setAutostart(!prev);
    setAutostartLoading(true);
    try {
      if (prev) {
        await disableAutostart();
        toast.success("已关闭开机自启动", { duration: 1100 });
      } else {
        await enableAutostart();
        toast.success("已开启开机自启动", { duration: 1100 });
      }
    } catch {
      setAutostart(prev); // revert
      toast.error("设置失败");
    } finally {
      setAutostartLoading(false);
    }
  };

  useEffect(() => {
    if (!capturingHotkey) return;

    const activeModifiers = new Set<HotkeyModifier>();
    // Track the peak set of modifiers held simultaneously (for modifier-only hotkeys)
    const peakModifiers = new Set<HotkeyModifier>();
    let mainKeyPressed = false;
    let applied = false;
    const clearModifiers = () => {
      activeModifiers.clear();
      peakModifiers.clear();
      mainKeyPressed = false;
    };

    const applyShortcut = (shortcut: string) => {
      if (applied) return;
      applied = true;
      setHotkeySaving(true);
      void setHotkey(shortcut)
        .then(() => {
          toast.success(`说话热键已设置为 ${formatHotkeyForDisplay(shortcut)}`);
        })
        .catch((err) => {
          const message = err instanceof Error ? err.message : "设置热键失败";
          toast.error(message);
        })
        .finally(() => {
          setHotkeySaving(false);
          setCapturingHotkey(false);
          clearModifiers();
        });
    };

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        setCapturingHotkey(false);
        clearModifiers();
        return;
      }

      const modifier = modifierFromKeyboardEvent(event);
      if (modifier) {
        activeModifiers.add(modifier);
        // Update peak: snapshot of all modifiers currently held
        for (const m of activeModifiers) peakModifiers.add(m);
        return;
      }

      mainKeyPressed = true;
      const shortcut = keyboardEventToHotkey(event, activeModifiers);
      if (!shortcut) return;

      applyShortcut(shortcut);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const modifier = modifierFromKeyboardEvent(event);
      if (!modifier || applied) return;

      activeModifiers.delete(modifier);

      // When all modifiers are released and no main key was pressed,
      // apply the peak modifier set as a modifier-only hotkey.
      if (activeModifiers.size === 0 && !mainKeyPressed && peakModifiers.size > 0) {
        const combo = HOTKEY_MODIFIER_ORDER
          .filter((key) => peakModifiers.has(key))
          .join("+");
        if (combo) {
          applyShortcut(combo);
        }
      }
    };

    const onVisibilityChange = () => {
      if (document.hidden) {
        clearModifiers();
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", clearModifiers);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", clearModifiers);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [capturingHotkey, setHotkey]);

  useEffect(() => {
    if (!capturingAssistantHotkey) return;

    const activeModifiers = new Set<HotkeyModifier>();
    const peakModifiers = new Set<HotkeyModifier>();
    let mainKeyPressed = false;
    let applied = false;
    const clearModifiers = () => {
      activeModifiers.clear();
      peakModifiers.clear();
      mainKeyPressed = false;
    };

    const applyShortcut = (shortcut: string) => {
      if (applied) return;
      applied = true;
      setAssistantHotkeySaving(true);
      const normalized = formatHotkeyForDisplay(shortcut);
      void setAssistantHotkey(shortcut)
        .then(() => {
          setAssistantHotkeyState(normalized);
          toast.success(`助手热键已设置为 ${normalized}`);
        })
        .catch((err) => {
          const message = err instanceof Error ? err.message : "设置助手热键失败";
          toast.error(message);
        })
        .finally(() => {
          setAssistantHotkeySaving(false);
          setCapturingAssistantHotkey(false);
          clearModifiers();
        });
    };

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      if (event.key === "Escape") {
        setCapturingAssistantHotkey(false);
        clearModifiers();
        return;
      }

      const modifier = modifierFromKeyboardEvent(event);
      if (modifier) {
        activeModifiers.add(modifier);
        for (const key of activeModifiers) peakModifiers.add(key);
        return;
      }

      mainKeyPressed = true;
      const shortcut = keyboardEventToHotkey(event, activeModifiers);
      if (!shortcut) return;
      applyShortcut(shortcut);
    };

    const onKeyUp = (event: KeyboardEvent) => {
      const modifier = modifierFromKeyboardEvent(event);
      if (!modifier || applied) return;

      activeModifiers.delete(modifier);
      if (activeModifiers.size === 0 && !mainKeyPressed && peakModifiers.size > 0) {
        const combo = HOTKEY_MODIFIER_ORDER
          .filter((key) => peakModifiers.has(key))
          .join("+");
        if (combo) applyShortcut(combo);
      }
    };

    const onVisibilityChange = () => {
      if (document.hidden) clearModifiers();
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", clearModifiers);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", clearModifiers);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [capturingAssistantHotkey]);

  const handleResetHotkey = async () => {
    if (hotkeySaving) return;
    setHotkeySaving(true);
    try {
      await setHotkey(DEFAULT_HOTKEY);
      toast.success("已恢复默认热键 F2");
    } catch (err) {
      const message = err instanceof Error ? err.message : "恢复默认热键失败";
      toast.error(message);
    } finally {
      setHotkeySaving(false);
      setCapturingHotkey(false);
    }
  };

  const handleClearAssistantHotkey = async () => {
    if (assistantHotkeySaving) return;
    setAssistantHotkeySaving(true);
    try {
      await setAssistantHotkey(null);
      setAssistantHotkeyState("");
      toast.success("已清除助手热键");
    } catch (err) {
      const message = err instanceof Error ? err.message : "清除助手热键失败";
      toast.error(message);
    } finally {
      setAssistantHotkeySaving(false);
      setCapturingAssistantHotkey(false);
    }
  };

  const scheduleCustomLlmConfigSave = useCallback((provider: string, baseUrl: string, model: string) => {
    clearPendingLlmConfigSave();
    llmConfigSaveTimer.current = setTimeout(() => {
      // 后端 set_llm_provider_config 已自动同步 custom_providers
      setLlmProviderConfig(provider, baseUrl || undefined, model || undefined).catch(() => {});
    }, 400);
  }, [clearPendingLlmConfigSave]);

  const refreshAiModels = useCallback(async (silent = false) => {
    const apiKey = aiPolishApiKey.trim();
    const baseUrl = customBaseUrl.trim();
    if (!apiKey) {
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("请先填写 API Key，再拉取模型列表。");
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
      setAiModelsError(payload.models.length === 0 ? "模型列表为空。" : "");
    } catch (err) {
      const message = err instanceof Error ? err.message : "拉取模型列表失败";
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError(message);
    } finally {
      setAiModelsLoading(false);
    }
  }, [aiPolishApiKey, customBaseUrl, llmProvider]);

  useEffect(() => {
    if (aiModelsFetchTimer.current) {
      clearTimeout(aiModelsFetchTimer.current);
    }

    if (!aiPolishApiKey.trim()) {
      setAiModels([]);
      setAiModelsSourceUrl("");
      setAiModelsError("");
      setAiModelsLoading(false);
      return;
    }

    aiModelsFetchTimer.current = setTimeout(() => {
      void refreshAiModels(true);
    }, 700);

    return () => {
      if (aiModelsFetchTimer.current) {
        clearTimeout(aiModelsFetchTimer.current);
        aiModelsFetchTimer.current = null;
      }
    };
  }, [aiPolishApiKey, customBaseUrl, llmProvider, refreshAiModels]);

  const handleAddHotWord = useCallback(() => {
    const word = newHotWord.trim();
    if (!word) return;

    addHotWord(word, 3).then(() => {
      setNewHotWord("");
      refreshProfile();
      toast.success(`已添加热词: ${word}`);
    }).catch(() => toast.error("添加失败"));
  }, [newHotWord, refreshProfile]);

  const hotkeyStatusError = hotkeyError || hotkeyDiagnostic?.lastError || null;
  const hotkeyBackendLabel = hotkeyDiagnostic
    ? (hotkeyBackendLabels[hotkeyDiagnostic.backend] ?? hotkeyDiagnostic.backend)
    : "加载中";
  const hotkeyEventLabel = hotkeyDiagnostic?.lastEvent
    ? (hotkeyEventLabels[hotkeyDiagnostic.lastEvent] ?? hotkeyDiagnostic.lastEvent)
    : "暂无";
  const selectedDeviceMissing = Boolean(selectedInputDeviceName)
    && !inputDevices.some((device) => device.name === selectedInputDeviceName);
  const currentLlmPreset = useMemo(() => {
    const cp = customProviders.find((p) => p.id === llmProvider);
    if (cp) return { key: cp.id, label: cp.name, desc: cp.api_format === "anthropic" ? "Anthropic" : "OpenAI 兼容", baseUrl: cp.base_url, defaultModel: cp.model, models: [] as string[] };
    return findLlmPreset(llmProvider);
  }, [llmProvider, customProviders]);
  const allProviderOptions = useMemo(() => {
    const presets = llmProviderOptions.map(({ key, label, desc, baseUrl }) => ({ key, label, desc, baseUrl, isCustom: false as const }));
    const customs = customProviders.map((cp) => ({
      key: cp.id,
      label: cp.name,
      desc: cp.api_format === "anthropic" ? "Anthropic" : "OpenAI 兼容",
      baseUrl: cp.base_url,
      isCustom: true as const,
    }));
    return [...presets, ...customs];
  }, [customProviders]);
  const filteredProviderOptions = useMemo(() => allProviderOptions.filter(({ label, desc, baseUrl }) => {
    const keyword = providerSearch.trim().toLowerCase();
    if (!keyword) return true;
    return label.toLowerCase().includes(keyword)
      || desc.toLowerCase().includes(keyword)
      || baseUrl.toLowerCase().includes(keyword);
  }), [allProviderOptions, providerSearch]);
  const filteredAiModels = useMemo(() => aiModels.filter((model) => {
    const keyword = aiModelSearch.trim().toLowerCase();
    if (!keyword) return true;
    return model.id.toLowerCase().includes(keyword) || (model.ownedBy ?? "").toLowerCase().includes(keyword);
  }), [aiModels, aiModelSearch]);
  const selectedAiModel = aiModels.find((model) => model.id === customModel);

  const handleProviderSelect = useCallback(async (nextProvider: string) => {
    if (nextProvider === llmProvider) {
      setProviderPickerOpen(false);
      setProviderSearch("");
      return;
    }

    updateProviderDraft(llmProvider, customBaseUrl, customModel);
    clearPendingApiKeySave();
    clearPendingLlmConfigSave();
    await setAiPolishConfig(aiPolishEnabled, aiPolishApiKey).catch(() => {});

    const nextDraft = resolveProviderDraft(nextProvider);
    setLlmProvider(nextProvider);
    setCustomBaseUrl(nextDraft.baseUrl);
    setCustomModel(nextDraft.model);
    updateProviderDraft(nextProvider, nextDraft.baseUrl, nextDraft.model);
    setProviderPickerOpen(false);
    setModelPickerOpen(false);
    setProviderSearch("");
    setAiModelSearch("");
    await setLlmProviderConfig(nextProvider, nextDraft.baseUrl || undefined, nextDraft.model || undefined).catch(() => {});
    await refreshAiPolishKey();
  }, [
    aiPolishApiKey,
    aiPolishEnabled,
    clearPendingApiKeySave,
    clearPendingLlmConfigSave,
    customBaseUrl,
    customModel,
    llmProvider,
    refreshAiPolishKey,
    resolveProviderDraft,
    updateProviderDraft,
  ]);

  const handleModelSelect = useCallback((nextModel: string) => {
    const normalizedModel = nextModel.trim();
    if (!normalizedModel) return;
    setCustomModel(normalizedModel);
    updateProviderDraft(llmProvider, customBaseUrl, normalizedModel);
    scheduleCustomLlmConfigSave(llmProvider, customBaseUrl, normalizedModel);
    setModelPickerOpen(false);
    setAiModelSearch("");
  }, [customBaseUrl, llmProvider, scheduleCustomLlmConfigSave, updateProviderDraft]);

  const handleTranslationSelect = useCallback(async (target: string | null) => {
    setTranslationTargetState(target);
    setTranslationPickerOpen(false);
    setShowCustomLangInput(false);
    setCustomLangInput("");
    if (translationSaveTimer.current) clearTimeout(translationSaveTimer.current);
    try {
      const autoEnabled = await setTranslationTarget(target);
      if (autoEnabled) {
        setAiPolishEnabled(true);
        writeLocalStorage(AI_POLISH_ENABLED_KEY, "true");
        toast.success("已自动开启 AI 润色");
      }
    } catch {
      toast.error("保存翻译设置失败");
    }
  }, []);

  const handleCustomPromptChange = useCallback((value: string) => {
    setCustomPromptState(value);
    if (customPromptSaveTimer.current) clearTimeout(customPromptSaveTimer.current);
    customPromptSaveTimer.current = setTimeout(() => {
      setCustomPrompt(value.trim() || null).catch(() => {
        toast.error("保存自定义指令失败");
      });
    }, 800);
  }, []);

  const handleAssistantPromptChange = useCallback((value: string) => {
    setAssistantPromptState(value);
    if (assistantPromptSaveTimer.current) clearTimeout(assistantPromptSaveTimer.current);
    assistantPromptSaveTimer.current = setTimeout(() => {
      setAssistantSystemPrompt(value.trim() || null).catch(() => {
        toast.error("保存助手提示词失败");
      });
    }, 800);
  }, []);

  useEffect(() => {
    if (providerPickerOpen) {
      providerSearchInputRef.current?.focus();
      providerSearchInputRef.current?.select();
    }
  }, [providerPickerOpen]);

  useEffect(() => {
    if (!providerPickerOpen && providerSearch) {
      setProviderSearch("");
    }
  }, [providerPickerOpen, providerSearch]);

  useEffect(() => {
    if (modelPickerOpen) {
      modelSearchInputRef.current?.focus();
      modelSearchInputRef.current?.select();
    }
  }, [modelPickerOpen]);

  useEffect(() => {
    if (!modelPickerOpen && aiModelSearch) {
      setAiModelSearch("");
    }
  }, [aiModelSearch, modelPickerOpen]);

  useEffect(() => {
    if (!providerPickerOpen && !modelPickerOpen) {
      return;
    }

    const onPointerDown = (event: MouseEvent) => {
      const target = event.target as Node;
      if (providerPickerOpen && providerPickerRef.current && !providerPickerRef.current.contains(target)) {
        setProviderPickerOpen(false);
      }
      if (modelPickerOpen && modelPickerRef.current && !modelPickerRef.current.contains(target)) {
        setModelPickerOpen(false);
      }
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setProviderPickerOpen(false);
        setModelPickerOpen(false);
      }
    };

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [modelPickerOpen, providerPickerOpen]);

  return (
    <div className="page-root">

      <TitleBar
        title="设置"
        leftAction={
          <button aria-label="返回" className="icon-btn plain" onClick={() => onNavigate("main")}>
            <ArrowLeft size={14} strokeWidth={1.5} />
          </button>
        }
      />

      {/* Content */}
      <div className="settings-content" style={{ padding: `16px ${PADDING}px 16px` }}>
        <div className="settings-sections">

          {/* Appearance */}
          <section className="settings-card" style={{ animationDelay: "0ms" }}>
            <div className="settings-section-header">
              {isDark ? <Moon size={15} className="icon-accent" /> : <Sun size={15} className="icon-accent" />}
              <h2 className="settings-section-title">外观</h2>
            </div>
            <div className="settings-grid-3">
              {themeOptions.map(({ mode, icon: Icon, label }) => (
                <button
                  key={mode}
                  className="theme-btn settings-option-btn theme-option"
                  aria-label={`切换为${label}模式`}
                  aria-pressed={theme === mode}
                  onClick={() => setTheme(mode)}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                </button>
              ))}
            </div>
          </section>

          {/* Engine */}
          <section className="settings-card" style={{ animationDelay: "50ms" }}>
            <div className="settings-section-header">
              <AudioLines size={15} className="icon-accent" />
              <h2 className="settings-section-title">识别引擎</h2>
            </div>
            <div className="settings-grid-3">
              {engineOptions.map(({ key, icon: Icon, label, desc }) => (
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
                  <span className="settings-option-desc">{desc}</span>
                </button>
              ))}
            </div>
            {engine === "glm-asr" && (
              <div className="settings-column" style={{ gap: 8, marginTop: 8 }}>
                <div className="settings-column" style={{ gap: 4 }}>
                  <span className="settings-option-desc">API 端点</span>
                  <div className="settings-row" style={{ gap: 6 }}>
                    {([
                      { region: "international", label: "国际站" },
                      { region: "domestic", label: "国内站" },
                    ] as const).map(({ region, label }) => (
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
                        {label}
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
                  <span className="settings-option-desc">API Key</span>
                  <div className="settings-row" style={{ position: "relative" }}>
                    <input
                      type={showOnlineAsrKey ? "text" : "password"}
                      className="settings-input"
                      placeholder="输入智谱 GLM-ASR API Key"
                      value={onlineAsrApiKey}
                      onChange={(e) => {
                        const val = e.target.value;
                        setOnlineAsrApiKeyState(val);
                        if (onlineAsrKeySaveTimer.current) clearTimeout(onlineAsrKeySaveTimer.current);
                        onlineAsrKeySaveTimer.current = setTimeout(() => {
                          setOnlineAsrApiKey(val).catch(() => {});
                        }, 600);
                      }}
                      style={{ flex: 1, padding: "8px 36px 8px 10px" }}
                    />
                    <button
                      className="icon-btn plain"
                      onClick={() => setShowOnlineAsrKey(!showOnlineAsrKey)}
                      style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)" }}
                      aria-label={showOnlineAsrKey ? "隐藏 API Key" : "显示 API Key"}
                    >
                      {showOnlineAsrKey ? <EyeOff size={14} /> : <Eye size={14} />}
                    </button>
                  </div>
                </div>
              </div>
            )}
          </section>

          {/* Hotkey */}
          <section className="settings-card" style={{ animationDelay: "100ms" }}>
            <div className="settings-section-header">
              <Keyboard size={15} className="icon-accent" />
              <h2 className="settings-section-title">说话热键</h2>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => setCapturingHotkey(true)}
                  disabled={hotkeySaving}
                  data-capturing={capturingHotkey}
                  style={{
                    cursor: hotkeySaving ? "wait" : "pointer",
                    opacity: hotkeySaving ? 0.7 : 1,
                  }}
                >
                  {capturingHotkey ? "请按下组合键..." : hotkeyDisplay}
                </button>
                <button
                  className="btn-ghost"
                  onClick={handleResetHotkey}
                  disabled={hotkeySaving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: hotkeySaving ? "wait" : "pointer",
                    opacity: hotkeySaving ? 0.7 : 1,
                  }}
                >
                  恢复 F2
                </button>
              </div>
              <p className="settings-hint">
                点击上方按钮后按下新热键，支持任意组合键（如 Ctrl+Win、独立 Alt、F2）。按 Esc 取消设置。
              </p>
              <div className="settings-row" style={{ alignItems: "center", gap: 10, marginTop: 8 }}>
                <span className="settings-option-desc" style={{ whiteSpace: "nowrap" }}>录音模式</span>
                <select
                  className="settings-input"
                  style={{ width: "auto", minWidth: 140 }}
                  value={recordingMode}
                  onChange={(e) => {
                    const mode = e.target.value as "hold" | "toggle";
                    setRecordingModeState(mode);
                    writeLocalStorage(RECORDING_MODE_KEY, mode);
                    setRecordingMode(mode === "toggle").catch(() => {});
                  }}
                >
                  <option value="hold">按住说话</option>
                  <option value="toggle">按一下开始 / 再按一下结束</option>
                </select>
              </div>
              <div className="diagnostic-grid">
                <div className="diagnostic-item">
                  <span className="settings-option-desc">注册状态</span>
                  <strong>{hotkeyDiagnostic?.registered ? "已注册" : "未注册"}</strong>
                </div>
                <div className="diagnostic-item">
                  <span className="settings-option-desc">监听后端</span>
                  <strong>{hotkeyBackendLabel}</strong>
                </div>
                <div className="diagnostic-item">
                  <span className="settings-option-desc">最近事件</span>
                  <strong>{hotkeyEventLabel}</strong>
                </div>
                <div className="diagnostic-item">
                  <span className="settings-option-desc">按住状态</span>
                  <strong>{hotkeyDiagnostic?.isPressed ? "按下中" : "未按下"}</strong>
                </div>
                <div className="diagnostic-item">
                  <span className="settings-option-desc">最近按下</span>
                  <strong>{formatDiagnosticTime(hotkeyDiagnostic?.lastPressedAtMs)}</strong>
                </div>
                <div className="diagnostic-item">
                  <span className="settings-option-desc">最近松开</span>
                  <strong>{formatDiagnosticTime(hotkeyDiagnostic?.lastReleasedAtMs)}</strong>
                </div>
              </div>
              {hotkeyDiagnostic?.warning && <p className="settings-hint">{hotkeyDiagnostic.warning}</p>}
              {hotkeyStatusError && <p className="settings-error">{hotkeyStatusError}</p>}
            </div>
          </section>

          <section className="settings-card" style={{ animationDelay: "112ms" }}>
            <div className="settings-section-header">
              <Sparkles size={15} className="icon-accent" />
              <h2 className="settings-section-title">语音助手</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <button
                  className="theme-btn hotkey-capture-btn"
                  onClick={() => setCapturingAssistantHotkey(true)}
                  disabled={assistantHotkeySaving}
                  data-capturing={capturingAssistantHotkey}
                  style={{
                    cursor: assistantHotkeySaving ? "wait" : "pointer",
                    opacity: assistantHotkeySaving ? 0.7 : 1,
                  }}
                >
                  {capturingAssistantHotkey
                    ? "请按下助手热键..."
                    : assistantHotkey || "未设置助手热键"}
                </button>
                <button
                  className="btn-ghost"
                  onClick={handleClearAssistantHotkey}
                  disabled={assistantHotkeySaving}
                  style={{
                    fontSize: 12,
                    padding: "8px 10px",
                    cursor: assistantHotkeySaving ? "wait" : "pointer",
                    opacity: assistantHotkeySaving ? 0.7 : 1,
                  }}
                >
                  清除
                </button>
              </div>
              <p className="settings-hint" style={{ margin: 0 }}>
                助手模式会把你的语音当成任务指令，直接生成邮件、消息、翻译或回答并粘贴到当前窗口。
              </p>
              <div className="settings-column" style={{ gap: 6 }}>
                <span className="settings-option-desc">自定义助手提示词</span>
                <textarea
                  className="settings-input"
                  placeholder="例如：默认用简洁口吻；写邮件时偏正式；回复 IM 时保持自然口语"
                  value={assistantPromptState}
                  onChange={(e) => handleAssistantPromptChange(e.target.value)}
                  rows={4}
                  style={{ resize: "vertical", minHeight: 84, fontFamily: "inherit" }}
                />
                <p className="settings-hint" style={{ margin: 0 }}>
                  这段提示词只作用于助手模式，不影响普通听写与润色。
                </p>
              </div>
            </div>
          </section>

          {/* Microphone */}
          <section className="settings-card" style={{ animationDelay: "125ms" }}>
            <div className="settings-section-header">
              <Mic size={15} className="icon-accent" />
              <h2 className="settings-section-title">麦克风</h2>
            </div>
            <div className="settings-column">
              <div className="settings-row" style={{ alignItems: "center", gap: 10 }}>
                <select
                  className="settings-input microphone-select"
                  value={selectedInputDeviceName}
                  disabled={deviceListLoading}
                  onChange={(event) => {
                    void handleInputDeviceChange(event.target.value);
                  }}
                >
                  <option value="">跟随系统默认麦克风</option>
                  {inputDevices.map((device) => (
                    <option key={device.name} value={device.name}>
                      {device.name}{device.isDefault ? "（系统默认）" : ""}
                    </option>
                  ))}
                </select>
                <button
                  className="btn-ghost"
                  disabled={deviceListLoading}
                  onClick={() => { void refreshInputDevices(); }}
                  style={{ fontSize: 12, padding: "8px 10px", opacity: deviceListLoading ? 0.7 : 1 }}
                >
                  刷新
                </button>
                <button className="test-btn" onClick={async () => {
                  try {
                    setMicMonitorReady(false);
                    setMicLevel(0);
                    await stopMicrophoneLevelMonitor().catch(() => undefined);
                    const msg = await testMicrophone();
                    toast.success(msg);
                    if (!isRecording) {
                      await startMicrophoneLevelMonitor();
                      setMicMonitorReady(true);
                    }
                  } catch {
                    toast.error("麦克风测试失败");
                  }
                }}>测试</button>
              </div>
              <div className="mic-level-shell" aria-label="麦克风电平预览">
                <div className="mic-level-fill" style={{ width: `${Math.round(micLevel * 100)}%` }} />
              </div>
              <div className="settings-row" style={{ gap: 10 }}>
                <span className="settings-hint">
                  {isRecording
                    ? "录音中已暂停电平预览，避免和正式录音抢占设备。"
                    : micMonitorReady
                      ? "电平预览已开启，对着麦克风说话即可看到变化。"
                      : "电平预览未启动，通常是设备忙或系统暂时拒绝访问。"}
                </span>
                <span className="settings-option-desc">{Math.round(micLevel * 100)}%</span>
              </div>
              {selectedDeviceMissing && (
                <p className="settings-error">
                  已保存的麦克风当前不可用，录音时会回退到系统默认设备。
                </p>
              )}
            </div>
          </section>

          {/* Input Method */}
          <section className="settings-card" style={{ animationDelay: "150ms" }}>
            <div className="settings-section-header">
              <ClipboardPaste size={15} className="icon-accent" />
              <h2 className="settings-section-title">输入</h2>
            </div>
            <div className="settings-grid-2">
              {inputOptions.map(({ key, icon: Icon, label, desc }) => (
                <button
                  key={key}
                  className="theme-btn settings-option-btn"
                  aria-label={label}
                  aria-pressed={inputMethod === key}
                  onClick={() => {
                    setInputMethod(key);
                    writeLocalStorage(INPUT_METHOD_KEY, key);
                    setInputMethodCommand(key).catch(() => {});
                  }}
                >
                  <Icon size={20} strokeWidth={1.5} />
                  <span className="settings-option-label">{label}</span>
                  <span className="settings-option-desc">{desc}</span>
                </button>
              ))}
            </div>
            <div className="settings-row" style={{ marginTop: 6 }}>
              <span className="permission-label">录音提示音</span>
              <button
                role="switch"
                aria-checked={soundEnabled}
                aria-label="录音提示音"
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
            style={{
              animationDelay: "200ms",
              position: "relative",
              zIndex: providerPickerOpen || modelPickerOpen ? 8 : 1,
            }}
          >
            <div className="settings-section-header">
              <Sparkles size={15} className="icon-accent" />
              <h2 className="settings-section-title">AI 纠错</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row">
                <span className="permission-label">启用 AI 文本润色</span>
                <button
                  role="switch"
                  aria-checked={aiPolishEnabled}
                  aria-label="启用 AI 文本润色"
                  onClick={() => {
                    const next = !aiPolishEnabled;
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

              <div className="settings-column" style={{ gap: 10 }}>
                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">服务商</span>
                  <div className="picker-shell" ref={providerPickerRef}>
                    <button
                      type="button"
                      className="picker-trigger"
                      data-open={providerPickerOpen}
                      onClick={() => {
                        setProviderPickerOpen((open) => !open);
                        setModelPickerOpen(false);
                      }}
                    >
                      <span className="picker-trigger-copy">
                        <strong>{currentLlmPreset.label}</strong>
                        <span>{customBaseUrl || currentLlmPreset.baseUrl}</span>
                      </span>
                      <ChevronsUpDown size={14} className="icon-tertiary" />
                    </button>
                    {providerPickerOpen && (
                      <div className="picker-popover">
                        <input
                          ref={providerSearchInputRef}
                          type="text"
                          className="settings-input picker-search-input"
                          placeholder="搜索服务商、描述或地址"
                          value={providerSearch}
                          onChange={(e) => setProviderSearch(e.target.value)}
                        />
                        <div className="picker-list">
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
                                    title="删除此服务商"
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
                            <div className="picker-empty">没有匹配的服务商。</div>
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
                                <strong><Plus size={12} style={{ verticalAlign: -1, marginRight: 4 }} />添加自定义服务商</strong>
                              </span>
                            </button>
                          ) : (
                            <div
                              style={{ padding: "8px 10px", display: "flex", flexDirection: "column", gap: 6, borderTop: "1px solid var(--color-border)" }}
                              onClick={(e) => e.stopPropagation()}
                            >
                              <input className="settings-input" placeholder="名称" value={newProviderName} onChange={(e) => setNewProviderName(e.target.value)} style={{ fontSize: 12 }} />
                              <input className="settings-input" placeholder="Base URL" value={newProviderBaseUrl} onChange={(e) => setNewProviderBaseUrl(e.target.value)} style={{ fontSize: 12 }} />
                              <input className="settings-input" placeholder="默认模型" value={newProviderModel} onChange={(e) => setNewProviderModel(e.target.value)} style={{ fontSize: 12 }} />
                              <select
                                className="settings-input"
                                value={newProviderFormat}
                                onChange={(e) => setNewProviderFormat(e.target.value as ApiFormat)}
                                style={{ fontSize: 12 }}
                              >
                                <option value="openai_compat">OpenAI 兼容</option>
                                <option value="anthropic">Anthropic</option>
                              </select>
                              <div style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                                <button className="btn-ghost" style={{ fontSize: 11, padding: "4px 8px" }} onClick={() => { setAddingProvider(false); setNewProviderName(""); setNewProviderBaseUrl(""); setNewProviderModel(""); setNewProviderFormat("openai_compat"); }}>取消</button>
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
                                      updateProviderDraft(id, baseUrl, model);
                                      await setLlmProviderConfig(id, baseUrl || undefined, model || undefined).catch(() => {});
                                      await refreshAiPolishKey();
                                    });
                                  }}
                                >确定</button>
                              </div>
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">接口地址</span>
                  <input
                    type="text"
                    className="settings-input"
                    placeholder="Base URL 或完整接口地址"
                    value={customBaseUrl}
                    onChange={(e) => {
                      const nextBaseUrl = e.target.value;
                      setCustomBaseUrl(nextBaseUrl);
                      updateProviderDraft(llmProvider, nextBaseUrl, customModel);
                      scheduleCustomLlmConfigSave(llmProvider, nextBaseUrl, customModel);
                    }}
                  />
                  <p className="settings-hint">
                    参考 Cherry Studio：可以直接填根地址，例如 `https://api.openai.com`；如果你填完整接口地址，末尾加 `#` 可阻止自动补全路由。
                  </p>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <span className="settings-option-desc">API Key</span>
                  <div className="settings-row" style={{ position: "relative" }}>
                    <input
                      type={showApiKey ? "text" : "password"}
                      className="settings-input"
                      placeholder={`${currentLlmPreset.label} API Key`}
                      value={aiPolishApiKey}
                      onChange={(e) => {
                        const val = e.target.value;
                        setAiPolishApiKey(val);
                        clearPendingApiKeySave();
                        apiKeySaveTimer.current = setTimeout(() => {
                          setAiPolishConfig(aiPolishEnabled, val).catch(() => {});
                        }, 600);
                      }}
                      style={{ flex: 1, padding: "8px 36px 8px 10px" }}
                    />
                    <button
                      className="icon-btn plain"
                      onClick={() => setShowApiKey(!showApiKey)}
                      style={{ position: "absolute", right: 4, top: "50%", transform: "translateY(-50%)" }}
                      aria-label={showApiKey ? "隐藏 API Key" : "显示 API Key"}
                    >
                      {showApiKey ? <EyeOff size={14} /> : <Eye size={14} />}
                    </button>
                  </div>
                </div>

                <div className="settings-column" style={{ gap: 6 }}>
                  <div className="settings-row">
                    <span className="settings-option-desc">模型</span>
                    <span className="settings-option-desc">{filteredAiModels.length}/{aiModels.length}</span>
                  </div>
                  <div className="picker-shell" ref={modelPickerRef}>
                    <div className="picker-inline-row">
                      <input
                        type="text"
                        className="settings-input"
                        placeholder="模型名，可直接手动输入"
                        value={customModel}
                        onChange={(e) => {
                          const nextModel = e.target.value;
                          setCustomModel(nextModel);
                          updateProviderDraft(llmProvider, customBaseUrl, nextModel);
                          scheduleCustomLlmConfigSave(llmProvider, customBaseUrl, nextModel);
                        }}
                      />
                      <button
                        type="button"
                        className="picker-inline-button"
                        data-open={modelPickerOpen}
                        onClick={() => {
                          setModelPickerOpen((open) => !open);
                          setProviderPickerOpen(false);
                        }}
                        aria-label="打开模型列表"
                        title="打开模型列表"
                      >
                        <ChevronsUpDown size={14} className="icon-tertiary" />
                      </button>
                    </div>
                    <p className="settings-hint" style={{ margin: 0 }}>
                      {selectedAiModel?.ownedBy || (aiModels.length > 0 ? `${aiModels.length} 个可选模型，列表仅作参考，也可以直接手输完整模型名。` : "模型列表仅作参考，也可以直接手输完整模型名。")}
                    </p>
                    {modelPickerOpen && (
                      <div className="picker-popover">
                        <div className="picker-toolbar">
                          <input
                            ref={modelSearchInputRef}
                            type="text"
                            className="settings-input picker-search-input"
                            placeholder="搜索模型，回车可直接使用当前输入"
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
                            {aiModelsLoading ? "拉取中..." : "刷新"}
                          </button>
                        </div>
                        <p className="settings-hint" style={{ margin: 0 }}>
                          {aiModelsSourceUrl ? `来源：${aiModelsSourceUrl}` : "填写 API Key 后会自动拉取，也可以手动刷新。"}
                        </p>
                        {aiModelSearch.trim() ? (
                          <button
                            type="button"
                            className="picker-option picker-option-action"
                            onClick={() => handleModelSelect(aiModelSearch)}
                          >
                            <span className="picker-option-copy">
                              <strong>使用 {aiModelSearch.trim()}</strong>
                              <span>作为当前模型名</span>
                            </span>
                          </button>
                        ) : null}
                        <div className="picker-list">
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
                                ? "正在从官方接口拉取模型列表..."
                                : aiModelsError || "暂无模型列表，先填写接口地址和 API Key。"}
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              </div>

              <div className="settings-column" style={{ gap: 6 }}>
                <span className="settings-option-desc">自定义指令</span>
                <textarea
                  className="settings-input"
                  placeholder="例如：我是程序员，保留所有英文技术术语不翻译；遇到「光语」一律改为「轻语」"
                  value={customPromptState}
                  onChange={(e) => handleCustomPromptChange(e.target.value)}
                  rows={3}
                  style={{ resize: "vertical", minHeight: 60, fontFamily: "inherit" }}
                />
                <p className="settings-hint" style={{ margin: 0 }}>
                  自定义的校正规则，优先级高于内置规则。留空则不启用。
                </p>
              </div>

              <p className="settings-hint">
                AI 纠错会自动学习你的用词习惯，并将常用词汇注入热词列表提升识别准确率。
              </p>
            </div>
          </section>

          {/* Translation */}
          <section className="settings-card" style={{ animationDelay: "212ms" }}>
            <div className="settings-section-header">
              <Languages size={15} className="icon-accent" />
              <h2 className="settings-section-title">翻译</h2>
            </div>
            <div className="settings-column" style={{ gap: 10 }}>
              <div className="settings-row">
                <span className="permission-label">{translationTarget ? `目标语言：${translationTarget}` : "未开启"}</span>
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
                  {translationPickerOpen ? "收起" : translationTarget ? "更改" : "选择语言"}
                </button>
              </div>
              {translationPickerOpen && (
                <div className="settings-column" style={{ gap: 8 }}>
                  <p className="settings-hint" style={{ margin: 0 }}>
                    语音输入的文本会在 AI 润色时一并翻译。技术术语和专有名词保留原文。
                  </p>
                  <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
                    <button
                      type="button"
                      className="picker-option"
                      data-active={!translationTarget}
                      onClick={() => void handleTranslationSelect(null)}
                      style={{ padding: "5px 12px", borderRadius: 6, fontSize: 12 }}
                    >
                      关闭
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
                      自定义…
                    </button>
                  </div>
                  {showCustomLangInput && (
                    <div style={{ display: "flex", gap: 6 }}>
                      <input
                        type="text"
                        className="settings-input"
                        placeholder="输入语言名称，如 Italiano、العربية"
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
          <section className="settings-card" style={{ animationDelay: "225ms" }}>
            <div className="settings-section-header">
              <BookOpen size={15} className="icon-accent" />
              <h2 className="settings-section-title">智能词库</h2>
              {profile && (
                <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--color-text-tertiary)" }}>
                  {profile.hot_words.length} 个热词 · {profile.total_transcriptions} 次转录
                </span>
              )}
            </div>
            <div className="settings-column" style={{ gap: 8 }}>
              {/* Add hot word */}
              <div style={{ display: "flex", gap: 6 }}>
                <input
                  type="text"
                  placeholder="添加热词 (如 Claude Code)"
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
                    {label}
                  </span>
                ))}
                <span style={{ flex: 1 }} />
              </div>
            </div>
          </section>

          {/* Profile Export/Import */}
          <section className="settings-card" style={{ animationDelay: "255ms" }}>
            <div className="settings-section-header">
              <Download size={15} className="icon-accent" />
              <h2 className="settings-section-title">数据</h2>
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
                    toast.success("画像已导出");
                  } catch { toast.error("导出失败"); }
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Download size={13} style={{ marginRight: 4 }} />导出画像
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
                      toast.success("画像已导入");
                    } catch { toast.error("导入失败，请检查文件格式"); }
                  };
                  input.click();
                }}
                style={{ flex: 1, fontSize: 12, padding: "8px" }}
              >
                <Upload size={13} style={{ marginRight: 4 }} />导入画像
              </button>
            </div>
          </section>

          {/* Permissions */}
          <section className="settings-card" style={{ animationDelay: "250ms" }}>
            <div className="settings-section-header">
              <Accessibility size={15} className="icon-accent" />
              <h2 className="settings-section-title">权限</h2>
            </div>
            <div className="permission-list">
              <div className="settings-row">
                <div className="permission-item">
                  <Accessibility size={14} className="icon-tertiary" />
                  <span className="permission-label">辅助功能 / 粘贴</span>
                </div>
                <button className="test-btn" onClick={async () => {
                  try {
                    await pasteText("测试粘贴", inputMethod);
                    toast.success("粘贴功能正常");
                  } catch { toast.error("粘贴功能异常"); }
                }}>测试</button>
              </div>
            </div>
          </section>

          {/* Startup */}
          <section className="settings-card" style={{ animationDelay: "300ms" }}>
            <div className="settings-section-header">
              <Power size={15} className="icon-accent" />
              <h2 className="settings-section-title">启动</h2>
            </div>
            <div className="settings-row">
              <span className="permission-label">开机自启动</span>
              <button
                role="switch"
                aria-checked={autostart}
                aria-label="开机自启动"
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
        </div>
      </div>

      {/* Footer */}
      <div className="settings-footer" style={{ padding: `10px ${PADDING}px` }}>
        <p className="settings-footer-text">
          轻语 Whisper <span className="settings-footer-version">v{appVersion}</span>
          <span style={{ margin: "0 6px" }}>·</span>
          本地语音转文字
        </p>
      </div>
    </div>
  );
}
