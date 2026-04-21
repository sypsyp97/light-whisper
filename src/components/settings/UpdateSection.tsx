import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { checkAppUpdate, openAppReleasePage } from "@/api/tauri";
import type { AppUpdateInfo } from "@/types";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";
import Badge from "@/components/ui/Badge";
import Banner from "@/components/ui/Banner";

export default function UpdateSection() {
  const { t } = useTranslation();
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<AppUpdateInfo | null>(null);

  useEffect(() => {
    void getVersion().then(setVersion).catch(() => {});
  }, []);

  const handleCheck = useCallback(async () => {
    setChecking(true);
    try {
      const res = await checkAppUpdate();
      setResult(res);
      if (!res.available) toast.success(t("toast.alreadyLatest"));
    } catch {
      toast.error(t("toast.checkUpdateFailed"));
    } finally {
      setChecking(false);
    }
  }, [t]);

  const openRelease = useCallback(async () => {
    try { await openAppReleasePage(result?.releaseUrl); }
    catch { toast.error(t("toast.openReleaseFailed")); }
  }, [result, t]);

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-update"
      data-nav-id="update"
    >
      <h2 className="lw-settings-section-title">{t("settings.update")}</h2>
      <Field label={t("settings.currentVersion", { version })}>
        <div className="lw-inline">
          <Badge tone="neutral" data-testid="update-current-version">v{version}</Badge>
          <Button
            onClick={() => void handleCheck()}
            loading={checking}
            data-testid="update-check-btn"
          >
            {t("settings.checkUpdate")}
          </Button>
        </div>
      </Field>
      {result?.available && result.latestVersion && (
        <Banner
          tone="info"
          message={t("settings.newVersionAvailable", { version: result.latestVersion })}
          action={{ label: t("settings.goToDownload"), onClick: () => void openRelease() }}
        />
      )}
    </section>
  );
}
