import { Sun, Moon, Monitor } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useTheme, type ThemeMode } from "@/hooks/useTheme";
import { LANGUAGE_STORAGE_KEY } from "@/lib/constants";
import { writeLocalStorage } from "@/lib/storage";
import Segmented from "@/components/ui/Segmented";
import Picker from "@/components/ui/Picker";
import Field from "@/components/ui/Field";

export default function AppearanceSection() {
  const { t, i18n } = useTranslation();
  const { theme, setTheme } = useTheme();

  const themeOptions: Array<{ value: ThemeMode; label: string; icon: React.ReactNode }> = [
    { value: "light", label: t("settings.themeLight"), icon: <Sun size={13} /> },
    { value: "dark", label: t("settings.themeDark"), icon: <Moon size={13} /> },
    { value: "system", label: t("settings.themeSystem"), icon: <Monitor size={13} /> },
  ];

  const languageOptions = [
    { value: "en", label: "English" },
    { value: "zh", label: "中文" },
  ];

  const currentLang = i18n.language?.startsWith("zh") ? "zh" : "en";

  const handleLanguageChange = (next: string) => {
    void i18n.changeLanguage(next);
    writeLocalStorage(LANGUAGE_STORAGE_KEY, next);
    try {
      window.dispatchEvent(new StorageEvent("storage", { key: LANGUAGE_STORAGE_KEY, newValue: next }));
    } catch {
      // ignore
    }
  };

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-appearance"
      data-nav-id="appearance"
    >
      <h2 className="lw-settings-section-title">{t("settings.appearance")}</h2>
      <Field label={t("settings.themeSystem")}>
        <Segmented
          value={theme}
          options={themeOptions}
          onChange={setTheme}
          data-testid="appearance-theme"
        />
      </Field>
      <Field label={t("settings.language")}>
        <Picker
          value={currentLang}
          options={languageOptions}
          onChange={handleLanguageChange}
          data-testid="appearance-language"
        />
      </Field>
    </section>
  );
}
