import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import Kbd from "@/components/ui/Kbd";
import IconButton from "@/components/ui/IconButton";

export interface OnboardingHintProps {
  hotkeyDisplay: string;
  mode: "hold" | "toggle";
  onDismiss: () => void;
}

export function OnboardingHint({ hotkeyDisplay, mode, onDismiss }: OnboardingHintProps) {
  const { t } = useTranslation();
  return (
    <div className="lw-onboarding" data-testid="main-onboarding">
      <div className="lw-onboarding-dismiss">
        <IconButton
          label={t("common.close")}
          icon={<X size={12} />}
          onClick={onDismiss}
          data-testid="main-onboarding-dismiss"
        />
      </div>
      <span className="lw-onboarding-title">{t("main.quickStart")}</span>
      <div className="lw-onboarding-body">
        <span>{t("main.pressHotkey")}</span>
        <Kbd combo={hotkeyDisplay} />
        <span>{mode === "toggle" ? t("main.hotkeyHintToggle") : t("main.hotkeyHintHold")}</span>
      </div>
      <div className="lw-field-hint">{t("main.autoInputHint")}</div>
      <div className="lw-field-hint">{t("main.settingsHint")}</div>
    </div>
  );
}

export default OnboardingHint;
