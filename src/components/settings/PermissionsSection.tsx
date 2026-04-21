import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import {
  checkPermission,
  requestPermission,
  pasteText,
  type PermissionKind,
  type PermissionStatus,
} from "@/api/tauri";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";
import Badge from "@/components/ui/Badge";

const PERMISSIONS: Array<{ kind: PermissionKind; labelKey: string; fallback: string }> = [
  { kind: "microphone", labelKey: "settings.permMicrophone", fallback: "Microphone" },
  { kind: "accessibility", labelKey: "settings.permAccessibility", fallback: "Accessibility" },
  { kind: "screen", labelKey: "settings.permScreen", fallback: "Screen recording" },
  { kind: "automation", labelKey: "settings.permAutomation", fallback: "Automation" },
];

export function PermissionsSection() {
  const { t } = useTranslation();
  const [statuses, setStatuses] = useState<Record<PermissionKind, PermissionStatus>>({
    microphone: { granted: false, canRequest: true },
    accessibility: { granted: false, canRequest: true },
    screen: { granted: false, canRequest: true },
    automation: { granted: false, canRequest: true },
  });

  const refresh = useCallback(async (kind: PermissionKind) => {
    try {
      const status = await checkPermission(kind);
      setStatuses((s) => ({ ...s, [kind]: status }));
    } catch { /* */ }
  }, []);

  useEffect(() => {
    for (const { kind } of PERMISSIONS) void refresh(kind);
  }, [refresh]);

  const handleRequest = useCallback(async (kind: PermissionKind) => {
    try {
      const status = await requestPermission(kind);
      setStatuses((s) => ({ ...s, [kind]: status }));
    } catch { /* */ }
  }, []);

  const handlePasteTest = useCallback(async () => {
    try {
      await pasteText("ok", "clipboard");
      toast.success(t("toast.pasteOk"));
    } catch {
      toast.error(t("toast.pasteFailed"));
    }
  }, [t]);

  const translate = (k: string, fallback: string): string => {
    const v = t(k, { defaultValue: "" });
    return v || fallback;
  };

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-permissions"
      data-nav-id="permissions"
    >
      <h2 className="lw-settings-section-title">{t("settings.permissions")}</h2>
      {PERMISSIONS.map(({ kind, labelKey, fallback }) => {
        const status = statuses[kind];
        return (
          <div
            key={kind}
            className="lw-settings-row"
            data-testid={`perm-row-${kind}`}
          >
            <div className="lw-settings-row-label">
              <span className="lw-settings-row-main">{translate(labelKey, fallback)}</span>
            </div>
            <div className="lw-settings-row-control">
              <Badge
                tone={status.granted ? "success" : "warn"}
                data-testid={`perm-status-${kind}`}
              >
                {status.granted ? "Granted" : "Denied"}
              </Badge>
              <Button
                size="sm"
                onClick={() => void handleRequest(kind)}
                data-testid={`perm-request-${kind}`}
              >
                {t("common.settings")}
              </Button>
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
