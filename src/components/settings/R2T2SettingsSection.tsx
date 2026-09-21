import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { setR2T2Config } from "@/api/tauri";
import type { UserProfile } from "@/types";

// Language codes supported by the pinned native R2T2 model spec.
const LANGUAGES = [
  "zh", "en", "yue", "ar", "de", "fr", "es", "pt", "id", "it", "ko", "ru",
  "th", "vi", "ja", "tr", "hi", "ms", "nl", "sv", "da", "fi", "pl", "cs",
  "fil", "fa", "el", "hu", "mk", "ro",
];

export default function R2T2SettingsSection({ profile, onSaved }: {
  profile: UserProfile;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation();
  const savedContext = profile.r2t2?.context ?? "";
  const savedLanguage = profile.r2t2?.language ?? "";
  const [context, setContext] = useState(savedContext);
  const [language, setLanguage] = useState(savedLanguage);
  const [saving, setSaving] = useState(false);
  const pending = useRef(false);
  useEffect(() => { setContext(savedContext); setLanguage(savedLanguage); }, [savedContext, savedLanguage]);
  const names = new Intl.DisplayNames([i18n.language], { type: "language" });
  const dirty = context !== savedContext || language !== savedLanguage;

  const save = async () => {
    if (pending.current || !dirty) return;
    pending.current = true;
    setSaving(true);
    try {
      await setR2T2Config(context, language || null);
      toast.success(t("settings.r2t2Saved"));
      onSaved();
    } catch {
      toast.error(t("settings.r2t2SaveFailed"));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  };

  return (
    <div className="settings-inline-panel">
      <label className="settings-column" style={{ gap: 6 }}>
        <span className="settings-option-desc">{t("settings.r2t2Language")}</span>
        <select className="settings-input" value={language} disabled={saving}
          onChange={(event) => setLanguage(event.target.value)}>
          <option value="">{t("settings.r2t2AutoLanguage")}</option>
          {LANGUAGES.map((code) => <option key={code} value={code}>{names.of(code) ?? code}</option>)}
        </select>
      </label>
      <label className="settings-column" style={{ gap: 6 }}>
        <span className="settings-option-desc">{t("settings.r2t2Context")}</span>
        <textarea className="settings-input" rows={3} style={{ resize: "vertical" }}
          value={context} disabled={saving} placeholder={t("settings.r2t2ContextPlaceholder")}
          onChange={(event) => setContext(event.target.value)} />
      </label>
      <p className="settings-hint settings-hint-flush">{t("settings.r2t2ContextHint")}</p>
      <div className="settings-row">
        <span className="settings-option-desc">{t("settings.r2t2ApplyHint")}</span>
        <button type="button" className="test-btn" disabled={saving || !dirty} onClick={() => { void save(); }}>
          {saving ? t("common.loading") : t("settings.r2t2Save")}
        </button>
      </div>
    </div>
  );
}
