import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { ContextMode } from "@/types";

interface Props {
  label: string;
  value: ContextMode;
  hint: string;
  disabled?: boolean;
  onChange: (mode: ContextMode) => void;
  onConfigure: () => void;
}

export default function ProcessingModeControl({ label, value, hint, disabled, onChange, onConfigure }: Props) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <div className="processing-mode">
      <div className="processing-mode-heading">
        <span className="permission-label" id={`${id}-label`}>{label}</span>
        <div className="processing-mode-options" role="group" aria-labelledby={`${id}-label`}>
          {(["off", "on", "auto"] as const).map((mode) => (
            <button key={mode} type="button" aria-pressed={value === mode}
              disabled={disabled} onClick={() => onChange(mode)}>
              {t(`settings.processingMode${mode === "off" ? "Off" : mode === "on" ? "On" : "Auto"}`)}
            </button>
          ))}
        </div>
      </div>
      <p className="settings-hint settings-hint-flush">{hint}</p>
      {value === "auto" && (
        <div className="processing-mode-auto">
          <span>{t("settings.jevAutoHint")}</span>
          <button type="button" onClick={onConfigure}>{t("settings.jevConfigure")}</button>
        </div>
      )}
    </div>
  );
}
