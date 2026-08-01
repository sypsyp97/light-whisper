import { useEffect, useState } from "react";
import { Check, ChevronsUpDown } from "lucide-react";
import { useTranslation } from "react-i18next";

export const COMMON_TRANSLATION_LANGUAGES = [
  "简体中文",
  "繁體中文",
  "English",
  "日本語",
  "한국어",
  "Français",
  "Deutsch",
  "Español",
  "Русский",
  "Português",
] as const;

interface PickerControl {
  open: boolean;
  expanded: boolean;
  toggle: () => void;
  close: () => void;
  setRef: (element: HTMLDivElement | null) => void;
  popoverClassName: string;
}

interface TranslationLanguagePickerProps {
  value: string | null;
  ariaLabel: string;
  control: PickerControl;
  allowOff?: boolean;
  maxLength?: number;
  onSelect: (value: string | null) => void;
}

export default function TranslationLanguagePicker({
  value,
  ariaLabel,
  control,
  allowOff = false,
  maxLength,
  onSelect,
}: TranslationLanguagePickerProps) {
  const { t } = useTranslation();
  const isCommonLanguage = value !== null
    && COMMON_TRANSLATION_LANGUAGES.includes(
      value as typeof COMMON_TRANSLATION_LANGUAGES[number],
    );
  const isCustomLanguage = value !== null && !isCommonLanguage;
  const [editingCustom, setEditingCustom] = useState(false);
  const [customInput, setCustomInput] = useState(isCustomLanguage ? value : "");

  useEffect(() => {
    if (!control.open) setEditingCustom(false);
  }, [control.open]);

  useEffect(() => {
    if (!editingCustom) setCustomInput(isCustomLanguage ? value : "");
  }, [editingCustom, isCustomLanguage, value]);

  const select = (nextValue: string | null) => {
    onSelect(nextValue);
    setEditingCustom(false);
    control.close();
  };
  const submitCustom = () => {
    const nextValue = customInput.trim();
    if (nextValue) select(nextValue);
  };

  return (
    <div className="picker-shell" ref={control.setRef}>
      <button
        type="button"
        className="picker-trigger"
        data-open={control.open}
        aria-haspopup="listbox"
        aria-expanded={control.expanded}
        aria-label={ariaLabel}
        onClick={control.toggle}
      >
        <span className="picker-trigger-copy">
          <strong>{value ?? t("settings.off")}</strong>
          <span>{t("settings.translationTargetPickerHint")}</span>
        </span>
        <ChevronsUpDown size={14} className="icon-tertiary" />
      </button>

      {control.open && (
        <div className={control.popoverClassName}>
          <div className="picker-list" role="listbox">
            {allowOff && (
              <button
                type="button"
                className="picker-option"
                data-active={value === null}
                onClick={() => select(null)}
              >
                <span className="picker-option-copy">
                  <strong>{t("settings.off")}</strong>
                </span>
              </button>
            )}
            <button
              type="button"
              className="picker-option"
              data-active={isCustomLanguage}
              onClick={() => {
                setCustomInput(isCustomLanguage ? value : "");
                setEditingCustom(true);
              }}
            >
              <span className="picker-option-copy">
                <strong>{t("settings.customLang")}</strong>
                <span>{isCustomLanguage ? value : t("settings.customLangPlaceholder")}</span>
              </span>
            </button>
            {COMMON_TRANSLATION_LANGUAGES.map((language) => (
              <button
                key={language}
                type="button"
                className="picker-option"
                data-active={value === language}
                onClick={() => select(language)}
              >
                <span className="picker-option-copy">
                  <strong>{language}</strong>
                </span>
              </button>
            ))}
          </div>

          {editingCustom && (
            <div className="picker-inline-row">
              <input
                type="text"
                className="settings-input"
                placeholder={t("settings.customLangPlaceholder")}
                aria-label={t("settings.customLangLabel")}
                value={customInput}
                maxLength={maxLength}
                onChange={(event) => setCustomInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") submitCustom();
                }}
                autoFocus
              />
              <button
                type="button"
                className="picker-inline-button"
                disabled={!customInput.trim()}
                aria-label={t("settings.selectLanguage")}
                onClick={submitCustom}
              >
                <Check size={14} />
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
