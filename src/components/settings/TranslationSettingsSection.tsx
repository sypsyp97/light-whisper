import { useState } from "react";
import { Check, Languages } from "lucide-react";
import { useTranslation } from "react-i18next";
import Kbd from "@/components/Kbd";

const COMMON_LANGUAGES = [
  "English", "日本語", "한국어", "Français", "Deutsch", "Español", "Русский", "Português",
] as const;

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
  const [pickerOpen, setPickerOpen] = useState(false);
  const [customInput, setCustomInput] = useState("");
  const [showCustomInput, setShowCustomInput] = useState(false);

  const selectTarget = (nextTarget: string | null) => {
    setPickerOpen(false);
    setShowCustomInput(false);
    setCustomInput("");
    void onSelectTarget(nextTarget);
  };

  const togglePicker = () => {
    setPickerOpen((current) => {
      if (!current) {
        setShowCustomInput(false);
        setCustomInput("");
      }
      return !current;
    });
  };

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
        <div className="settings-row">
          <span className="permission-label">
            {target ? t("settings.targetLanguage", { language: target }) : t("settings.notEnabled")}
          </span>
          <button className="btn-ghost translation-picker-toggle" onClick={togglePicker}>
            {pickerOpen ? t("settings.collapse") : target ? t("settings.changeTarget") : t("settings.selectLanguage")}
          </button>
        </div>
        {pickerOpen && (
          <div className="settings-column translation-picker-panel">
            <p className="settings-hint settings-hint-flush">{t("settings.translationSelectHint")}</p>
            <div className="translation-language-list" role="listbox" aria-label={t("settings.selectLanguage")}>
              <button
                type="button"
                className="picker-option translation-language-option"
                role="option"
                aria-selected={!target}
                data-active={!target}
                onClick={() => selectTarget(null)}
              >
                {t("settings.off")}
              </button>
              {COMMON_LANGUAGES.map((language) => (
                <button
                  key={language}
                  type="button"
                  className="picker-option translation-language-option"
                  role="option"
                  aria-selected={target === language}
                  data-active={target === language}
                  onClick={() => selectTarget(language)}
                >
                  {language}
                </button>
              ))}
              {target && !COMMON_LANGUAGES.includes(target as typeof COMMON_LANGUAGES[number]) && (
                <button
                  type="button"
                  className="picker-option translation-language-option"
                  role="option"
                  aria-selected="true"
                  data-active="true"
                >
                  {target}
                </button>
              )}
              <button
                type="button"
                className="picker-option translation-language-option"
                data-active={showCustomInput}
                onClick={() => setShowCustomInput((current) => !current)}
              >
                {t("settings.customLang")}
              </button>
            </div>
            {showCustomInput && (
              <div className="translation-custom-row">
                <input
                  type="text"
                  className="settings-input"
                  placeholder={t("settings.customLangPlaceholder")}
                  aria-label={t("settings.customLangLabel")}
                  value={customInput}
                  onChange={(event) => setCustomInput(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && customInput.trim()) selectTarget(customInput.trim());
                  }}
                  autoFocus
                />
                <button
                  className="test-btn translation-custom-submit"
                  disabled={!customInput.trim()}
                  aria-label={t("settings.selectLanguage")}
                  onClick={() => {
                    if (customInput.trim()) selectTarget(customInput.trim());
                  }}
                >
                  <Check size={14} />
                </button>
              </div>
            )}
          </div>
        )}
      </div>
    </section>
  );
}
