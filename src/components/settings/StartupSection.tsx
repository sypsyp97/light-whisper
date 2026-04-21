import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  isAutostartEnabled,
  enableAutostart,
  disableAutostart,
} from "@/api/tauri";
import Field from "@/components/ui/Field";
import Toggle from "@/components/ui/Toggle";

export function StartupSection() {
  const { t } = useTranslation();
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    void isAutostartEnabled().then(setEnabled).catch(() => {});
  }, []);

  const handleToggle = useCallback(async (next: boolean) => {
    const prev = enabled;
    setEnabled(next);
    try {
      if (next) { await enableAutostart(); toast.success(t("toast.autostartEnabled")); }
      else { await disableAutostart(); toast.success(t("toast.autostartDisabled")); }
    } catch {
      setEnabled(prev);
      toast.error(t("toast.autostartFailed"));
    }
  }, [enabled, t]);

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-startup"
      data-nav-id="startup"
    >
      <h2 className="lw-settings-section-title">{t("settings.startup")}</h2>
      <Field label={t("settings.autostart")}>
        <Toggle
          checked={enabled}
          onChange={(v) => void handleToggle(v)}
          label={t("settings.autostart")}
          data-testid="autostart-toggle"
        />
      </Field>
    </section>
  );
}

export default StartupSection;
