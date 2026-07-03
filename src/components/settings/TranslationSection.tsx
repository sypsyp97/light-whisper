import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { setTranslationHotkey, setTranslationTarget, getUserProfile } from "@/api/tauri";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { formatHotkeyForDisplay } from "@/lib/hotkey";
import { AI_POLISH_ENABLED_CHANGED_EVENT, AI_POLISH_ENABLED_KEY } from "@/lib/constants";
import { writeLocalStorage } from "@/lib/storage";
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
  const savedTargetRef = useRef("");

  const applyTargetState = useCallback((next: string) => {
    setTargetState(next);
    setIsCustom(Boolean(next) && !PRESET_LANGS.includes(next));
  }, []);

  useEffect(() => {
    void getUserProfile().then((profile) => {
      setHotkeyState(profile.translation_hotkey ?? null);
      const tgt = profile.translation_target ?? "";
      savedTargetRef.current = tgt;
      applyTargetState(tgt);
    }).catch(() => {});
  }, [applyTargetState]);

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
      if (next) {
        writeLocalStorage(AI_POLISH_ENABLED_KEY, "true");
        window.dispatchEvent(new Event(AI_POLISH_ENABLED_CHANGED_EVENT));
      }
      savedTargetRef.current = next ?? "";
      if (autoPolish) {
        toast.success(t("toast.translationAutoPolish"));
      }
      return true;
    } catch {
      toast.error(t("toast.translationSaveFailed"));
      return false;
    }
  }, [t]);

  const handleTargetChange = useCallback(async (next: string) => {
    if (next === "__off") {
      setIsCustom(false);
      setTargetState("");
      if (!await persistTarget(null)) {
        applyTargetState(savedTargetRef.current);
      }
      return;
    }
    if (next === "__custom") {
      setIsCustom(true);
      setTargetState("");
      return;
    }
    setIsCustom(false);
    setTargetState(next);
    if (!await persistTarget(next)) {
      applyTargetState(savedTargetRef.current);
    }
  }, [applyTargetState, persistTarget]);

  const handleCustomChange = useCallback((v: string) => {
    setTargetState(v);
  }, []);

  const handleCustomBlur = useCallback(() => {
    const next = target.trim();
    setTargetState(next);
    void persistTarget(next || null).then((ok) => {
      if (!ok) applyTargetState(savedTargetRef.current);
    });
  }, [applyTargetState, target, persistTarget]);

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
      <Field label={t("settings.selectLanguage")} hint={t("settings.translationSelectHint")}>
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
