import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { setRecordingMode } from "@/api/tauri";
import { DEFAULT_HOTKEY, RECORDING_MODE_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";
import Kbd from "@/components/ui/Kbd";
import Segmented from "@/components/ui/Segmented";
import Banner from "@/components/ui/Banner";

export function HotkeySection() {
  const { t } = useTranslation();
  const { hotkeyDisplay, setHotkey, hotkeyError, hotkeyDiagnostic } = useRecordingContext();
  const [recordingMode, setRecordingModeState] = useState<"hold" | "toggle">(
    () => readLocalStorage(RECORDING_MODE_KEY) === "toggle" ? "toggle" : "hold",
  );

  const capture = useHotkeyCapture({
    save: async (shortcut) => { await setHotkey(shortcut); },
    label: t("settings.hotkeyLabel"),
  });

  const handleReset = useCallback(async () => {
    try {
      await setHotkey(DEFAULT_HOTKEY);
      toast.success(t("toast.hotkeyReset"));
    } catch {
      toast.error(t("toast.hotkeyResetFailed"));
    }
  }, [setHotkey, t]);

  const handleModeChange = useCallback(async (mode: "hold" | "toggle") => {
    const prev = recordingMode;
    setRecordingModeState(mode);
    writeLocalStorage(RECORDING_MODE_KEY, mode);
    try { await setRecordingMode(mode === "toggle"); }
    catch { setRecordingModeState(prev); }
  }, [recordingMode]);

  const diagnosticText = hotkeyDiagnostic?.systemConflict || hotkeyDiagnostic?.warning;

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-hotkey"
      data-nav-id="hotkey"
    >
      <h2 className="lw-settings-section-title">{t("settings.hotkeySection")}</h2>
      <Field label={t("settings.hotkeyLabel")} hint={t("settings.hotkeyHint")}>
        <div className="lw-inline">
          <Button
            variant="secondary"
            onClick={capture.startCapture}
            loading={capture.saving}
            data-testid="hotkey-capture-btn"
          >
            {capture.capturing ? t("settings.pressCombo") : <Kbd combo={hotkeyDisplay} />}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => void handleReset()} data-testid="hotkey-reset-btn">
            {t("settings.resetF2")}
          </Button>
        </div>
      </Field>
      {diagnosticText && (
        <Banner tone="warn" message={diagnosticText} data-testid="hotkey-diagnostic" />
      )}
      {hotkeyError && (
        <Banner tone="error" message={hotkeyError} data-testid="hotkey-error-banner" />
      )}
      <Field label={t("settings.recordingMode")}>
        <Segmented
          value={recordingMode}
          options={[
            { value: "hold", label: t("settings.holdToTalk") },
            { value: "toggle", label: t("settings.toggleMode") },
          ]}
          onChange={(v) => void handleModeChange(v)}
          data-testid="recording-mode-segmented"
        />
      </Field>
    </section>
  );
}

export default HotkeySection;
