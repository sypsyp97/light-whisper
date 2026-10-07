import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getGpuIdleSeconds, setGpuIdleSeconds } from "@/api/tauri";
import { SettingsReveal } from "./SettingsReveal";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import {
  GPU_IDLE_OFF_SECONDS,
  GPU_IDLE_SUGGESTED_SECONDS,
  normalizeGpuIdleSeconds,
} from "@/lib/gpuIdle";

export default function GpuIdleUnloadControl() {
  const { t } = useTranslation();
  const [seconds, setSeconds] = useState(GPU_IDLE_OFF_SECONDS);
  const [draft, setDraft] = useState(String(GPU_IDLE_SUGGESTED_SECONDS));
  const [saving, setSaving] = useState(false);
  const edited = useRef(false);
  const togglePointerDown = useRef(false);
  const pending = useRef(false);
  const rememberedTimeout = useRef(GPU_IDLE_SUGGESTED_SECONDS);

  useEffect(() => {
    let cancelled = false;
    Promise.all([getGpuIdleSeconds(), getGpuIdleSeconds(true)])
      .then(([value, remembered]) => {
        if (cancelled || edited.current) return;
        const next = normalizeGpuIdleSeconds(value);
        setSeconds(next);
        rememberedTimeout.current = next || normalizeGpuIdleSeconds(remembered) || GPU_IDLE_SUGGESTED_SECONDS;
        setDraft(String(rememberedTimeout.current));
      })
      .catch(() => {
        // Missing config reads as off, matching an engine.json without the key.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const persist = async (next: number) => {
    if (pending.current) return;
    pending.current = true;
    edited.current = true;
    const secondsToSave = normalizeGpuIdleSeconds(next);
    const timeoutToSave = normalizeGpuIdleSeconds(draft) || rememberedTimeout.current;
    setSaving(true);
    try {
      const saved = normalizeGpuIdleSeconds(await (secondsToSave > 0
        ? setGpuIdleSeconds(secondsToSave)
        : setGpuIdleSeconds(secondsToSave, timeoutToSave)));
      setSeconds(saved);
      rememberedTimeout.current = saved || timeoutToSave;
      setDraft(String(rememberedTimeout.current));
    } catch {
      toast.error(t("settings.gpuIdleSaveFailed"));
    } finally {
      pending.current = false;
      setSaving(false);
    }
  };
  const saveDraft = useDebouncedCallback(persist, 600, { onUnmount: "flush" });

  const enabled = seconds > 0;

  const commitDraft = () => {
    saveDraft.cancel();
    const parsed = normalizeGpuIdleSeconds(draft);
    if (parsed === seconds) return;
    void persist(parsed);
  };

  return (
    <div className="settings-inline-panel" style={{ marginTop: 8 }}>
      <div className="settings-column" style={{ gap: 6 }}>
        <div className="settings-row" style={{ gap: 8, alignItems: "center" }}>
          <div className="settings-column" style={{ gap: 2, flex: 1 }}>
            <span className="permission-label">{t("settings.gpuIdleTitle")}</span>
            <span className="settings-hint" style={{ margin: 0 }}>{t("settings.gpuIdleDesc")}</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={enabled}
            aria-label={t("settings.gpuIdleTitle")}
            disabled={saving}
            onPointerDown={(event) => { togglePointerDown.current = event.button === 0; }}
            onPointerUp={() => { togglePointerDown.current = false; }}
            onPointerCancel={() => { togglePointerDown.current = false; }}
            onBlur={() => { togglePointerDown.current = false; }}
            onClick={() => {
              saveDraft.cancel();
              void persist(enabled ? GPU_IDLE_OFF_SECONDS : normalizeGpuIdleSeconds(draft) || GPU_IDLE_SUGGESTED_SECONDS);
            }}
            className="toggle-switch"
            style={{
              flexShrink: 0,
            }}
          >
            <div className="toggle-knob" />
          </button>
        </div>
        <SettingsReveal open={enabled} gap={6}>
          <label className="settings-column" style={{ gap: 4 }}>
            <span className="settings-option-desc">{t("settings.gpuIdleSeconds")}</span>
            <input
              className="settings-input"
              type="text"
              inputMode="numeric"
              autoComplete="off"
              spellCheck={false}
              aria-label={t("settings.gpuIdleSeconds")}
              value={draft}
              disabled={saving || !enabled}
              onChange={(event) => {
                edited.current = true;
                const value = event.target.value;
                setDraft(value);
                saveDraft.cancel();
                if (value.trim() && Number.isFinite(Number(value)) && Number(value) >= 0) {
                  saveDraft.schedule(normalizeGpuIdleSeconds(value));
                }
              }}
              onBlur={(event) => {
                // Let a switch click save its final value, without a competing blur save.
                const switchClick = togglePointerDown.current
                  && event.relatedTarget?.getAttribute("role") === "switch";
                togglePointerDown.current = false;
                if (!switchClick) commitDraft();
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.currentTarget.blur();
                }
              }}
              style={{ maxWidth: 140 }}
            />
          </label>
        </SettingsReveal>
      </div>
    </div>
  );
}
