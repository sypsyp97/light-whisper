import { useCallback, useState } from "react";
import { Keyboard, ClipboardPaste } from "lucide-react";
import { useTranslation } from "react-i18next";
import { setInputMethodCommand, setSoundEnabled } from "@/api/tauri";
import { INPUT_METHOD_KEY, SOUND_ENABLED_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import Field from "@/components/ui/Field";
import Segmented from "@/components/ui/Segmented";
import Toggle from "@/components/ui/Toggle";

type InputMethod = "sendInput" | "clipboard";

export function InputMethodSection() {
  const { t } = useTranslation();
  const [method, setMethod] = useState<InputMethod>(
    () => readLocalStorage(INPUT_METHOD_KEY) === "clipboard" ? "clipboard" : "sendInput",
  );
  const [sound, setSound] = useState(() => readLocalStorage(SOUND_ENABLED_KEY) !== "false");

  const handleMethod = useCallback(async (next: InputMethod) => {
    const prev = method;
    setMethod(next);
    writeLocalStorage(INPUT_METHOD_KEY, next);
    try { await setInputMethodCommand(next); } catch { setMethod(prev); }
  }, [method]);

  const handleSound = useCallback(async (next: boolean) => {
    const prev = sound;
    setSound(next);
    writeLocalStorage(SOUND_ENABLED_KEY, next ? "true" : "false");
    try { await setSoundEnabled(next); } catch { setSound(prev); }
  }, [sound]);

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-input"
      data-nav-id="input"
    >
      <h2 className="lw-settings-section-title">{t("settings.inputMethod")}</h2>
      <Field label={t("settings.inputMethod")}>
        <Segmented
          value={method}
          options={[
            { value: "sendInput", label: t("settings.directInput"), icon: <Keyboard size={12} /> },
            { value: "clipboard", label: t("settings.clipboardPaste"), icon: <ClipboardPaste size={12} /> },
          ]}
          onChange={(v) => void handleMethod(v)}
          data-testid="input-method-segmented"
        />
      </Field>
      <Field label={t("settings.recordingSound")}>
        <Toggle
          checked={sound}
          onChange={(v) => void handleSound(v)}
          label={t("settings.recordingSound")}
          data-testid="input-sound-toggle"
        />
      </Field>
    </section>
  );
}

export default InputMethodSection;
