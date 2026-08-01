import { Languages } from "lucide-react";
import { useTranslation } from "react-i18next";
import Kbd from "@/components/Kbd";
import TranslationLanguagePicker from "@/components/settings/TranslationLanguagePicker";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";

interface HotkeyCaptureController {
  capturing: boolean;
  saving: boolean;
  startCapture: () => void;
  cancelCapture: () => void;
}

interface TranslationSettingsSectionProps {
  target: string | null;
  hotkeyDisplay: string;
  hotkeyCapture: HotkeyCaptureController;
  onClearHotkey: () => void;
  onSelectTarget: (target: string | null) => Promise<void>;
}

export default function TranslationSettingsSection({
  target,
  hotkeyDisplay,
  hotkeyCapture,
  onClearHotkey,
  onSelectTarget,
}: TranslationSettingsSectionProps) {
  const { t } = useTranslation();
  const picker = useExclusivePicker<"translationLanguage">();

  return (
    <section className="settings-card" data-nav-id="translation">
      <div className="settings-section-header">
        <Languages size={15} className="icon-accent" />
        <h2 className="settings-section-title">{t("settings.translation")}</h2>
      </div>
      <div className="settings-column translation-settings">
        <div className="settings-row translation-hotkey-row">
          <button
            className="theme-btn hotkey-capture-btn"
            onClick={hotkeyCapture.startCapture}
            disabled={hotkeyCapture.saving}
            data-capturing={hotkeyCapture.capturing}
          >
            {hotkeyCapture.capturing
              ? t("settings.pressTranslationHotkey")
              : hotkeyDisplay
                ? <Kbd combo={hotkeyDisplay} />
                : t("settings.noTranslationHotkey")}
          </button>
          <button
            className="btn-ghost translation-clear-button"
            onClick={onClearHotkey}
            disabled={hotkeyCapture.saving}
          >
            {t("common.clear")}
          </button>
        </div>
        <p className="settings-hint settings-hint-flush">{t("settings.translationHint")}</p>
        <div className="settings-column" style={{ gap: 4 }}>
          <span className="settings-option-desc">{t("settings.translationTargetLanguage")}</span>
          <TranslationLanguagePicker
            value={target}
            ariaLabel={t("settings.translationTargetLanguage")}
            allowOff
            control={{
              open: picker.isOpen("translationLanguage"),
              expanded: picker.isExpanded("translationLanguage"),
              toggle: () => picker.toggle("translationLanguage"),
              close: picker.close,
              setRef: picker.setRef("translationLanguage"),
              popoverClassName: picker.popoverClass("translationLanguage"),
            }}
            onSelect={(nextTarget) => { void onSelectTarget(nextTarget); }}
          />
        </div>
      </div>
    </section>
  );
}
