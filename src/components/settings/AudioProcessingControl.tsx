import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getAudioProcessingEnabled, setAudioProcessingEnabled } from "@/api/tauri";

export default function AudioProcessingControl() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [saveError, setSaveError] = useState("");
  const [readVersion, setReadVersion] = useState(0);
  const mounted = useRef(false);
  const pending = useRef(false);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    setLoading(true);
    setLoadError(false);
    getAudioProcessingEnabled().then((value) => {
      if (!cancelled) setEnabled(value);
    }).catch(() => {
      if (!cancelled) setLoadError(true);
    }).finally(() => {
      if (!cancelled) setLoading(false);
    });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [readVersion]);

  const toggle = async () => {
    if (pending.current || loading || loadError) return;
    pending.current = true;
    setSaving(true);
    setSaveError("");
    try {
      const saved = await setAudioProcessingEnabled(!enabled);
      if (mounted.current) setEnabled(saved);
    } catch (error) {
      if (mounted.current) {
        toast.error(t("settings.audioProcessingSaveFailed"));
        setSaveError(error instanceof Error ? error.message : t("settings.audioProcessingSaveFailed"));
      }
    } finally {
      pending.current = false;
      if (mounted.current) setSaving(false);
    }
  };

  return (
    <div className="settings-inline-panel" style={{ marginTop: 8 }}>
      <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
        <div className="settings-column" style={{ gap: 2, flex: 1 }}>
          <span className="permission-label">{t("settings.audioProcessingTitle")}</span>
          <span className="settings-hint" style={{ margin: 0 }}>{t("settings.audioProcessingDesc")}</span>
        </div>
        <button
          type="button"
          role="switch"
          aria-label={t("settings.audioProcessingTitle")}
          aria-checked={enabled}
          aria-busy={saving || loading}
          disabled={saving || loading || loadError}
          className="toggle-switch"
          style={{ flexShrink: 0 }}
          onClick={() => { void toggle(); }}
        >
          <div className="toggle-knob" />
        </button>
      </div>
      {saveError && <span className="settings-hint" role="alert">{saveError}</span>}
      {loadError && (
        <div className="settings-row" role="alert" style={{ gap: 8 }}>
          <span className="settings-hint">{t("settings.audioProcessingLoadFailed")}</span>
          <button type="button" className="btn btn-ghost" onClick={() => setReadVersion((value) => value + 1)}>
            {t("common.retry")}
          </button>
        </div>
      )}
    </div>
  );
}
