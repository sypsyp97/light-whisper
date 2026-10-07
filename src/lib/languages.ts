// Shared common presets; recognition uses the model's language codes.
export const COMMON_LANGUAGE_OPTIONS = [
  { code: "zh", label: "简体中文" },
  { code: "zh", label: "繁體中文" },
  { code: "en", label: "English" },
  { code: "ja", label: "日本語" },
  { code: "ko", label: "한국어" },
  { code: "fr", label: "Français" },
  { code: "de", label: "Deutsch" },
  { code: "es", label: "Español" },
  { code: "ru", label: "Русский" },
  { code: "pt", label: "Português" },
] as const;
