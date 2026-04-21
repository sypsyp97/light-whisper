import { useCallback, useEffect, useMemo, useState } from "react";
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
import { AI_POLISH_ENABLED_KEY } from "@/lib/constants";
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

const PRESETS = ["openai", "deepseek", "cerebras", "siliconflow", "custom_compat"] as const;

export default function AiPolishSection() {
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

  useEffect(() => {
    void getAiPolishApiKey().then(setApiKey).catch(() => {});
    void getUserProfile().then((profile) => {
      const llm = profile.llm_provider;
      setProvider(llm.active || "cerebras");
      setCustomProviders(llm.custom_providers ?? []);
      setBaseUrl(llm.custom_base_url ?? "");
      setModel(llm.custom_model ?? "");
      setReasoningMode(llm.polish_reasoning_mode ?? "provider_default");
      setAuthMode(llm.openai_auth_mode ?? "api_key");
      setFastMode(Boolean(llm.openai_fast_mode));
      setScreenContext(Boolean(profile.ai_polish_screen_context_enabled));
      setCustomPromptState(profile.custom_prompt ?? "");
    }).catch(() => {});
    void getOpenaiCodexOauthStatus().then(setOauthStatus).catch(() => {});
  }, []);

  const apiKeySave = useDebouncedCallback((enabledValue: boolean, value: string) => {
    void setAiPolishConfig(enabledValue, value).catch(() => toast.error(t("toast.aiPolishFailed", { error: "" })));
  }, 900, { onUnmount: "flush" });

  const baseUrlSave = useDebouncedCallback((value: string) => {
    void setLlmProviderConfig(provider, value, model, reasoningMode).catch(() => {});
  }, 900, { onUnmount: "flush" });

  const promptSave = useDebouncedCallback((value: string) => {
    void setCustomPrompt(value || null).catch(() => toast.error(t("toast.customPromptSaveFailed")));
  }, 900, { onUnmount: "flush" });

  const handleEnable = useCallback(async (next: boolean) => {
    setEnabled(next);
    writeLocalStorage(AI_POLISH_ENABLED_KEY, next ? "true" : "false");
    try { await setAiPolishConfig(next, apiKey); } catch { setEnabled(!next); }
  }, [apiKey]);

  const handleScreenContext = useCallback(async (next: boolean) => {
    const prev = screenContext;
    setScreenContext(next);
    try { await setAiPolishScreenContextEnabled(next); }
    catch { setScreenContext(prev); toast.error(t("toast.polishScreenContextFailed")); }
  }, [screenContext, t]);

  const handleProviderChange = useCallback(async (next: string) => {
    const prev = provider;
    setProvider(next);
    try { await setLlmProviderConfig(next, baseUrl, model, reasoningMode); }
    catch { setProvider(prev); }
  }, [provider, baseUrl, model, reasoningMode]);

  const handleApiKeyChange = useCallback((v: string) => {
    setApiKey(v);
    apiKeySave.schedule(enabled, v);
  }, [apiKeySave, enabled]);

  const handleBaseUrlChange = useCallback((v: string) => {
    setBaseUrl(v);
    baseUrlSave.schedule(v);
  }, [baseUrlSave]);

  const handleModelChange = useCallback(async (next: string) => {
    const prev = model;
    setModel(next);
    try { await setLlmProviderConfig(provider, baseUrl, next, reasoningMode); }
    catch { setModel(prev); }
  }, [model, provider, baseUrl, reasoningMode]);

  const handleReasoningChange = useCallback(async (next: LlmReasoningMode) => {
    const prev = reasoningMode;
    setReasoningMode(next);
    try { await setLlmProviderConfig(provider, baseUrl, model, next); }
    catch { setReasoningMode(prev); }
  }, [reasoningMode, provider, baseUrl, model]);

  const handleAuthModeChange = useCallback(async (next: OpenaiAuthMode) => {
    const prev = authMode;
    setAuthMode(next);
    try {
      await setLlmProviderConfig(provider, baseUrl, model, reasoningMode, undefined, undefined, undefined, undefined, next);
    } catch { setAuthMode(prev); }
  }, [authMode, provider, baseUrl, model, reasoningMode]);

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
    const presets = PRESETS.map((p) => ({ value: p, label: p }));
    const custom = customProviders.map((p) => ({ value: p.id, label: p.name, description: p.base_url }));
    return [...presets, ...custom];
  }, [customProviders]);

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

  const showCustomBaseUrl = provider === "custom_compat" || customProviders.some((p) => p.id === provider);
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
        onAdd={(provider) => { setCustomProviders((p) => [...p, provider]); }}
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
          <TextInput value={name} onChange={setName} placeholder={t("settings.providerNameLabel")} />
        </Field>
        <Field label={t("settings.baseUrl")}>
          <TextInput value={baseUrl} onChange={setBaseUrl} placeholder={t("settings.baseUrlPlaceholder")} />
        </Field>
        <Field label={t("settings.defaultModel")}>
          <TextInput value={model} onChange={setModel} placeholder={t("settings.modelNamePlaceholder")} />
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
          <Button variant="primary" onClick={() => void submit()}>{t("common.add")}</Button>
        </div>
      </div>
    </Modal>
  );
}
