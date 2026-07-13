import { useEffect, useState } from "react";
import { Database } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";

import { setHistorySettings } from "@/api/tauri";
import type { HistorySettings, UserProfile } from "@/types";

interface HistorySettingsSectionProps {
  profile: UserProfile | null;
  onSaved: () => void;
}

const DEFAULT_SETTINGS: HistorySettings = {
  enabled: false,
  save_audio: false,
  retention_days: 90,
};

export default function HistorySettingsSection({ profile, onSaved }: HistorySettingsSectionProps) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<HistorySettings>(DEFAULT_SETTINGS);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setSettings(profile?.history_settings ?? DEFAULT_SETTINGS);
  }, [profile?.history_settings]);

  const persist = async (next: HistorySettings) => {
    const normalized = { ...next, save_audio: next.enabled ? next.save_audio : false };
    setSettings(normalized);
    setSaving(true);
    try {
      await setHistorySettings(normalized.enabled, normalized.save_audio, normalized.retention_days);
      onSaved();
    } catch {
      setSettings(profile?.history_settings ?? DEFAULT_SETTINGS);
      onSaved();
      toast.error(t("settings.historySaveFailed"));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section className="settings-card" data-nav-id="history-settings">
      <div className="settings-section-header">
        <Database size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.historySettings")}</h2>
      </div>
      <div className="settings-column history-settings-column">
        <p className="settings-hint settings-hint-flush">{t("settings.historySettingsHint")}</p>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.historyEnabled")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.historyEnabledHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.enabled}
            aria-label={t("settings.historyEnabled")}
            className="toggle-switch"
            data-active={settings.enabled}
            disabled={saving}
            onClick={() => { void persist({ ...settings, enabled: !settings.enabled }); }}
          >
            <div className="toggle-knob" />
          </button>
        </div>

        <div className="settings-row">
          <div className="settings-column history-setting-copy">
            <span className="permission-label">{t("settings.historySaveAudio")}</span>
            <span className="settings-hint settings-hint-flush">{t("settings.historySaveAudioHint")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={settings.save_audio}
            aria-label={t("settings.historySaveAudio")}
            className="toggle-switch"
            data-active={settings.save_audio}
            disabled={!settings.enabled || saving}
            onClick={() => { void persist({ ...settings, save_audio: !settings.save_audio }); }}
          >
            <div className="toggle-knob" />
          </button>
        </div>

        <label className="settings-column history-retention-field">
          <span className="permission-label">{t("settings.historyRetention")}</span>
          <select
            className="settings-input"
            value={settings.retention_days}
            disabled={!settings.enabled || saving}
            onChange={(event) => {
              void persist({ ...settings, retention_days: Number(event.target.value) });
            }}
          >
            <option value={30}>{t("settings.historyRetention30")}</option>
            <option value={90}>{t("settings.historyRetention90")}</option>
            <option value={365}>{t("settings.historyRetention365")}</option>
            <option value={0}>{t("settings.historyRetentionForever")}</option>
          </select>
        </label>
      </div>
    </section>
  );
}
