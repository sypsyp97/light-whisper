import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  checkPermission,
  openPermissionSettings,
  pasteText,
  requestPermission,
  type PermissionKind,
  type PermissionStatus,
} from "@/api/tauri";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";
import Badge from "@/components/ui/Badge";

interface PermissionMeta {
  kind: PermissionKind;
  labelKey: string;
  descKey: string;
  fallbackLabel: string;
  fallbackDesc: string;
}

const PERMISSIONS: readonly PermissionMeta[] = [
  {
    kind: "microphone",
    labelKey: "settings.permMicrophone",
    descKey: "settings.permMicrophoneDesc",
    fallbackLabel: "Microphone",
    fallbackDesc: "Used while recording.",
  },
  {
    kind: "accessibility",
    labelKey: "settings.permAccessibility",
    descKey: "settings.permAccessibilityDesc",
    fallbackLabel: "Accessibility",
    fallbackDesc: "Pastes the transcript into the focused app.",
  },
  {
    kind: "screen",
    labelKey: "settings.permScreen",
    descKey: "settings.permScreenDesc",
    fallbackLabel: "Screen Recording",
    fallbackDesc: "Used by the screen-aware assistant when enabled.",
  },
  {
    kind: "automation",
    labelKey: "settings.permAutomation",
    descKey: "settings.permAutomationDesc",
    fallbackLabel: "Automation",
    fallbackDesc: "Pastes via System Events.",
  },
] as const;

type StatusMap = Record<PermissionKind, PermissionStatus>;

const INITIAL_STATUSES: StatusMap = {
  microphone: { granted: false, canRequest: true },
  accessibility: { granted: false, canRequest: true },
  screen: { granted: false, canRequest: true },
  automation: { granted: false, canRequest: true },
};

export function PermissionsSection() {
  const { t } = useTranslation();
  const [statuses, setStatuses] = useState<StatusMap>(INITIAL_STATUSES);

  const refresh = useCallback(async (kind: PermissionKind) => {
    try {
      const status = await checkPermission(kind);
      setStatuses((s) => ({ ...s, [kind]: status }));
    } catch {
      /* check_permission never throws structured errors today; ignore so we
         don't crash the section when one row fails to probe. */
    }
  }, []);

  const refreshAll = useCallback(async () => {
    await Promise.all(PERMISSIONS.map(({ kind }) => refresh(kind)));
  }, [refresh]);

  useEffect(() => {
    void refreshAll();
    // Re-probe whenever the window regains focus — the user has likely just
    // returned from System Settings, so we want the badges to update without
    // a manual click.
    const onFocus = () => void refreshAll();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refreshAll]);

  const handleRequest = useCallback(
    async (kind: PermissionKind) => {
      try {
        const status = await requestPermission(kind);
        setStatuses((s) => ({ ...s, [kind]: status }));
      } catch {
        /* swallow — re-checking on focus will reconcile state. */
      }
    },
    [],
  );

  const handleOpenSettings = useCallback(async (kind: PermissionKind) => {
    try {
      await openPermissionSettings(kind);
    } catch {
      /* If `open` failed (extremely rare), the user can still navigate
         manually — no need to nag them with a toast. */
    }
  }, []);

  const handlePasteTest = useCallback(async () => {
    try {
      await pasteText("ok", "clipboard");
      toast.success(t("toast.pasteOk"));
    } catch {
      toast.error(t("toast.pasteFailed"));
    }
  }, [t]);

  const translate = (key: string, fallback: string): string => {
    const v = t(key, { defaultValue: "" });
    return v || fallback;
  };

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-permissions"
      data-nav-id="permissions"
    >
      <div className="lw-settings-section-header">
        <h2 className="lw-settings-section-title">{t("settings.permissions")}</h2>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void refreshAll()}
          data-testid="perm-recheck-all-btn"
        >
          {translate("settings.permRecheckAll", "Re-check all")}
        </Button>
      </div>
      {PERMISSIONS.map(({ kind, labelKey, descKey, fallbackLabel, fallbackDesc }) => {
        const status = statuses[kind];
        const grantedTone = status.granted ? "success" : "warn";
        return (
          <div
            key={kind}
            className="lw-settings-row"
            data-testid={`perm-row-${kind}`}
          >
            <div className="lw-settings-row-label">
              <span className="lw-settings-row-main">
                {translate(labelKey, fallbackLabel)}
              </span>
              <span className="lw-settings-row-desc">
                {translate(descKey, fallbackDesc)}
              </span>
            </div>
            <div className="lw-settings-row-control">
              <Badge tone={grantedTone} data-testid={`perm-status-${kind}`}>
                {status.granted
                  ? translate("settings.permGranted", "Granted")
                  : translate("settings.permDenied", "Needs attention")}
              </Badge>
              {status.granted ? (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void refresh(kind)}
                  data-testid={`perm-recheck-${kind}`}
                  aria-label={translate("settings.permRecheck", "Re-check")}
                >
                  {translate("settings.permRecheck", "Re-check")}
                </Button>
              ) : (
                <>
                  {status.canRequest && (
                    <Button
                      size="sm"
                      onClick={() => void handleRequest(kind)}
                      data-testid={`perm-request-${kind}`}
                    >
                      {translate("settings.permRequest", "Request")}
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="primary"
                    onClick={() => void handleOpenSettings(kind)}
                    data-testid={`perm-open-settings-${kind}`}
                  >
                    {translate("settings.permOpenSettings", "Open Settings")}
                  </Button>
                </>
              )}
            </div>
          </div>
        );
      })}
      <Field label={t("settings.accessibilityPaste")}>
        <Button onClick={() => void handlePasteTest()} data-testid="perm-paste-test-btn">
          {t("settings.testPasteContent")}
        </Button>
      </Field>
    </section>
  );
}

export default PermissionsSection;
