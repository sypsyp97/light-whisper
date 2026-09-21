import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { getJevApiKey, setJevApiKey, setJevProvider, setJevFeatures, validateCorrections } from "@/api/tauri";
import SecretInput from "@/components/SecretInput";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import type { JevFeatures, JevProvider, UserProfile } from "@/types";

const PROVIDERS: ReadonlyArray<{ value: JevProvider; label: string }> = [
  { value: "typesafe", label: "TypeSafe (official)" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "vercel", label: "Vercel" },
];

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
  const anyEnabled = (enabled && polishEnabled) || Object.values(features).some(Boolean)
    || Boolean(profile?.jev?.screen_routing && (profile.ai_polish_screen_context_enabled || profile.assistant_screen_context_enabled))
    || Boolean(profile?.jev?.search_routing && profile.web_search?.enabled);
  const [apiKey, setApiKey] = useState("");
  const [configSaving, setConfigSaving] = useState(false);
  const [reviewing, setReviewing] = useState(false);
  const [keyLoadError, setKeyLoadError] = useState(false);
  const providerRef = useRef<JevProvider>(provider);
  const keyRequestIdRef = useRef(0);
  const keyDraftRef = useRef(false);
  const configTransitionRef = useRef(false);
  providerRef.current = provider;

  const saveKey = useDebouncedCallback(async (targetProvider: JevProvider, value: string) => {
    try {
      await setJevApiKey(targetProvider, value);
    } catch {
      toast.error(t("settings.jevSaveFailed"));
    }
  }, 600, { onUnmount: "flush" });

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

  useEffect(() => {
    setFeatures({ correction_review: correctionReview, polish_audit: polishAudit });
  }, [correctionReview, polishAudit]);

  useEffect(() => {
    setProvider(savedProvider);
    providerRef.current = savedProvider;
  }, [savedProvider]);

  useEffect(() => {
    if (!anyEnabled) {
      keyRequestIdRef.current += 1;
      keyDraftRef.current = false;
      setKeyLoadError(false);
      setApiKey("");
      void saveKey.flush();
      return;
    }
    let active = true;
    void saveKey.flush().then(() => { if (active) void refreshApiKey(provider); });
    return () => { active = false; };
  }, [anyEnabled, provider, refreshApiKey, saveKey]);

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
      keyDraftRef.current = false;
      setApiKey("");
      setKeyLoadError(false);
      toast.error(t("settings.jevSaveFailed"));
    } finally {
      releaseConfigTransition();
    }
  }, [onSaved, releaseConfigTransition, t]);

  const handleProviderChange = async (nextProvider: JevProvider) => {
    if (configSaving || nextProvider === provider || !reserveConfigTransition()) return;
    const previousProvider = provider;
    keyRequestIdRef.current += 1;
    keyDraftRef.current = false;
    setKeyLoadError(false);
    try {
      await saveKey.flush();
      providerRef.current = nextProvider;
      setProvider(nextProvider);
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

  const reviewNow = async () => {
    if (reviewing) return;
    setReviewing(true);
    try {
      await saveKey.flush();
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
          {feature === "correction_review" && features.correction_review && (
            <button className="btn-ghost" type="button" disabled={reviewing || configSaving}
              onClick={() => { void reviewNow(); }}>
              {t(reviewing ? "settings.jevReviewing" : "settings.jevReviewNow")}
            </button>
          )}
        </div>
      ))}

      {anyEnabled && (
        <div className="settings-column">
          <span className="settings-hint settings-hint-flush">{t("settings.jevSharedServiceHint")}</span>
          <label className="settings-column" style={{ gap: 6 }}>
            <span className="settings-option-desc">{t("settings.jevProvider")}</span>
            <select
              className="settings-input"
              aria-label={t("settings.jevProvider")}
              value={provider}
              disabled={configSaving}
              onChange={(event) => { void handleProviderChange(event.target.value as JevProvider); }}
            >
              {PROVIDERS.map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </label>

          <fieldset
            disabled={configSaving}
            style={{ border: 0, margin: 0, minWidth: 0, padding: 0 }}
          >
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
        </div>
      )}
    </div>
  );
}
