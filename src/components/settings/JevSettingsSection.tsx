import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { getJevApiKey, setJevApiKey, setJevConfig } from "@/api/tauri";
import SecretInput from "@/components/SecretInput";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import type { JevProvider, UserProfile } from "@/types";

const PROVIDERS: ReadonlyArray<{ value: JevProvider; label: string }> = [
  { value: "typesafe", label: "TypeSafe (official)" },
  { value: "openrouter", label: "OpenRouter" },
  { value: "vercel", label: "Vercel" },
];

interface JevSettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

export default function JevSettingsSection({ profile, onSaved }: JevSettingsSectionProps) {
  const { t } = useTranslation();
  const savedEnabled = Boolean(profile?.jev?.enabled);
  const savedProvider = profile?.jev?.provider ?? "typesafe";
  const [enabled, setEnabled] = useState(savedEnabled);
  const [provider, setProvider] = useState<JevProvider>(savedProvider);
  const [apiKey, setApiKey] = useState("");
  const [configSaving, setConfigSaving] = useState(false);
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
    setEnabled(savedEnabled);
    setProvider(savedProvider);
    providerRef.current = savedProvider;
  }, [savedEnabled, savedProvider]);

  useEffect(() => {
    if (!enabled) {
      keyRequestIdRef.current += 1;
      setApiKey("");
      return;
    }
    void refreshApiKey(provider);
  }, [enabled, provider, refreshApiKey]);

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
    nextEnabled: boolean,
    nextProvider: JevProvider,
    previousEnabled: boolean,
    previousProvider: JevProvider,
  ) => {
    try {
      await setJevConfig(nextEnabled, nextProvider);
      onSaved();
    } catch {
      setEnabled(previousEnabled);
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

  const handleEnabledChange = () => {
    if (configSaving || !reserveConfigTransition()) return;
    const nextEnabled = !enabled;
    const previousEnabled = enabled;
    if (!nextEnabled) {
      keyRequestIdRef.current += 1;
      keyDraftRef.current = false;
      setKeyLoadError(false);
    }
    setEnabled(nextEnabled);
    void (async () => {
      try {
        if (!nextEnabled) await saveKey.flush();
        await persistConfig(nextEnabled, provider, previousEnabled, provider);
      } catch {
        releaseConfigTransition();
        toast.error(t("settings.jevSaveFailed"));
      }
    })();
  };

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
      await persistConfig(enabled, nextProvider, enabled, previousProvider);
    } catch {
      releaseConfigTransition();
      toast.error(t("settings.jevSaveFailed"));
    }
  };

  return (
    <div className="settings-column jev-settings">
      <div className="settings-row" style={{ gap: 12 }}>
        <span className="permission-label" style={{ minWidth: 0 }}>{t("settings.jev")}</span>
        <button
          type="button"
          role="switch"
          aria-checked={enabled}
          aria-label={t("settings.jevEnabled")}
          className="toggle-switch"
          data-active={enabled}
          disabled={configSaving}
          onClick={handleEnabledChange}
        >
          <div className="toggle-knob" />
        </button>
      </div>
      <span className="settings-hint settings-hint-flush">
        {t("settings.jevHint")}
      </span>

      {enabled && (
        <div className="settings-column">
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
                inputStyle={{ padding: "10px 36px 10px 12px" }}
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
