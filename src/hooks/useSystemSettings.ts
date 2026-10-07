import { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  checkAppUpdate,
  copyToClipboard,
  disableAutostart,
  enableAutostart,
  exportUserProfile,
  importUserProfile,
  isAutostartEnabled,
  openAppReleasePage,
  pasteText,
  setAutostartMinimized,
} from "@/api/tauri";
import type { UserProfile } from "@/types";

interface UseSystemSettingsOptions {
  inputMethod: "sendInput" | "clipboard";
  profile: UserProfile | null;
  refreshProfile: () => Promise<UserProfile | undefined>;
  refreshAiPolishKey: () => Promise<unknown>;
}

export function useSystemSettings({
  inputMethod,
  profile,
  refreshProfile,
  refreshAiPolishKey,
}: UseSystemSettingsOptions) {
  const { t } = useTranslation();
  const [autostart, setAutostart] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [autostartMinimizedSaving, setAutostartMinimizedSaving] = useState(false);
  const autostartMinimized = profile?.autostart_minimized ?? false;
  const [appVersion, setAppVersion] = useState("");
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateStatusText, setUpdateStatusText] = useState("");
  const [latestAvailableVersion, setLatestAvailableVersion] = useState<string | null>(null);
  const [latestReleaseUrl, setLatestReleaseUrl] = useState<string | null>(null);
  const [lastExportPath, setLastExportPath] = useState("");
  const [exporting, setExporting] = useState(false);
  const exportPending = useRef(false);

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => {});
  }, []);

  useEffect(() => {
    void isAutostartEnabled().then((enabled) => {
      setAutostart(enabled);
      setAutostartLoading(false);
    }).catch(() => setAutostartLoading(false));
  }, []);

  const handleCheckForUpdates = useCallback(async () => {
    if (updateChecking) return;

    setUpdateChecking(true);
    setLatestAvailableVersion(null);
    setLatestReleaseUrl(null);
    setUpdateStatusText(t("toast.checkingGitHub"));

    try {
      const updateInfo = await checkAppUpdate();
      setLatestReleaseUrl(updateInfo.releaseUrl ?? null);
      if (!updateInfo.available || !updateInfo.latestVersion) {
        setUpdateStatusText(t("toast.alreadyLatest"));
        toast.success(t("toast.alreadyLatest"));
        return;
      }

      setLatestAvailableVersion(updateInfo.latestVersion);
      setUpdateStatusText(t("toast.newVersionFound", { version: updateInfo.latestVersion }));
      toast.info(t("toast.newVersionToast", { version: updateInfo.latestVersion }));
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toast.checkUpdateFailed");
      setUpdateStatusText(message);
      toast.error(message);
    } finally {
      setUpdateChecking(false);
    }
  }, [t, updateChecking]);

  const handleOpenReleasePage = useCallback(async () => {
    try {
      const message = await openAppReleasePage(latestReleaseUrl);
      toast.success(message);
    } catch (error) {
      const message = error instanceof Error ? error.message : t("toast.openReleaseFailed");
      setUpdateStatusText(message);
      toast.error(message);
    }
  }, [latestReleaseUrl, t]);

  const handleAutostartToggle = useCallback(async () => {
    if (autostartLoading) return;
    const prev = autostart;
    const next = !prev;
    // Reflect the requested state immediately, then reconcile it with the
    // plugin's authoritative value before reporting success.
    setAutostart(next);
    setAutostartLoading(true);
    try {
      if (prev) {
        await disableAutostart();
      } else {
        await enableAutostart();
      }
      const confirmed = await isAutostartEnabled();
      setAutostart(confirmed);
      if (confirmed !== next) {
        throw new Error("Autostart state was not persisted");
      }
      toast.success(t(next ? "toast.autostartEnabled" : "toast.autostartDisabled"), {
        duration: 1100,
      });
    } catch {
      // A failed operation or unreadable confirmation leaves no authoritative
      // new value, so return to the last confirmed UI state.
      setAutostart(prev);
      toast.error(t("toast.autostartFailed"));
    } finally {
      setAutostartLoading(false);
    }
  }, [autostart, autostartLoading, t]);

  const handleAutostartMinimizedToggle = useCallback(async () => {
    if (!profile || !autostart || autostartLoading || autostartMinimizedSaving) return;
    const next = !autostartMinimized;
    setAutostartMinimizedSaving(true);
    try {
      await setAutostartMinimized(next);
      const confirmed = await refreshProfile();
      if (confirmed?.autostart_minimized !== next) {
        throw new Error("Startup preference could not be confirmed");
      }
    } catch {
      toast.error(t("toast.autostartMinimizedFailed"));
    } finally {
      setAutostartMinimizedSaving(false);
    }
  }, [profile, autostart, autostartLoading, autostartMinimized, autostartMinimizedSaving, refreshProfile, t]);

  const handleExportConfig = useCallback(async () => {
    if (exportPending.current) return;
    exportPending.current = true;
    setExporting(true);
    try {
      const path = await exportUserProfile();
      if (!path) return;
      setLastExportPath(path);
      toast.success(t("toast.configExported"));
    } catch {
      toast.error(t("toast.configExportFailed"));
    } finally {
      exportPending.current = false;
      setExporting(false);
    }
  }, [t]);

  const handleCopyExportPath = useCallback(async () => {
    if (!lastExportPath) return;
    try {
      await copyToClipboard(lastExportPath);
      toast.success(t("common.copiedToClipboard"));
    } catch {
      toast.error(t("common.copyFailed"));
    }
  }, [lastExportPath, t]);

  const handleImportConfig = useCallback(async (json: string) => {
    try {
      await importUserProfile(json);
      await refreshProfile();
      await refreshAiPolishKey();
      toast.success(t("toast.configImported"));
    } catch {
      toast.error(t("toast.configImportFailed"));
    }
  }, [refreshAiPolishKey, refreshProfile, t]);

  const handleTestPaste = useCallback(async () => {
    try {
      await pasteText(t("settings.testPasteContent"), inputMethod);
      toast.success(t("toast.pasteOk"));
    } catch {
      toast.error(t("toast.pasteFailed"));
    }
  }, [inputMethod, t]);

  return {
    appVersion,
    exporting,
    autostart,
    autostartLoading,
    autostartMinimized,
    autostartMinimizedLoading: autostartLoading || autostartMinimizedSaving || !profile,
    handleAutostartToggle,
    handleAutostartMinimizedToggle,
    handleCheckForUpdates,
    handleCopyExportPath,
    handleExportConfig,
    handleImportConfig,
    handleOpenReleasePage,
    handleTestPaste,
    lastExportPath,
    latestAvailableVersion,
    updateChecking,
    updateStatusText,
  };
}
