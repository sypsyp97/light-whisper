import { useState, useEffect, useRef, useCallback } from "react";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  HOTKEY_MODIFIER_ORDER,
  type HotkeyModifier,
  formatHotkeyForDisplay,
  keyboardEventToHotkey,
  modifierFromKeyboardEvent,
} from "@/lib/hotkey";

interface HotkeyCaptureConfig {
  /** Persist the shortcut. Throw to signal failure. */
  save: (shortcut: string) => Promise<void>;
  /** Label for toast messages, e.g. "说话热键" */
  label: string;
}

/**
 * Reusable keyboard shortcut capture logic.
 *
 * While `capturing` is true, all keydown/keyup events are intercepted.
 * Modifier-only combos (e.g. Ctrl+Win) are detected on full release.
 * Pressing Escape cancels capture.
 */
export function useHotkeyCapture(config: HotkeyCaptureConfig) {
  const [capturing, setCapturing] = useState(false);
  const [saving, setSaving] = useState(false);
  const configRef = useRef(config);
  configRef.current = config;

  const startCapture = useCallback(() => setCapturing(true), []);
  const cancelCapture = useCallback(() => setCapturing(false), []);

  useEffect(() => {
    if (!capturing) return;

    const active = new Set<HotkeyModifier>();
    const peak = new Set<HotkeyModifier>();
    let mainKeyPressed = false;
    let applied = false;

    const reset = () => { active.clear(); peak.clear(); mainKeyPressed = false; };

    const apply = (shortcut: string) => {
      if (applied) return;
      applied = true;
      setSaving(true);
      const display = formatHotkeyForDisplay(shortcut);
      const { save, label } = configRef.current;
      void save(shortcut)
        .then(() => toast.success(i18n.t("toast.hotkeySet", { label, display })))
        .catch((err) => {
          toast.error(err instanceof Error ? err.message : i18n.t("toast.hotkeySetFailed", { label }));
        })
        .finally(() => { setSaving(false); setCapturing(false); reset(); });
    };

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") { setCapturing(false); reset(); return; }

      const mod = modifierFromKeyboardEvent(e);
      if (mod) { active.add(mod); for (const m of active) peak.add(m); return; }

      mainKeyPressed = true;
      const shortcut = keyboardEventToHotkey(e, active);
      if (shortcut) apply(shortcut);
    };

    const onKeyUp = (e: KeyboardEvent) => {
      const mod = modifierFromKeyboardEvent(e);
      if (!mod || applied) return;
      active.delete(mod);
      if (active.size === 0 && !mainKeyPressed && peak.size > 0) {
        const combo = HOTKEY_MODIFIER_ORDER.filter((k) => peak.has(k)).join("+");
        if (combo) apply(combo);
      }
    };

    const onVisibilityChange = () => { if (document.hidden) reset(); };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", reset);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", reset);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [capturing]);

  return { capturing, saving, startCapture, cancelCapture };
}
