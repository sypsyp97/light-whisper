import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { getJevApiKey, getJevConnection, setJevApiKey, setJevConnection, setJevProvider, setJevFeatures, validateCorrections } from "@/api/tauri";
import SecretInput from "@/components/SecretInput";
import ModelConnectionTest from "./ModelConnectionTest";
import { SettingsReveal } from "./SettingsReveal";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import type { JevConnection, JevFeatures, JevProvider, UserProfile } from "@/types";

const PROVIDERS: ReadonlyArray<{ value: JevProvider; label: string }> = [
  { value: "typesafe", label: "TypeSafe (official)" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "vercel", label: "Vercel" },
  { value: "liquid", label: "Liquid AI" },
  { value: "custom", label: "Custom" },
];

const EMPTY_CONNECTION: JevConnection = { base_url: "", model: "" };

interface JevSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
  polishEnabled?: boolean;
}

const FEATURES = [
  ["correction_review", "jevCorrectionReview"],
  ["polish_audit", "jevPolishAudit"],
] as const;

export default function JevSettingsSection({ profile, onSaved, polishEnabled = true }: JevSettingsSectionProps) {
  const { t } = useTranslation();
  const savedEnabled = Boolean(profile?.jev?.enabled);
  const savedProvider = profile?.jev?.provider ?? "typesafe";
  const correctionReview = Boolean(profile?.jev?.correction_review);
  const polishAudit = Boolean(profile?.jev?.polish_audit);
  const [features, setFeatures] = useState<JevFeatures>({
    correction_review: correctionReview, polish_audit: polishAudit,
  });
  const enabled = savedEnabled;
  const [provider, setProvider] = useState<JevProvider>(savedProvider);
  const [connection, setConnection] = useState<JevConnection>(EMPTY_CONNECTION);
  const anyEnabled = (enabled && polishEnabled) || Object.values(features).some(Boolean)
    || Boolean(profile?.jev?.screen_routing && (profile.ai_polish_screen_context_enabled || profile.assistant_screen_context_enabled))
    || Boolean(profile?.selection_assistant?.auto_screenshot && profile.selection_assistant.screenshot_routing)
    || Boolean(profile?.jev?.search_routing && profile.web_search?.enabled);
  const [apiKey, setApiKey] = useState("");
  const [configSaving, setConfigSaving] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [keyLoadError, setKeyLoadError] = useState(false);
  const [connectionLoadError, setConnectionLoadError] = useState(false);
  const providerRef = useRef<JevProvider>(provider);
  const providerEditedRef = useRef(false);
  const apiKeyRef = useRef(apiKey);
  const keyRequestIdRef = useRef(0);
  const keyDraftRef = useRef(false);
  const connectionRequestIdRef = useRef(0);
  const connectionsRef = useRef<Partial<Record<JevProvider, JevConnection>>>({});
  const connectionEditsRef = useRef<Partial<Record<JevProvider, Partial<JevConnection>>>>({});
  const loadedConnectionsRef = useRef(new Set<JevProvider>());
  const connectionReadsRef = useRef<Partial<Record<JevProvider, Promise<void>>>>({});
  const failedKeyRef = useRef<[JevProvider, string] | null>(null);
  const failedConnectionRef = useRef<[JevProvider, JevConnection] | null>(null);
  const configTransitionRef = useRef(false);
  providerRef.current = provider;
  apiKeyRef.current = apiKey;

  const saveKey = useDebouncedCallback(async (targetProvider: JevProvider, value: string) => {
    try {
      await setJevApiKey(targetProvider, value);
      failedKeyRef.current = null;
    } catch {
      failedKeyRef.current = [targetProvider, value];
      toast.error(t("settings.jevSaveFailed"));
      throw new Error("Decision key save failed");
    }
  }, 600, { onUnmount: "flush" });

  const saveConnection = useDebouncedCallback(async (targetProvider: JevProvider, value: JevConnection) => {
    try {
      await setJevConnection(targetProvider, value.base_url, value.model);
      failedConnectionRef.current = null;
      onSaved();
    } catch {
      failedConnectionRef.current = [targetProvider, value];
      toast.error(t("settings.jevSaveFailed"));
      throw new Error("Decision connection save failed");
    }
  }, 600, { onUnmount: "flush" });

  const flushSettings = useCallback(async () => {
    const targetProvider = providerRef.current;
    const edits = connectionEditsRef.current[targetProvider];
    if (edits && !loadedConnectionsRef.current.has(targetProvider)
      && (edits.base_url === undefined || edits.model === undefined)) {
      await connectionReadsRef.current[targetProvider];
      if (!loadedConnectionsRef.current.has(targetProvider)) {
        throw new Error("Decision connection has not loaded");
      }
    }
    if (failedKeyRef.current) {
      const [failedProvider, value] = failedKeyRef.current;
      saveKey.schedule(failedProvider, failedProvider === providerRef.current ? apiKeyRef.current : value);
    }
    if (failedConnectionRef.current) {
      const [failedProvider, value] = failedConnectionRef.current;
      saveConnection.schedule(failedProvider, connectionsRef.current[failedProvider] ?? value);
    }
    await Promise.all([saveKey.flush(), saveConnection.flush()]);
  }, [saveKey, saveConnection]);

  const refreshApiKey = useCallback(async (targetProvider: JevProvider) => {
    const requestId = ++keyRequestIdRef.current;
    setKeyLoadError(false);
    try {
      const value = await getJevApiKey(targetProvider);
      if (
        requestId === keyRequestIdRef.current
        && providerRef.current === targetProvider
        && !keyDraftRef.current
      ) {
        setApiKey(value || "");
      }
    } catch {
      if (requestId === keyRequestIdRef.current && providerRef.current === targetProvider) {
        setKeyLoadError(true);
        if (!keyDraftRef.current) setApiKey("");
      }
    }
  }, []);

  const refreshConnection = useCallback((targetProvider: JevProvider) => {
    const requestId = ++connectionRequestIdRef.current;
    setConnectionLoadError(false);
    const pending = getJevConnection(targetProvider).then((value) => {
      if (requestId === connectionRequestIdRef.current && providerRef.current === targetProvider) {
        const edits = connectionEditsRef.current[targetProvider];
        const needsSave = edits && !loadedConnectionsRef.current.has(targetProvider)
          && (edits.base_url === undefined || edits.model === undefined);
        const merged = { ...value, ...edits };
        loadedConnectionsRef.current.add(targetProvider);
        connectionsRef.current[targetProvider] = merged;
        setConnection(merged);
        if (needsSave) saveConnection.schedule(targetProvider, merged);
      }
    }, () => {
      if (requestId === connectionRequestIdRef.current && providerRef.current === targetProvider) {
        setConnectionLoadError(true);
      }
    });
    connectionReadsRef.current[targetProvider] = pending;
    return pending;
  }, [saveConnection]);

  useEffect(() => {
    setFeatures({ correction_review: correctionReview, polish_audit: polishAudit });
  }, [correctionReview, polishAudit]);

  useEffect(() => {
    // Profile refreshes can finish out of order after a local selection.
    if (providerEditedRef.current) return;
    setProvider(savedProvider);
    providerRef.current = savedProvider;
    setConnection(connectionsRef.current[savedProvider] ?? EMPTY_CONNECTION);
  }, [savedProvider]);

  useEffect(() => {
    let active = true;
    if (!anyEnabled) {
      keyRequestIdRef.current += 1;
      keyDraftRef.current = false;
      setKeyLoadError(false);
      setConnectionLoadError(false);
      void flushSettings().then(() => {
        if (active) {
          connectionRequestIdRef.current += 1;
          setApiKey("");
        }
      }).catch(() => undefined);
    } else {
      void flushSettings().then(() => {
        if (active) {
          void refreshApiKey(provider);
          void refreshConnection(provider);
        }
      }).catch(() => undefined);
    }
    return () => { active = false; };
  }, [anyEnabled, provider, refreshApiKey, refreshConnection, flushSettings]);

  const reserveConfigTransition = useCallback(() => {
    if (configTransitionRef.current) return false;
    configTransitionRef.current = true;
    setConfigSaving(true);
    return true;
  }, []);

  const releaseConfigTransition = useCallback(() => {
    configTransitionRef.current = false;
    setConfigSaving(false);
  }, []);

  const persistConfig = useCallback(async (
    nextProvider: JevProvider,
    previousProvider: JevProvider,
  ) => {
    try {
      await setJevProvider(nextProvider);
      onSaved();
    } catch {
      setProvider(previousProvider);
      providerRef.current = previousProvider;
      keyRequestIdRef.current += 1;
      connectionRequestIdRef.current += 1;
      keyDraftRef.current = false;
      setApiKey("");
      setKeyLoadError(false);
      setConnection(connectionsRef.current[previousProvider] ?? EMPTY_CONNECTION);
      void refreshApiKey(previousProvider);
      void refreshConnection(previousProvider);
      toast.error(t("settings.jevSaveFailed"));
    } finally {
      releaseConfigTransition();
    }
  }, [onSaved, refreshApiKey, refreshConnection, releaseConfigTransition, t]);

  const handleProviderChange = async (nextProvider: JevProvider) => {
    if (configSaving || nextProvider === provider || !reserveConfigTransition()) return;
    const previousProvider = provider;
    try {
      await flushSettings();
      keyRequestIdRef.current += 1;
      connectionRequestIdRef.current += 1;
      keyDraftRef.current = false;
      setKeyLoadError(false);
      setConnectionLoadError(false);
      providerEditedRef.current = true;
      providerRef.current = nextProvider;
      setProvider(nextProvider);
      setConnection(connectionsRef.current[nextProvider] ?? EMPTY_CONNECTION);
      setApiKey("");
      await persistConfig(nextProvider, previousProvider);
    } catch {
      releaseConfigTransition();
      toast.error(t("settings.jevSaveFailed"));
    }
  };

  const handleFeatureChange = async (feature: keyof JevFeatures) => {
    if (!reserveConfigTransition()) return;
    const previous = features;
    const next = { ...features, [feature]: !features[feature] };
    setFeatures(next);
    try {
      await setJevFeatures(next);
      onSaved();
    } catch {
      setFeatures(previous);
      toast.error(t("settings.jevSaveFailed"));
    } finally {
      releaseConfigTransition();
    }
  };

  const handleConnectionChange = (field: keyof JevConnection, value: string) => {
    if (configTransitionRef.current) return;
    const targetProvider = providerRef.current;
    const next = { ...(connectionsRef.current[targetProvider] ?? connection), [field]: value };
    const edits = { ...connectionEditsRef.current[targetProvider], [field]: value };
    connectionEditsRef.current[targetProvider] = edits;
    connectionsRef.current[targetProvider] = next;
    setConnection(next);
    if (loadedConnectionsRef.current.has(targetProvider)
      || (edits.base_url !== undefined && edits.model !== undefined)) {
      saveConnection.schedule(targetProvider, next);
    }
  };

  const reviewNow = async () => {
    if (reviewing) return;
    setReviewing(true);
    try {
      await flushSettings();
      const removed = await validateCorrections();
      toast.success(t("settings.jevReviewComplete", { count: removed }));
      onSaved();
    } catch {
      toast.error(t("settings.jevReviewFailed"));
    } finally {
      setReviewing(false);
    }
  };

  return (
    <div className="settings-column jev-settings" id="jev-settings" tabIndex={-1}>
      <div className="jev-section-heading">
        <h2 className="settings-section-title">{t("settings.jevTitle")}</h2>
        <span>{t("settings.jevOptional")}</span>
      </div>
      <p className="settings-hint settings-hint-flush">{t("settings.jevOverview")}</p>

      {FEATURES.map(([feature, label]) => (
        <div className="settings-column" key={feature}>
          <div className="settings-row" style={{ gap: 12 }}>
            <span className="permission-label" style={{ minWidth: 0 }}>{t(`settings.${label}`)}</span>
            <button type="button" role="switch" aria-checked={features[feature]}
              aria-label={t(`settings.${label}`)} aria-describedby={`jev-${feature}-hint`}
              className="toggle-switch" data-active={features[feature]} disabled={configSaving || !profile}
              onClick={() => { void handleFeatureChange(feature); }}>
              <div className="toggle-knob" />
            </button>
          </div>
          <span id={`jev-${feature}-hint`} className="settings-hint settings-hint-flush">{t(`settings.${label}Hint`)}</span>
          {feature === "correction_review" && <SettingsReveal open={features.correction_review} gap={10}>
            <button className="btn-ghost" type="button" disabled={reviewing || configSaving}
              onClick={() => { void reviewNow(); }}>
              {t(reviewing ? "settings.jevReviewing" : "settings.jevReviewNow")}
            </button>
          </SettingsReveal>}
        </div>
      ))}

      <SettingsReveal open={anyEnabled} gap={16}>
        <div className="settings-column decision-model-connection">
          <span className="settings-hint settings-hint-flush">{t("settings.jevSharedServiceHint")}</span>
          <div className="decision-model-fields">
            <label className="settings-column">
              <span className="settings-option-desc">{t("settings.jevModel")}</span>
              <input
                type="text"
                className="settings-input"
                aria-label={t("settings.jevModel")}
                autoComplete="off"
                spellCheck={false}
                value={connection.model}
                disabled={configSaving}
                onChange={(event) => handleConnectionChange("model", event.target.value)}
              />
            </label>
            <label className="settings-column">
              <span className="settings-option-desc">{t("settings.jevProvider")}</span>
              <select
                className="settings-input"
                aria-label={t("settings.jevProvider")}
                value={provider}
                disabled={configSaving}
                onChange={(event) => { void handleProviderChange(event.target.value as JevProvider); }}
              >
                {PROVIDERS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.value === "custom" ? t("settings.jevCustomProvider") : option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <fieldset
            disabled={configSaving}
            className="settings-column"
            style={{ border: 0, margin: 0, minWidth: 0, padding: 0 }}
          >
            <label className="settings-column" style={{ gap: 6 }}>
              <span className="settings-option-desc">{t("settings.jevApiUrl")}</span>
              <input type="url" className="settings-input" aria-label={t("settings.jevApiUrl")}
                aria-describedby="jev-connection-hint" autoComplete="off" spellCheck={false}
                value={connection.base_url}
                onChange={(event) => handleConnectionChange("base_url", event.target.value)} />
            </label>
            <span id="jev-connection-hint" className="settings-hint settings-hint-flush">
              {t("settings.jevConnectionHint")}
            </span>
            {connectionLoadError && (
              <span className="settings-error" role="alert">{t("settings.jevConnectionReadFailed")}</span>
            )}
            <div className="settings-column" style={{ gap: 6 }}>
              <span className="settings-option-desc">{t("settings.jevApiKey")}</span>
              <SecretInput
                key={provider}
                value={apiKey}
                placeholder={t("settings.jevApiKey")}
                ariaLabel={t("settings.jevApiKey")}
                onChange={(value) => {
                  if (configTransitionRef.current) return;
                  keyDraftRef.current = true;
                  apiKeyRef.current = value;
                  setKeyLoadError(false);
                  setApiKey(value);
                  saveKey.schedule(providerRef.current, value);
                }}
              />
              {keyLoadError && (
                <span className="settings-error" role="alert">
                  {t("settings.jevKeyReadFailed")}
                </span>
              )}
            </div>
          </fieldset>
          <ModelConnectionTest disabled={configSaving || keyLoadError || connectionLoadError}
            request={{ target: "decision", provider, baseUrl: connection.base_url, model: connection.model, apiKey }} />
        </div>
      </SettingsReveal>
    </div>
  );
}
