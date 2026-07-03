import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Plus } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  setAiPolishConfig,
  setAiPolishScreenContextEnabled,
  getAiPolishApiKey,
  setLlmProviderConfig,
  listAiModels,
  getLlmReasoningSupport,
  addCustomProvider,
  getUserProfile,
  getOpenaiCodexOauthStatus,
  loginOpenaiCodexOauth,
  logoutOpenaiCodexOauth,
  setOpenaiFastMode,
  setCustomPrompt,
} from "@/api/tauri";
import type {
  ApiFormat,
  CustomProvider,
  LlmReasoningMode,
  OpenaiAuthMode,
  OpenaiCodexOauthStatus,
} from "@/types";
import { AI_POLISH_ENABLED_CHANGED_EVENT, AI_POLISH_ENABLED_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import Field from "@/components/ui/Field";
import Toggle from "@/components/ui/Toggle";
import Picker from "@/components/ui/Picker";
import SecretInput from "@/components/ui/SecretInput";
import TextInput from "@/components/ui/TextInput";
import TextArea from "@/components/ui/TextArea";
import Segmented from "@/components/ui/Segmented";
import Button from "@/components/ui/Button";
import Badge from "@/components/ui/Badge";
import Modal from "@/components/ui/Modal";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";

const PRESETS = ["openai", "deepseek", "cerebras", "siliconflow", "custom"] as const;

function normalizeProviderId(id: string | null | undefined): string {
  return id === "custom_compat" ? "custom" : id || "cerebras";
}

export function AiPolishSection() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(() => readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
  const [screenContext, setScreenContext] = useState(false);
  const [provider, setProvider] = useState("cerebras");
  const [customProviders, setCustomProviders] = useState<CustomProvider[]>([]);
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [reasoningMode, setReasoningMode] = useState<LlmReasoningMode>("provider_default");
  const [reasoningSupported, setReasoningSupported] = useState(false);
  const [customPrompt, setCustomPromptState] = useState("");
  const [showAddProvider, setShowAddProvider] = useState(false);
  const [authMode, setAuthMode] = useState<OpenaiAuthMode>("api_key");
  const [oauthStatus, setOauthStatus] = useState<OpenaiCodexOauthStatus>({ loggedIn: false });
  const [fastMode, setFastMode] = useState(false);
  const providerRef = useRef(provider);
  const baseUrlRef = useRef(baseUrl);
  const modelRef = useRef(model);
  const reasoningModeRef = useRef(reasoningMode);

  useEffect(() => { providerRef.current = provider; }, [provider]);
  useEffect(() => { baseUrlRef.current = baseUrl; }, [baseUrl]);
  useEffect(() => { modelRef.current = model; }, [model]);
  useEffect(() => { reasoningModeRef.current = reasoningMode; }, [reasoningMode]);

  useEffect(() => {
    const syncEnabled = () => setEnabled(readLocalStorage(AI_POLISH_ENABLED_KEY) === "true");
    window.addEventListener(AI_POLISH_ENABLED_CHANGED_EVENT, syncEnabled);
    return () => window.removeEventListener(AI_POLISH_ENABLED_CHANGED_EVENT, syncEnabled);
  }, []);

  const customProviderFor = useCallback((id: string, list = customProviders) => (
    list.find((item) => item.id === id)
  ), [customProviders]);

  useEffect(() => {
    void getAiPolishApiKey().then(setApiKey).catch(() => {});
    void getUserProfile().then((profile) => {
      const llm = profile.llm_provider;
      const active = normalizeProviderId(llm.active);
      const activeCustomProvider = (llm.custom_providers ?? []).find((item) => item.id === active);
      const nextBaseUrl = activeCustomProvider?.base_url ?? llm.custom_base_url ?? "";
      const nextModel = activeCustomProvider?.model ?? llm.custom_model ?? "";
      const nextReasoningMode = llm.polish_reasoning_mode ?? "provider_default";
      setProvider(active);
      setCustomProviders(llm.custom_providers ?? []);
      setBaseUrl(nextBaseUrl);
      setModel(nextModel);
      setReasoningMode(nextReasoningMode);
      setAuthMode(llm.openai_auth_mode ?? "api_key");
      setFastMode(Boolean(llm.openai_fast_mode));
      setScreenContext(Boolean(profile.ai_polish_screen_context_enabled));
      setCustomPromptState(profile.custom_prompt ?? "");
      providerRef.current = active;
      baseUrlRef.current = nextBaseUrl;
      modelRef.current = nextModel;
      reasoningModeRef.current = nextReasoningMode;
    }).catch(() => {});
    void getOpenaiCodexOauthStatus().then(setOauthStatus).catch(() => {});
  }, []);

  const apiKeySave = useDebouncedCallback((enabledValue: boolean, value: string, saveProvider: string) => {
    void setAiPolishConfig(enabledValue, value, saveProvider).catch(() => toast.error(t("toast.aiPolishFailed", { error: "" })));
  }, 900, { onUnmount: "flush" });

  const persistProviderConfig = useCallback((patch: {
    provider?: string;
    baseUrl?: string;
    model?: string;
    reasoningMode?: LlmReasoningMode;
    authMode?: OpenaiAuthMode;
  } = {}) => {
    const nextProvider = patch.provider ?? providerRef.current;
    const nextBaseUrl = patch.baseUrl ?? baseUrlRef.current;
    const nextModel = patch.model ?? modelRef.current;
    const nextReasoningMode = patch.reasoningMode ?? reasoningModeRef.current;
    return setLlmProviderConfig(
      nextProvider,
      nextBaseUrl,
      nextModel,
      nextReasoningMode,
      undefined,
      undefined,
      undefined,
      undefined,
      patch.authMode,
    );
  }, []);

  const baseUrlSave = useDebouncedCallback((value: string) => {
    void persistProviderConfig({ baseUrl: value }).catch(() => {});
  }, 900, { onUnmount: "flush" });

  const promptSave = useDebouncedCallback((value: string) => {
    void setCustomPrompt(value || null).catch(() => toast.error(t("toast.customPromptSaveFailed")));
  }, 900, { onUnmount: "flush" });

  const handleEnable = useCallback(async (next: boolean) => {
    setEnabled(next);
    writeLocalStorage(AI_POLISH_ENABLED_KEY, next ? "true" : "false");
    window.dispatchEvent(new Event(AI_POLISH_ENABLED_CHANGED_EVENT));
    try {
      await setAiPolishConfig(next, apiKey, providerRef.current);
    } catch {
      setEnabled(!next);
      writeLocalStorage(AI_POLISH_ENABLED_KEY, !next ? "true" : "false");
      window.dispatchEvent(new Event(AI_POLISH_ENABLED_CHANGED_EVENT));
    }
  }, [apiKey]);

  const handleScreenContext = useCallback(async (next: boolean) => {
    const prev = screenContext;
    setScreenContext(next);
    try { await setAiPolishScreenContextEnabled(next); }
    catch { setScreenContext(prev); toast.error(t("toast.polishScreenContextFailed")); }
  }, [screenContext, t]);

  const handleProviderChange = useCallback(async (next: string) => {
    const prev = provider;
    const prevBaseUrl = baseUrlRef.current;
    const prevModel = modelRef.current;
    apiKeySave.flush();
    baseUrlSave.cancel();
    const normalizedNext = normalizeProviderId(next);
    const customProvider = customProviderFor(normalizedNext);
    const nextBaseUrl = customProvider ? customProvider.base_url : normalizedNext === "custom" ? baseUrlRef.current : "";
    const nextModel = customProvider ? customProvider.model : modelRef.current;
    setProvider(normalizedNext);
    setBaseUrl(nextBaseUrl);
    setModel(nextModel);
    providerRef.current = normalizedNext;
    baseUrlRef.current = nextBaseUrl;
    modelRef.current = nextModel;
    try {
      await persistProviderConfig({ provider: normalizedNext, baseUrl: nextBaseUrl, model: nextModel });
      setApiKey(await getAiPolishApiKey());
    } catch {
      setProvider(prev);
      setBaseUrl(prevBaseUrl);
      setModel(prevModel);
      providerRef.current = prev;
      baseUrlRef.current = prevBaseUrl;
      modelRef.current = prevModel;
    }
  }, [apiKeySave, baseUrlSave, provider, customProviderFor, persistProviderConfig]);

  const handleApiKeyChange = useCallback((v: string) => {
    setApiKey(v);
    apiKeySave.schedule(enabled, v, providerRef.current);
  }, [apiKeySave, enabled]);

  const handleBaseUrlChange = useCallback((v: string) => {
    setBaseUrl(v);
    baseUrlRef.current = v;
    baseUrlSave.schedule(v);
  }, [baseUrlSave]);

  const handleModelChange = useCallback(async (next: string) => {
    const prev = model;
    baseUrlSave.cancel();
    setModel(next);
    modelRef.current = next;
    try { await persistProviderConfig({ model: next }); }
    catch { setModel(prev); modelRef.current = prev; }
  }, [baseUrlSave, model, persistProviderConfig]);

  const handleReasoningChange = useCallback(async (next: LlmReasoningMode) => {
    const prev = reasoningMode;
    baseUrlSave.cancel();
    setReasoningMode(next);
    reasoningModeRef.current = next;
    try { await persistProviderConfig({ reasoningMode: next }); }
    catch { setReasoningMode(prev); reasoningModeRef.current = prev; }
  }, [baseUrlSave, reasoningMode, persistProviderConfig]);

  const handleAuthModeChange = useCallback(async (next: OpenaiAuthMode) => {
    const prev = authMode;
    baseUrlSave.cancel();
    setAuthMode(next);
    try {
      await persistProviderConfig({ authMode: next });
    } catch { setAuthMode(prev); }
  }, [baseUrlSave, authMode, persistProviderConfig]);

  const handleOauthLogin = useCallback(async () => {
    try {
      const status = await loginOpenaiCodexOauth();
      setOauthStatus(status);
      toast.success(t("toast.codexOauthLoginSuccess"));
    } catch {
      toast.error(t("toast.codexOauthLoginFailed"));
    }
  }, [t]);

  const handleOauthLogout = useCallback(async () => {
    try {
      await logoutOpenaiCodexOauth();
      setOauthStatus({ loggedIn: false });
      toast.success(t("toast.codexOauthLogoutSuccess"));
    } catch {
      toast.error(t("toast.codexOauthLogoutFailed"));
    }
  }, [t]);

  const handleFastMode = useCallback(async (next: boolean) => {
    const prev = fastMode;
    setFastMode(next);
    try { await setOpenaiFastMode(next); } catch { setFastMode(prev); }
  }, [fastMode]);

  const handlePromptChange = useCallback((v: string) => {
    setCustomPromptState(v);
    promptSave.schedule(v);
  }, [promptSave]);

  const fetchModels = useCallback(async () => {
    if (!apiKey) return;
    try {
      const res = await listAiModels(provider, baseUrl || undefined, apiKey);
      setModels(res.models.map((m) => m.id));
    } catch {
      toast.error(t("settings.fetchModelsFailed"));
    }
  }, [apiKey, provider, baseUrl, t]);

  useEffect(() => {
    if (apiKey && provider) void fetchModels();
  }, [apiKey, provider, fetchModels]);

  useEffect(() => {
    if (!provider) return;
    void getLlmReasoningSupport(provider, baseUrl || undefined, model || undefined).then((s) => {
      setReasoningSupported(s.supported);
    }).catch(() => setReasoningSupported(false));
  }, [provider, baseUrl, model]);

  const providerOptions = useMemo(() => {
    const presetLabels: Record<(typeof PRESETS)[number], { label: string; description?: string }> = {
      openai: { label: "OpenAI", description: t("settings.openaiDesc") },
      deepseek: { label: "DeepSeek", description: t("settings.deepseekDesc") },
      cerebras: { label: "Cerebras", description: t("settings.cerebrasDesc") },
      siliconflow: { label: "SiliconFlow", description: t("settings.siliconflowDesc") },
      custom: { label: t("settings.customCompatLabel"), description: t("settings.customCompatDesc") },
    };
    const presets = PRESETS.map((p) => ({ value: p, ...presetLabels[p] }));
    const custom = customProviders.map((p) => ({ value: p.id, label: p.name, description: p.base_url }));
    return [...presets, ...custom];
  }, [customProviders, t]);

  const modelOptions = useMemo(() => {
    const list = models.map((m) => ({ value: m, label: m }));
    if (model && !list.some((o) => o.value === model)) list.unshift({ value: model, label: model });
    return list;
  }, [models, model]);

  const reasoningOptions: Array<{ value: LlmReasoningMode; label: string; description: string }> = [
    { value: "provider_default", label: t("settings.reasoningDefault"), description: t("settings.reasoningDefaultDesc") },
    { value: "off", label: t("settings.reasoningOff"), description: t("settings.reasoningOffDesc") },
    { value: "light", label: t("settings.reasoningLight"), description: t("settings.reasoningLightDesc") },
    { value: "balanced", label: t("settings.reasoningBalanced"), description: t("settings.reasoningBalancedDesc") },
    { value: "deep", label: t("settings.reasoningDeep"), description: t("settings.reasoningDeepDesc") },
  ];

  const showCustomBaseUrl = provider === "custom" || customProviders.some((p) => p.id === provider);
  const isOpenai = provider === "openai";
  const showFastMode = isOpenai && oauthStatus.loggedIn && authMode === "oauth";
  const hideApiKey = isOpenai && authMode === "oauth" && oauthStatus.loggedIn;

  const providerFooter = (
    <Button
      variant="ghost"
      size="sm"
      icon={<Plus size={14} />}
      onClick={() => setShowAddProvider(true)}
    >
      {t("settings.addCustomProvider")}
    </Button>
  );

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-ai-polish"
      data-nav-id="ai-polish"
    >
      <h2 className="lw-settings-section-title">{t("settings.aiPolish")}</h2>
      <Field label={t("settings.enableAiPolish")}>
        <Toggle
          checked={enabled}
          onChange={(v) => void handleEnable(v)}
          label={t("settings.enableAiPolish")}
          data-testid="polish-enable-toggle"
        />
      </Field>
      <Field label={t("settings.screenContext")} hint={t("settings.screenContextPolishHint")}>
        <Toggle
          checked={screenContext}
          onChange={(v) => void handleScreenContext(v)}
          label={t("settings.screenContext")}
          data-testid="polish-screen-context-toggle"
        />
      </Field>
      <Field label={t("settings.provider")}>
        <Picker
          value={provider}
          options={providerOptions}
          onChange={(v) => void handleProviderChange(v)}
          searchable
          footer={providerFooter}
          data-testid="polish-provider-picker"
        />
      </Field>
      {showCustomBaseUrl && (
        <Field label={t("settings.baseUrl")} hint={t("settings.baseUrlCustomHint")}>
          <TextInput
            value={baseUrl}
            onChange={handleBaseUrlChange}
            placeholder={t("settings.baseUrlPlaceholder")}
            data-testid="polish-base-url"
          />
        </Field>
      )}
      {isOpenai && (
        <Field label={t("settings.openaiAuthModeLabel")}>
          <Segmented
            value={authMode}
            options={[
              { value: "api_key", label: t("settings.openaiAuthModeApiKey") },
              { value: "oauth", label: t("settings.openaiAuthModeOauth") },
            ]}
            onChange={(v) => void handleAuthModeChange(v)}
            data-testid="polish-openai-auth-mode"
          />
          <div className="lw-inline" style={{ marginTop: 8 }}>
            {oauthStatus.loggedIn ? (
              <>
                {oauthStatus.email && <Badge tone="success">{oauthStatus.email}</Badge>}
                {oauthStatus.planType && <Badge tone="accent">{oauthStatus.planType}</Badge>}
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void handleOauthLogout()}
                  data-testid="polish-openai-oauth-btn"
                >
                  {t("settings.codexOauthLogout")}
                </Button>
              </>
            ) : (
              <Button
                size="sm"
                onClick={() => void handleOauthLogin()}
                data-testid="polish-openai-oauth-btn"
              >
                {t("settings.codexOauthLogin")}
              </Button>
            )}
          </div>
          {showFastMode && (
            <div style={{ marginTop: 8 }}>
              <Toggle
                checked={fastMode}
                onChange={(v) => void handleFastMode(v)}
                label={t("settings.fastModeLabel")}
                data-testid="polish-fast-mode-toggle"
              />
            </div>
          )}
        </Field>
      )}
      {!hideApiKey && (
        <Field label={t("settings.apiKey")}>
          <SecretInput
            value={apiKey}
            onChange={handleApiKeyChange}
            placeholder={t("settings.apiKey")}
            data-testid="polish-api-key"
          />
        </Field>
      )}
      <Field label={t("settings.modelLabel")}>
        <Picker
          value={model}
          options={modelOptions}
          onChange={(v) => void handleModelChange(v)}
          searchable
          allowCustomValue
          customValueLabel={(value) => t("settings.useCustomModel", { model: value })}
          placeholder={t("settings.searchModelPlaceholder")}
          data-testid="polish-model-picker"
        />
      </Field>
      {reasoningSupported && (
        <Field label={t("settings.polishReasoningMode")}>
          <Picker
            value={reasoningMode}
            options={reasoningOptions}
            onChange={(v) => void handleReasoningChange(v as LlmReasoningMode)}
            data-testid="polish-reasoning-picker"
          />
        </Field>
      )}
      <Field label={t("settings.customPrompt")} hint={t("settings.customPromptHint")}>
        <TextArea
          value={customPrompt}
          onChange={handlePromptChange}
          placeholder={t("settings.customPromptPlaceholder")}
          data-testid="polish-custom-prompt"
        />
      </Field>

      <AddProviderModal
        open={showAddProvider}
        onClose={() => setShowAddProvider(false)}
        onAdd={(provider) => {
          setCustomProviders((items) => [...items, provider]);
          setProvider(provider.id);
          setBaseUrl(provider.base_url);
          setModel(provider.model);
          providerRef.current = provider.id;
          baseUrlRef.current = provider.base_url;
          modelRef.current = provider.model;
          void persistProviderConfig({
            provider: provider.id,
            baseUrl: provider.base_url,
            model: provider.model,
          }).then(async () => {
            setApiKey(await getAiPolishApiKey());
          }).catch(() => {});
        }}
      />
    </section>
  );
}

function AddProviderModal({
  open,
  onClose,
  onAdd,
}: {
  open: boolean;
  onClose: () => void;
  onAdd: (provider: CustomProvider) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [model, setModel] = useState("");
  const [format, setFormat] = useState<ApiFormat>("openai_compat");

  const submit = useCallback(async () => {
    if (!name || !baseUrl || !model) return;
    try {
      const id = await addCustomProvider(name, baseUrl, model, format);
      onAdd({ id, name, base_url: baseUrl, model, api_format: format });
      setName(""); setBaseUrl(""); setModel(""); setFormat("openai_compat");
      onClose();
    } catch {
      toast.error(t("settings.fetchModelsFailed"));
    }
  }, [name, baseUrl, model, format, onAdd, onClose, t]);

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t("settings.addCustomProvider")}
      data-testid="modal-add-provider"
    >
      <div className="lw-stack lw-stack--md">
        <Field label={t("settings.providerName")}>
          <TextInput
            value={name}
            onChange={setName}
            placeholder={t("settings.providerNameLabel")}
            data-testid="custom-provider-name"
          />
        </Field>
        <Field label={t("settings.baseUrl")}>
          <TextInput
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder={t("settings.baseUrlPlaceholder")}
            data-testid="custom-provider-base-url"
          />
        </Field>
        <Field label={t("settings.defaultModel")}>
          <TextInput
            value={model}
            onChange={setModel}
            placeholder={t("settings.modelNamePlaceholder")}
            data-testid="custom-provider-model"
          />
        </Field>
        <Field label={t("settings.apiFormatLabel")}>
          <Segmented
            value={format}
            options={[
              { value: "openai_compat", label: t("settings.openaiCompat") },
              { value: "anthropic", label: "Anthropic" },
            ]}
            onChange={(v) => setFormat(v as ApiFormat)}
          />
        </Field>
        <div className="lw-inline" style={{ justifyContent: "flex-end" }}>
          <Button variant="ghost" onClick={onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" onClick={() => void submit()} data-testid="custom-provider-submit">
            {t("common.add")}
          </Button>
        </div>
      </div>
    </Modal>
  );
}

export default AiPolishSection;
