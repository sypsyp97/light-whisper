import { Languages, Monitor, Moon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useTheme } from "@/hooks/useTheme";
import { LANGUAGE_STORAGE_KEY } from "@/lib/constants";
import { writeLocalStorage } from "@/lib/storage";

const themeOptions = [
  { mode: "light" as const, icon: Sun, labelKey: "settings.themeLight" },
  { mode: "dark" as const, icon: Moon, labelKey: "settings.themeDark" },
  { mode: "system" as const, icon: Monitor, labelKey: "settings.themeSystem" },
] as const;

interface AppearancePickerProps {
  isOpen: boolean;
  isExpanded: boolean;
  toggle: () => void;
  close: () => void;
  setRef: (element: HTMLDivElement | null) => void;
  popoverClass: string;
}

export default function AppearanceSettingsSection({ picker }: { picker: AppearancePickerProps }) {
  const { t, i18n } = useTranslation();
  const { isDark, theme, setTheme } = useTheme();

  return (
    <section className="settings-card settings-card-picker" data-nav-id="appearance" data-picker-open={picker.isOpen}>
      <div className="settings-section-header">
        {isDark ? <Moon size={15} className="icon-accent" /> : <Sun size={15} className="icon-accent" />}
        <h2 className="settings-section-title">{t("settings.appearance")}</h2>
      </div>
      <div className="settings-grid-3">
        {themeOptions.map(({ mode, icon: Icon, labelKey }) => (
          <button
            key={mode}
            className="theme-btn settings-option-btn theme-option"
            aria-label={t("settings.switchToTheme", { label: t(labelKey) })}
            aria-pressed={theme === mode}
            onClick={() => setTheme(mode)}
          >
            <Icon size={20} strokeWidth={1.5} />
            <span className="settings-option-label">{t(labelKey)}</span>
          </button>
        ))}
      </div>
      <div className="settings-row settings-language-row">
        <Languages size={13} className="icon-tertiary settings-row-icon" />
        <span className="settings-option-desc settings-row-label">{t("settings.language")}</span>
        <div ref={picker.setRef} className="settings-language-picker">
          <button
            type="button"
            className="picker-inline-button settings-language-trigger"
            data-open={picker.isOpen}
            aria-haspopup="listbox"
            aria-expanded={picker.isExpanded}
            aria-label={t("settings.language")}
            onClick={picker.toggle}
          >
            <Languages size={13} />
          </button>
          {picker.isOpen && (
            <div className={`${picker.popoverClass} settings-language-popover`}>
              <div className="picker-list" role="listbox">
                {([
                  { lang: "zh", label: "中文" },
                  { lang: "en", label: "English" },
                ] as const).map(({ lang, label }) => (
                  <button
                    key={lang}
                    type="button"
                    className="picker-option"
                    data-active={i18n.language.startsWith(lang)}
                    onClick={() => {
                      void i18n.changeLanguage(lang);
                      writeLocalStorage(LANGUAGE_STORAGE_KEY, lang);
                      picker.close();
                    }}
                  >
                    <strong className="settings-language-option">{label}</strong>
                  </button>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
