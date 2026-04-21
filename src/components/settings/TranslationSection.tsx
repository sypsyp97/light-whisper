import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { setTranslationHotkey, setTranslationTarget, getUserProfile } from "@/api/tauri";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { formatHotkeyForDisplay } from "@/lib/hotkey";
import Field from "@/components/ui/Field";
import Button from "@/components/ui/Button";
import Picker from "@/components/ui/Picker";
import TextInput from "@/components/ui/TextInput";
import Kbd from "@/components/ui/Kbd";

const PRESET_LANGS = ["English", "日本語", "한국어", "Français", "Deutsch", "Español", "Русский", "Português"];

export function TranslationSection() {
  const { t } = useTranslation();
  const [hotkey, setHotkeyState] = useState<string | null>(null);
  const [target, setTargetState] = useState<string>("");
  const [isCustom, setIsCustom] = useState(false);

  useEffect(() => {
    void getUserProfile().then((profile) => {
      setHotkeyState(profile.translation_hotkey ?? null);
      const tgt = profile.translation_target ?? "";
      setTargetState(tgt);
      setIsCustom(Boolean(tgt) && !PRESET_LANGS.includes(tgt));
    }).catch(() => {});
  }, []);

  const capture = useHotkeyCapture({
    save: async (shortcut) => {
      await setTranslationHotkey(shortcut);
      setHotkeyState(shortcut);
    },
    label: t("settings.translationHotkeyLabel"),
  });

  const clearHotkey = useCallback(async () => {
    try {
      await setTranslationHotkey(null);
      setHotkeyState(null);
    } catch { /* */ }
  }, []);

  const persistTarget = useCallback(async (next: string | null) => {
    try {
      const autoPolish = await setTranslationTarget(next);
      if (autoPolish) toast.success(t("toast.translationAutoPolish"));
    } catch {
      toast.error(t("toast.translationSaveFailed"));
    }
  }, [t]);

  const handleTargetChange = useCallback(async (next: string) => {
    if (next === "__off") {
      setIsCustom(false);
      setTargetState("");
      await persistTarget(null);
      return;
    }
    if (next === "__custom") {
      setIsCustom(true);
      return;
    }
    setIsCustom(false);
    setTargetState(next);
    await persistTarget(next);
  }, [persistTarget]);

  const handleCustomChange = useCallback((v: string) => {
    setTargetState(v);
  }, []);

  const handleCustomBlur = useCallback(() => {
    void persistTarget(target || null);
  }, [target, persistTarget]);

  const options = [
    { value: "__off", label: t("settings.off") },
    ...PRESET_LANGS.map((l) => ({ value: l, label: l })),
    { value: "__custom", label: t("settings.customLang") },
  ];

  const pickerValue = !target ? "__off" : isCustom ? "__custom" : target;
  const hotkeyLabel = hotkey ? formatHotkeyForDisplay(hotkey) : "";

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-translation"
      data-nav-id="translation"
    >
      <h2 className="lw-settings-section-title">{t("settings.translation")}</h2>
      <Field label={t("settings.translationHotkeyLabel")} hint={t("settings.translationHint")}>
        <div className="lw-inline">
          <Button
            onClick={capture.startCapture}
            loading={capture.saving}
            data-testid="translation-hotkey-btn"
          >
            {capture.capturing
              ? t("settings.pressTranslationHotkey")
              : hotkeyLabel ? <Kbd combo={hotkeyLabel} /> : t("settings.noTranslationHotkey")}
          </Button>
          {hotkey && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => void clearHotkey()}
              data-testid="translation-hotkey-clear"
            >
              {t("common.clear")}
            </Button>
          )}
        </div>
      </Field>
      <Field label={t("settings.selectLanguage")}>
        <Picker
          value={pickerValue}
          options={options}
          onChange={(v) => void handleTargetChange(v)}
          data-testid="translation-target-picker"
        />
      </Field>
      {isCustom && (
        <Field label={t("settings.customLangLabel")}>
          <TextInput
            value={target}
            onChange={handleCustomChange}
            onBlur={handleCustomBlur}
            placeholder={t("settings.customLangPlaceholder")}
            data-testid="translation-custom-input"
          />
        </Field>
      )}
    </section>
  );
}

export default TranslationSection;
