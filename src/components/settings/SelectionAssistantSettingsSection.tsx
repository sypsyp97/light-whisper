import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronsUpDown, MousePointer2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import SecretInput from "@/components/SecretInput";
import TranslationLanguagePicker from "@/components/settings/TranslationLanguagePicker";
import { resolveSelectionModelConfig } from "@/features/selection-assistant/modelConfig";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";
import {
  findLlmPreset,
  llmProviderOptions,
  reasoningModeOptions,
} from "@/lib/llmModelOptions";
import {
  getSelectionApiKey,
  listAiModels,
  setSelectionApiKey,
  setSelectionAssistantConfig,
} from "@/api/tauri";
import {
  shouldShowGrokBuildAuth,
  shouldUseGrokBuildOauth,
} from "@/lib/grokBuildAuth";
import type { AiModelInfo, LlmReasoningMode, OpenaiAuthMode, UserProfile, XaiAuthMode } from "@/types";

interface SelectionAssistantSettingsSectionProps {
  profile: UserProfile | null;
  openaiAuthMode: OpenaiAuthMode;
  openaiOauthLoggedIn: boolean;
  xaiAuthMode?: XaiAuthMode;
  grokOauthLoggedIn?: boolean;
  openaiControls: ReactNode;
  grokAuthToggle?: ReactNode;
  grokOauthBlock?: ReactNode;
}

export default function SelectionAssistantSettingsSection({
  profile,
  openaiAuthMode,
  openaiOauthLoggedIn,
  xaiAuthMode = "api_key",
  grokOauthLoggedIn = false,
  openaiControls,
  grokAuthToggle = null,
  grokOauthBlock = null,
}: SelectionAssistantSettingsSectionProps) {
  const { t } = useTranslation();
  const picker = useExclusivePicker<
    "selectionProvider" | "selectionModel" | "selectionReasoning" | "selectionLanguage"
  >();
  const [enabled, setEnabled] = useState(false);
  const [autoScreenshot, setAutoScreenshot] = useState(false);
  const [translationTarget, setTranslationTarget] = useState("English");
  const [excludedApps, setExcludedApps] = useState("");
  const [separate, setSeparate] = useState(false);
  const [provider, setProvider] = useState("openai");
  const [model, setModel] = useState("gpt-4.1-mini");
  const [reasoning, setReasoning] = useState<LlmReasoningMode>("provider_default");
  const [apiKey, setApiKey] = useState("");
  const [providerSearch, setProviderSearch] = useState("");
  const [modelSearch, setModelSearch] = useState("");
  const [availableModels, setAvailableModels] = useState<AiModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState("");
  const [modelRefreshToken, setModelRefreshToken] = useState(0);
  const selectionConfigDirty = useRef(false);
  const selectionConfigRevision = useRef(0);
  const lastHydratedSelectionConfig = useRef<string | null>(null);
  const latestSelectionConfig = useRef<Parameters<typeof setSelectionAssistantConfig>[0] | null>(null);
  latestSelectionConfig.current = {
    enabled,
    autoScreenshot,
    translationTarget,
    excludedApps: excludedApps.split(/[,;\n]/).map((value) => value.trim()).filter(Boolean),
    useSeparateModel: separate,
    provider: separate ? provider : null,
    model: separate ? model : null,
    reasoningMode: reasoning,
  };
  const selectionKeySave = useDebouncedCallback((keyProvider: string, value: string) => {
    setSelectionApiKey(keyProvider, value).catch(() => {
      toast.error(t("settings.selectionSaveFailed"));
    });
  }, 400, { onUnmount: "flush" });
  const selectionConfigSave = useDebouncedCallback(() => {
    const config = latestSelectionConfig.current;
    if (config === null) return;
    const revision = selectionConfigRevision.current;
    return setSelectionAssistantConfig(config).then(() => {
      if (selectionConfigRevision.current === revision) {
        selectionConfigDirty.current = false;
      }
    }).catch(() => {
      toast.error(t("settings.selectionSaveFailed"));
    });
  }, 350, { onUnmount: "flush" });
  const scheduleSelectionConfigSave = () => {
    selectionConfigDirty.current = true;
    selectionConfigRevision.current += 1;
    selectionConfigSave.schedule();
  };

  const profileSelectionConfig = useMemo(() => {
    if (!profile) return null;
    const config = profile.selection_assistant ?? {
      enabled: false,
      auto_screenshot: false,
      translation_target: "English",
      excluded_apps: ["light-whisper.exe", "snipaste.exe", "pixpin.exe", "sharex.exe"],
    };
    const resolved = resolveSelectionModelConfig(profile.llm_provider);
    const customProvider = profile.llm_provider.custom_providers
      ?.find((item) => item.id === resolved.provider);
    const defaultModel = customProvider?.model ?? findLlmPreset(resolved.provider).defaultModel;
    const value: Parameters<typeof setSelectionAssistantConfig>[0] = {
      enabled: config.enabled,
      autoScreenshot: Boolean(config.auto_screenshot),
      translationTarget: config.translation_target,
      excludedApps: config.excluded_apps,
      useSeparateModel: !resolved.followsPolish,
      provider: resolved.provider,
      model: resolved.model || defaultModel || "",
      reasoningMode: resolved.reasoningMode,
    };
    return {
      signature: JSON.stringify(value),
      value,
    };
  }, [profile]);

  const providers = useMemo(() => [
    ...llmProviderOptions.map((item) => ({
      ...item,
      desc: t(item.descKey),
      isCustom: false as const,
    })),
    ...(profile?.llm_provider.custom_providers ?? []).map((item) => ({
      key: item.id,
      label: item.name,
      desc: item.api_format === "anthropic" ? "Anthropic" : t("settings.openaiCompat"),
      baseUrl: item.base_url,
      defaultModel: item.model,
      models: [item.model] as readonly string[],
      isCustom: true as const,
    })),
  ], [profile?.llm_provider.custom_providers, t]);
  const currentProvider = providers.find((item) => item.key === provider)
    ?? { ...findLlmPreset(provider), desc: "", isCustom: false as const };
  const filteredProviders = providers.filter((item) => {
    const keyword = providerSearch.trim().toLowerCase();
    return !keyword || `${item.label} ${item.desc} ${item.baseUrl}`.toLowerCase().includes(keyword);
  });
  const effectiveModels: AiModelInfo[] = availableModels.length > 0
    ? availableModels
    : currentProvider.models.map((id) => ({ id }));
  const filteredModels = effectiveModels.filter((item) => {
    const keyword = modelSearch.trim().toLowerCase();
    return !keyword || item.id.toLowerCase().includes(keyword) || (item.ownedBy ?? "").toLowerCase().includes(keyword);
  });
  const selectedReasoning = reasoningModeOptions.find((item) => item.key === reasoning)
    ?? reasoningModeOptions[0];

  useEffect(() => {
    if (
      profileSelectionConfig === null
      || profileSelectionConfig.signature === lastHydratedSelectionConfig.current
      || selectionConfigDirty.current
    ) return;
    const config = profileSelectionConfig.value;
    setEnabled(config.enabled);
    setAutoScreenshot(config.autoScreenshot);
    setTranslationTarget(config.translationTarget);
    setExcludedApps(config.excludedApps.join("\n"));
    setSeparate(config.useSeparateModel);
    setProvider(config.provider ?? "openai");
    setModel(config.model ?? "");
    setReasoning(config.reasoningMode);
    lastHydratedSelectionConfig.current = profileSelectionConfig.signature;
  }, [profileSelectionConfig]);

  useEffect(() => {
    if (!profile || !separate) {
      setApiKey("");
      return;
    }
    let disposed = false;
    void getSelectionApiKey(provider).then((value) => {
      if (!disposed) setApiKey(value);
    }).catch(() => {
      if (!disposed) setApiKey("");
    });
    return () => { disposed = true; };
  }, [Boolean(profile), provider, separate]);

  useEffect(() => {
    if (!separate) {
      setAvailableModels([]);
      setModelsError("");
      return;
    }
    setAvailableModels(currentProvider.models.map((id) => ({ id })));
    const hasAuth = shouldUseGrokBuildOauth({
      provider,
      authMode: xaiAuthMode,
      loggedIn: grokOauthLoggedIn,
    })
      || (provider === "openai"
        ? (openaiAuthMode === "oauth" ? openaiOauthLoggedIn : Boolean(apiKey.trim()))
        : provider !== "xai" && Boolean(apiKey.trim()))
      || (provider === "xai" && xaiAuthMode === "api_key" && Boolean(apiKey.trim()));
    if (!hasAuth) return;

    let disposed = false;
    const timer = window.setTimeout(() => {
      setModelsLoading(true);
      setModelsError("");
      void listAiModels(
        provider,
        currentProvider.isCustom ? currentProvider.baseUrl : undefined,
        apiKey,
        modelRefreshToken > 0,
        provider === "openai" ? openaiAuthMode : undefined,
        provider === "xai" ? xaiAuthMode : undefined,
      ).then((payload) => {
        if (!disposed) setAvailableModels(payload.models);
      }).catch((requestError) => {
        if (!disposed) {
          setModelsError(requestError instanceof Error ? requestError.message : String(requestError));
        }
      }).finally(() => {
        if (!disposed) setModelsLoading(false);
      });
    }, 350);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [apiKey, currentProvider.baseUrl, currentProvider.isCustom, currentProvider.models, grokOauthLoggedIn, modelRefreshToken, openaiAuthMode, openaiOauthLoggedIn, provider, separate, xaiAuthMode]);

  return (
    <section className="settings-card" data-nav-id="selection-assistant">
      <div className="settings-section-header">
        <MousePointer2 size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.selectionAssistant")}</h2>
      </div>
      <div className="settings-column" style={{ gap: 12 }}>
        <p className="settings-hint" style={{ margin: 0 }}>{t("settings.selectionAssistantHint")}</p>

        <div className="settings-row">
          <div className="settings-column" style={{ gap: 2 }}>
            <span className="permission-label">{t("settings.selectionAssistantEnabled")}</span>
            <span className="settings-hint" style={{ margin: 0 }}>{t("settings.selectionAssistantEnabledHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={enabled}
            aria-label={t("settings.selectionAssistantEnabled")}
            className="toggle-switch"
            onClick={() => {
              setEnabled((value) => !value);
              scheduleSelectionConfigSave();
            }}
            style={{ background: enabled ? "var(--color-accent)" : "var(--color-bg-tertiary)", flexShrink: 0 }}
          >
            <div className="toggle-knob" style={{ transform: enabled ? "translateX(20px)" : "translateX(0)" }} />
          </button>
        </div>

        <div className="settings-row">
          <div className="settings-column" style={{ gap: 2 }}>
            <span className="permission-label">{t("settings.selectionAutoScreenshot")}</span>
            <span className="settings-hint" style={{ margin: 0 }}>{t("settings.selectionAutoScreenshotHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={autoScreenshot}
            aria-label={t("settings.selectionAutoScreenshot")}
            className="toggle-switch"
            onClick={() => {
              setAutoScreenshot((value) => !value);
              scheduleSelectionConfigSave();
            }}
            style={{ background: autoScreenshot ? "var(--color-accent)" : "var(--color-bg-tertiary)", flexShrink: 0 }}
          >
            <div className="toggle-knob" style={{ transform: autoScreenshot ? "translateX(20px)" : "translateX(0)" }} />
          </button>
        </div>

        <div className="settings-row">
          <div className="settings-column" style={{ gap: 2 }}>
            <span className="permission-label">{t("settings.selectionSeparateConfig")}</span>
            <span className="settings-hint" style={{ margin: 0 }}>{t("settings.selectionSeparateConfigHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={separate}
            aria-label={t("settings.selectionSeparateConfig")}
            className="toggle-switch"
            onClick={() => {
              setSeparate((value) => !value);
              scheduleSelectionConfigSave();
            }}
            style={{ background: separate ? "var(--color-accent)" : "var(--color-bg-tertiary)", flexShrink: 0 }}
          >
            <div className="toggle-knob" style={{ transform: separate ? "translateX(20px)" : "translateX(0)" }} />
          </button>
        </div>

        {separate && (
          <div className="settings-column" style={{ gap: 8 }}>
            <span className="settings-option-desc">{t("settings.selectionProvider")}</span>
            <div className="picker-shell" ref={picker.setRef("selectionProvider")}>
              <button
                type="button"
                className="picker-trigger"
                data-open={picker.isOpen("selectionProvider")}
                aria-haspopup="listbox"
                aria-expanded={picker.isExpanded("selectionProvider")}
                aria-label={t("settings.selectionProvider")}
                onClick={() => picker.toggle("selectionProvider")}
              >
                <span className="picker-trigger-copy">
                  <strong>{currentProvider.label}</strong>
                  <span>{currentProvider.baseUrl}</span>
                </span>
                <ChevronsUpDown size={14} className="icon-tertiary" />
              </button>
              {picker.isOpen("selectionProvider") && (
                <div className={picker.popoverClass("selectionProvider")}>
                  <div className="picker-toolbar">
                    <input
                      type="text"
                      className="settings-input picker-search-input"
                      placeholder={t("settings.searchAssistantProvider")}
                      aria-label={t("settings.searchAssistantProviderLabel")}
                      value={providerSearch}
                      onChange={(event) => setProviderSearch(event.target.value)}
                      autoFocus
                    />
                  </div>
                  <div className="picker-list" role="listbox">
                    {filteredProviders.map((item) => (
                      <button
                        key={item.key}
                        type="button"
                        className="picker-option"
                        data-active={provider === item.key}
                        onClick={() => {
                          void selectionKeySave.flush();
                          setProvider(item.key);
                          setModel(item.defaultModel);
                          setProviderSearch("");
                          setModelSearch("");
                          setModelRefreshToken(0);
                          picker.close();
                          scheduleSelectionConfigSave();
                        }}
                      >
                        <span className="picker-option-copy">
                          <strong>{item.label}</strong>
                          <span>{item.desc}</span>
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>

            {provider === "openai" ? openaiControls : null}
            {shouldShowGrokBuildAuth(provider) ? grokAuthToggle : null}

            <div className="settings-column" style={{ gap: 4 }}>
              <span className="settings-option-desc">{currentProvider.label} API Key</span>
              <SecretInput
                value={apiKey}
                onChange={(value) => {
                  setApiKey(value);
                  selectionKeySave.schedule(provider, value);
                }}
                placeholder={`${currentProvider.label} API Key`}
                ariaLabel={t("settings.selectionApiKey")}
                ariaLabelShow={t("settings.showApiKey")}
                ariaLabelHide={t("settings.hideApiKey")}
              />
            </div>

            {shouldShowGrokBuildAuth(provider) ? grokOauthBlock : null}

            <div className="settings-row">
              <span className="settings-option-desc">{t("settings.selectionModel")}</span>
              <span className="settings-option-desc">{filteredModels.length}/{effectiveModels.length}</span>
            </div>
            <div className="picker-shell" ref={picker.setRef("selectionModel")}>
              <div className="picker-inline-row">
                <input
                  className="settings-input"
                  value={model}
                  placeholder={t("settings.assistantModelPlaceholder")}
                  aria-label={t("settings.assistantModelLabel")}
                  onChange={(event) => {
                    setModel(event.target.value);
                    scheduleSelectionConfigSave();
                  }}
                />
                <button
                  type="button"
                  className="picker-inline-button"
                  data-open={picker.isOpen("selectionModel")}
                  aria-haspopup="listbox"
                  aria-expanded={picker.isExpanded("selectionModel")}
                  aria-label={t("settings.openAssistantModelList")}
                  title={t("settings.openAssistantModelList")}
                  onClick={() => picker.toggle("selectionModel")}
                >
                  <ChevronsUpDown size={14} className="icon-tertiary" />
                </button>
              </div>
              {picker.isOpen("selectionModel") && (
                <div className={picker.popoverClass("selectionModel")}>
                  <div className="picker-toolbar">
                    <input
                      type="text"
                      className="settings-input picker-search-input"
                      placeholder={t("settings.searchModelPlaceholder")}
                      aria-label={t("settings.searchAssistantModel")}
                      value={modelSearch}
                      onChange={(event) => setModelSearch(event.target.value)}
                    />
                    <button
                      type="button"
                      className="btn-ghost btn-ghost-sm"
                      disabled={modelsLoading}
                      onClick={() => setModelRefreshToken((value) => value + 1)}
                    >
                      {modelsLoading ? t("settings.fetching") : t("common.refresh")}
                    </button>
                  </div>
                  {modelSearch.trim() ? (
                    <button
                      type="button"
                      className="picker-option picker-option-action"
                      onClick={() => {
                        setModel(modelSearch.trim());
                        setModelSearch("");
                        picker.close();
                        scheduleSelectionConfigSave();
                      }}
                    >
                      <span className="picker-option-copy">
                        <strong>{t("settings.useAsModel", { name: modelSearch.trim() })}</strong>
                        <span>{t("settings.asAssistantModelName")}</span>
                      </span>
                    </button>
                  ) : null}
                  <div className="picker-list" role="listbox">
                    {filteredModels.length > 0 ? filteredModels.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        className="picker-option"
                        data-active={model === item.id}
                        onClick={() => {
                          setModel(item.id);
                          setModelSearch("");
                          picker.close();
                          scheduleSelectionConfigSave();
                        }}
                      >
                        <span className="picker-option-copy">
                          <strong>{item.id}</strong>
                          <span>{item.ownedBy || currentProvider.label}</span>
                        </span>
                      </button>
                    )) : (
                      <div className="picker-empty">
                        {modelsLoading ? t("settings.fetchModelsFromApi") : modelsError || t(provider === "xai" ? "settings.fillApiKeyOrGrokLogin" : "settings.fillApiKeyOrLogin")}
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>

            <span className="settings-option-desc">{t("settings.selectionReasoning")}</span>
            <div className="picker-shell" ref={picker.setRef("selectionReasoning")}>
              <button
                type="button"
                className="picker-trigger"
                data-open={picker.isOpen("selectionReasoning")}
                aria-haspopup="listbox"
                aria-expanded={picker.isExpanded("selectionReasoning")}
                aria-label={t("settings.assistantReasoningLabel")}
                onClick={() => picker.toggle("selectionReasoning")}
              >
                <span className="picker-trigger-copy">
                  <strong>{t(selectedReasoning.labelKey)}</strong>
                  <span>{t(selectedReasoning.descKey)}</span>
                </span>
                <ChevronsUpDown size={14} className="icon-tertiary" />
              </button>
              {picker.isOpen("selectionReasoning") && (
                <div className={picker.popoverClass("selectionReasoning")}>
                  <div className="picker-list" role="listbox">
                    {reasoningModeOptions.map((item) => (
                      <button
                        key={item.key}
                        type="button"
                        className="picker-option"
                        data-active={reasoning === item.key}
                        onClick={() => {
                          setReasoning(item.key);
                          picker.close();
                          scheduleSelectionConfigSave();
                        }}
                      >
                        <span className="picker-option-copy">
                          <strong>{t(item.labelKey)}</strong>
                          <span>{t(item.descKey)}</span>
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}

        <div className="settings-column" style={{ gap: 4 }}>
          <span className="settings-option-desc">{t("settings.selectionTranslationTarget")}</span>
          <TranslationLanguagePicker
            value={translationTarget}
            maxLength={80}
            ariaLabel={t("settings.selectionTranslationTarget")}
            control={{
              open: picker.isOpen("selectionLanguage"),
              expanded: picker.isExpanded("selectionLanguage"),
              toggle: () => picker.toggle("selectionLanguage"),
              close: picker.close,
              setRef: picker.setRef("selectionLanguage"),
              popoverClassName: picker.popoverClass("selectionLanguage"),
            }}
            onSelect={(nextTarget) => {
              if (!nextTarget) return;
              setTranslationTarget(nextTarget);
              scheduleSelectionConfigSave();
            }}
          />
        </div>

        <label className="settings-column" style={{ gap: 4 }}>
          <span className="settings-option-desc">{t("settings.selectionExcludedApps")}</span>
          <textarea
            className="settings-input"
            rows={4}
            value={excludedApps}
            onChange={(event) => {
              setExcludedApps(event.target.value);
              scheduleSelectionConfigSave();
            }}
          />
          <span className="settings-hint">{t("settings.selectionExcludedAppsHint")}</span>
        </label>
      </div>
    </section>
  );
}
