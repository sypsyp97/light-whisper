import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  setAssistantHotkey,
  setAssistantScreenContextEnabled,
  setAssistantSystemPrompt,
  setAssistantApiKey,
  getAssistantApiKey,
  setLlmProviderConfig,
  listAiModels,
  setWebSearchConfig,
  setWebSearchApiKey,
  getWebSearchApiKey,
  getUserProfile,
} from "@/api/tauri";
import type {
  LlmReasoningMode,
  WebSearchProvider,
} from "@/types";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { formatHotkeyForDisplay } from "@/lib/hotkey";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import Field from "@/components/ui/Field";
import Toggle from "@/components/ui/Toggle";
import Picker from "@/components/ui/Picker";
import SecretInput from "@/components/ui/SecretInput";
import TextArea from "@/components/ui/TextArea";
import Button from "@/components/ui/Button";
import Kbd from "@/components/ui/Kbd";

function normalizeProviderId(id: string | null | undefined): string {
  return id === "custom_compat" ? "custom" : id || "cerebras";
}

export function AssistantSection() {
  const { t } = useTranslation();
  const [assistantHotkey, setAssistantHotkeyState] = useState<string | null>(null);
  const [screenContext, setScreenContext] = useState(false);
  const [useSeparate, setUseSeparate] = useState(false);
  const [provider, setProvider] = useState("cerebras");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [reasoningMode, setReasoningMode] = useState<LlmReasoningMode>("provider_default");
  const [systemPrompt, setSystemPrompt] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [webEnabled, setWebEnabled] = useState(false);
  const [webProvider, setWebProvider] = useState<WebSearchProvider>("model_native");
  const [webMaxResults, setWebMaxResults] = useState(5);
  const [tavilyKey, setTavilyKey] = useState("");

  useEffect(() => {
    void getUserProfile().then((profile) => {
      setAssistantHotkeyState(profile.assistant_hotkey ?? null);
      setScreenContext(Boolean(profile.assistant_screen_context_enabled));
      setSystemPrompt(profile.assistant_system_prompt ?? "");
      const llm = profile.llm_provider;
      setUseSeparate(Boolean(llm.assistant_use_separate_model));
      setProvider(normalizeProviderId(llm.assistant_provider || llm.active));
      setModel(llm.assistant_model ?? "");
      setReasoningMode(llm.assistant_reasoning_mode ?? "provider_default");
      const ws = profile.web_search;
      if (ws) {
        setWebEnabled(ws.enabled);
        setWebProvider(ws.provider);
        setWebMaxResults(ws.max_results);
      }
    }).catch(() => {});
    void getAssistantApiKey().then(setApiKey).catch(() => {});
    void getWebSearchApiKey().then(setTavilyKey).catch(() => {});
  }, []);

  const enabled = Boolean(assistantHotkey);

  const hotkeyCapture = useHotkeyCapture({
    save: async (shortcut) => {
      await setAssistantHotkey(shortcut);
      setAssistantHotkeyState(shortcut);
    },
    label: t("settings.assistantHotkeyLabel"),
  });

  const apiKeySave = useDebouncedCallback((v: string) => {
    void setAssistantApiKey(v).catch(() => {});
  }, 900, { onUnmount: "flush" });

  const promptSave = useDebouncedCallback((v: string) => {
    void setAssistantSystemPrompt(v || null).catch(() => toast.error(t("toast.assistantPromptSaveFailed")));
  }, 900, { onUnmount: "flush" });

  const webKeySave = useDebouncedCallback((v: string) => {
    void setWebSearchApiKey(v).catch(() => {});
  }, 900, { onUnmount: "flush" });

  const handleEnableToggle = useCallback(async (next: boolean) => {
    if (next) {
      hotkeyCapture.startCapture();
    } else {
      try {
        await setAssistantHotkey(null);
        setAssistantHotkeyState(null);
        toast.success(t("toast.hotkeyCleared", { label: t("settings.assistantHotkeyLabel") }));
      } catch {
        toast.error(t("toast.hotkeyClearFailed", { label: t("settings.assistantHotkeyLabel") }));
      }
    }
  }, [hotkeyCapture, t]);

  const clearHotkey = useCallback(async () => {
    try {
      await setAssistantHotkey(null);
      setAssistantHotkeyState(null);
    } catch { /* */ }
  }, []);

  const handleScreenContext = useCallback(async (next: boolean) => {
    const prev = screenContext;
    setScreenContext(next);
    try { await setAssistantScreenContextEnabled(next); }
    catch { setScreenContext(prev); toast.error(t("toast.assistantScreenContextFailed")); }
  }, [screenContext, t]);

  const persistLlm = useCallback(async (patch: {
    provider?: string;
    model?: string;
    reasoning?: LlmReasoningMode;
    useSeparate?: boolean;
  }) => {
    const effectiveProvider = patch.provider ?? provider;
    const effectiveModel = patch.model ?? model;
    const effectiveReasoning = patch.reasoning ?? reasoningMode;
    const effectiveUseSeparate = patch.useSeparate ?? useSeparate;
    try {
      await setLlmProviderConfig(
        effectiveProvider,
        undefined,
        undefined,
        undefined,
        effectiveReasoning,
        effectiveUseSeparate,
        effectiveModel,
        effectiveProvider,
      );
    } catch { /* */ }
  }, [provider, model, reasoningMode, useSeparate]);

  const handleUseSeparate = useCallback(async (next: boolean) => {
    setUseSeparate(next);
    await persistLlm({ useSeparate: next });
  }, [persistLlm]);

  const handleProviderChange = useCallback(async (next: string) => {
    setProvider(next);
    await persistLlm({ provider: next });
  }, [persistLlm]);

  const handleModelChange = useCallback(async (next: string) => {
    setModel(next);
    await persistLlm({ model: next });
  }, [persistLlm]);

  const handleReasoningChange = useCallback(async (next: LlmReasoningMode) => {
    setReasoningMode(next);
    await persistLlm({ reasoning: next });
  }, [persistLlm]);

  const handleApiKeyChange = useCallback((v: string) => {
    setApiKey(v);
    apiKeySave.schedule(v);
  }, [apiKeySave]);

  const handlePromptChange = useCallback((v: string) => {
    setSystemPrompt(v);
    promptSave.schedule(v);
  }, [promptSave]);

  const handleWebEnabled = useCallback(async (next: boolean) => {
    setWebEnabled(next);
    try { await setWebSearchConfig(next, webProvider, webMaxResults); } catch { setWebEnabled(!next); }
  }, [webProvider, webMaxResults]);

  const handleWebProvider = useCallback(async (next: WebSearchProvider) => {
    setWebProvider(next);
    try { await setWebSearchConfig(webEnabled, next, webMaxResults); } catch { /* */ }
  }, [webEnabled, webMaxResults]);

  const handleWebMax = useCallback(async (next: string) => {
    const n = Number(next);
    if (!Number.isFinite(n)) return;
    setWebMaxResults(n);
    try { await setWebSearchConfig(webEnabled, webProvider, n); } catch { /* */ }
  }, [webEnabled, webProvider]);

  const handleTavilyKey = useCallback((v: string) => {
    setTavilyKey(v);
    webKeySave.schedule(v);
  }, [webKeySave]);

  useEffect(() => {
    if (!apiKey || !provider) return;
    void listAiModels(provider, undefined, apiKey).then((res) => {
      setModels(res.models.map((m) => m.id));
    }).catch(() => {});
  }, [apiKey, provider]);

  const providerOptions = [
    { value: "openai", label: "OpenAI" },
    { value: "deepseek", label: "DeepSeek" },
    { value: "cerebras", label: "Cerebras" },
    { value: "siliconflow", label: "SiliconFlow" },
    { value: "custom", label: t("settings.customCompatLabel") },
  ];

  const reasoningOptions: Array<{ value: LlmReasoningMode; label: string }> = [
    { value: "provider_default", label: t("settings.reasoningDefault") },
    { value: "off", label: t("settings.reasoningOff") },
    { value: "light", label: t("settings.reasoningLight") },
    { value: "balanced", label: t("settings.reasoningBalanced") },
    { value: "deep", label: t("settings.reasoningDeep") },
  ];

  const webProviderOptions: Array<{ value: WebSearchProvider; label: string; description: string }> = [
    { value: "model_native", label: t("settings.webSearchModelNative"), description: t("settings.webSearchModelNativeDesc") },
    { value: "exa", label: t("settings.webSearchExa"), description: t("settings.webSearchExaDesc") },
    { value: "tavily", label: t("settings.webSearchTavily"), description: t("settings.webSearchTavilyDesc") },
  ];

  const maxResultsOptions = useMemo(() =>
    Array.from({ length: 10 }, (_, i) => ({ value: String(i + 1), label: String(i + 1) })),
  []);

  const modelOptions = models.map((m) => ({ value: m, label: m }));
  if (model && !modelOptions.some((o) => o.value === model)) {
    modelOptions.unshift({ value: model, label: model });
  }

  const hotkeyLabel = assistantHotkey ? formatHotkeyForDisplay(assistantHotkey) : "";

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-assistant"
      data-nav-id="assistant"
    >
      <h2 className="lw-settings-section-title">{t("settings.assistant")}</h2>
      <Field label={t("settings.assistant")} hint={t("settings.assistantHint")}>
        <Toggle
          checked={enabled}
          onChange={(v) => void handleEnableToggle(v)}
          label={t("settings.assistant")}
          data-testid="assistant-enable-toggle"
        />
      </Field>
      <Field label={t("settings.assistantHotkeyLabel")}>
        <div className="lw-inline">
          <Button
            onClick={hotkeyCapture.startCapture}
            loading={hotkeyCapture.saving}
            data-testid="assistant-hotkey-btn"
          >
            {hotkeyCapture.capturing
              ? t("settings.pressAssistantHotkey")
              : hotkeyLabel ? <Kbd combo={hotkeyLabel} /> : t("settings.noAssistantHotkey")}
          </Button>
          {assistantHotkey && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void clearHotkey()}
              data-testid="assistant-hotkey-clear"
            >
              {t("common.clear")}
            </Button>
          )}
        </div>
      </Field>
      <Field label={t("settings.useSeparateConfig")} hint={t("settings.separateConfigHint")}>
        <Toggle
          checked={!useSeparate}
          onChange={(v) => void handleUseSeparate(!v)}
          label={t("settings.useSeparateConfig")}
          data-testid="assistant-same-provider-toggle"
        />
      </Field>
      {useSeparate && (
        <>
          <Field label={t("settings.assistantProvider")}>
            <Picker
              value={provider}
              options={providerOptions}
              onChange={(v) => void handleProviderChange(v)}
              data-testid="assistant-provider-picker"
            />
          </Field>
          <Field label={t("settings.assistantModel")}>
            <Picker
              value={model}
              options={modelOptions}
              onChange={(v) => void handleModelChange(v)}
              searchable
              allowCustomValue
              customValueLabel={(value) => t("settings.useCustomModel", { model: value })}
              data-testid="assistant-model-picker"
            />
          </Field>
          <Field label={t("settings.assistantApiKey")}>
            <SecretInput
              value={apiKey}
              onChange={handleApiKeyChange}
              placeholder={t("settings.assistantApiKey")}
              data-testid="assistant-api-key"
            />
          </Field>
          <Field label={t("settings.assistantReasoningMode")}>
            <Picker
              value={reasoningMode}
              options={reasoningOptions}
              onChange={(v) => void handleReasoningChange(v as LlmReasoningMode)}
              data-testid="assistant-reasoning-picker"
            />
          </Field>
        </>
      )}
      <Field label={t("settings.assistantScreenContext")} hint={t("settings.screenContextAssistantHint")}>
        <Toggle
          checked={screenContext}
          onChange={(v) => void handleScreenContext(v)}
          label={t("settings.assistantScreenContext")}
          data-testid="assistant-screen-context-toggle"
        />
      </Field>
      <Field label={t("settings.customAssistantPrompt")} hint={t("settings.assistantPromptHint")}>
        <TextArea
          value={systemPrompt}
          onChange={handlePromptChange}
          placeholder={t("settings.assistantPromptPlaceholder")}
          data-testid="assistant-system-prompt"
        />
      </Field>

      <h3 className="lw-settings-section-title" style={{ fontSize: 14, marginTop: 16 }}>
        {t("settings.webSearch")}
      </h3>
      <Field label={t("settings.webSearch")} hint={t("settings.webSearchHint")}>
        <Toggle
          checked={webEnabled}
          onChange={(v) => void handleWebEnabled(v)}
          label={t("settings.webSearch")}
          data-testid="websearch-enable-toggle"
        />
      </Field>
      <Field label={t("settings.webSearchProvider")}>
        <Picker
          value={webProvider}
          options={webProviderOptions}
          onChange={(v) => void handleWebProvider(v as WebSearchProvider)}
          data-testid="websearch-provider-picker"
        />
      </Field>
      {webProvider !== "model_native" && (
        <Field label={t("settings.webSearchMaxResults")}>
          <Picker
            value={String(webMaxResults)}
            options={maxResultsOptions}
            onChange={(v) => void handleWebMax(v)}
            data-testid="websearch-max-results"
          />
        </Field>
      )}
      {webProvider === "tavily" && (
        <Field label={t("settings.webSearchTavilyApiKeyLabel")}>
          <SecretInput
            value={tavilyKey}
            onChange={handleTavilyKey}
            placeholder={t("settings.webSearchTavilyKeyPlaceholder")}
            data-testid="websearch-tavily-key"
          />
        </Field>
      )}
    </section>
  );
}

export default AssistantSection;
