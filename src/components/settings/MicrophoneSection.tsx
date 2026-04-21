import { useCallback, useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import {
  listInputDevices,
  setInputDevice,
  testMicrophone,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
} from "@/api/tauri";
import { INPUT_DEVICE_STORAGE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import type { InputDeviceInfo } from "@/types";
import Field from "@/components/ui/Field";
import Picker from "@/components/ui/Picker";
import IconButton from "@/components/ui/IconButton";
import Button from "@/components/ui/Button";
import Toggle from "@/components/ui/Toggle";

interface LevelPayload { deviceName?: string; level?: number }

export default function MicrophoneSection() {
  const { t } = useTranslation();
  const [devices, setDevices] = useState<InputDeviceInfo[]>([]);
  const [selected, setSelected] = useState("");
  const [monitorEnabled, setMonitorEnabled] = useState(
    () => readLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY) === "true",
  );
  const [level, setLevel] = useState(0);
  const unlistenRef = useRef<null | (() => void)>(null);

  const refresh = useCallback(async () => {
    try {
      const payload = await listInputDevices();
      setDevices(payload.devices);
      const stored = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
      if (stored !== null) setSelected(stored);
      else if (payload.selectedDeviceName) setSelected(payload.selectedDeviceName);
    } catch {
      toast.error(t("toast.micListFailed"));
    }
  }, [t]);

  useEffect(() => { void refresh(); }, [refresh]);

  useEffect(() => {
    if (!monitorEnabled) {
      setLevel(0);
      void stopMicrophoneLevelMonitor().catch(() => {});
      unlistenRef.current?.();
      unlistenRef.current = null;
      return;
    }
    let disposed = false;
    void startMicrophoneLevelMonitor().catch(() => {});
    void (async () => {
      const un = await listen<LevelPayload>("microphone-level", (event) => {
        if (typeof event.payload?.level === "number") setLevel(event.payload.level);
      });
      if (disposed) un();
      else unlistenRef.current = un;
    })();
    return () => {
      disposed = true;
      unlistenRef.current?.();
      unlistenRef.current = null;
      void stopMicrophoneLevelMonitor().catch(() => {});
    };
  }, [monitorEnabled]);

  const handleDeviceChange = useCallback(async (name: string) => {
    const prev = selected;
    setSelected(name);
    writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, name);
    try {
      await setInputDevice(name || null);
    } catch {
      setSelected(prev);
      toast.error(t("toast.micSwitchFailed"));
    }
  }, [selected, t]);

  const handleTest = useCallback(async () => {
    try {
      await testMicrophone();
      toast.success(t("common.test"));
    } catch {
      toast.error(t("toast.micTestFailed"));
    }
  }, [t]);

  const handleMonitorToggle = useCallback((enabled: boolean) => {
    setMonitorEnabled(enabled);
    writeLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY, enabled ? "true" : "false");
  }, []);

  const options = [
    { value: "", label: t("settings.followSystemMic") },
    ...devices.map((d) => ({ value: d.name, label: d.name })),
  ];

  return (
    <section
      className="lw-settings-section"
      data-testid="settings-section-microphone"
      data-nav-id="microphone"
    >
      <h2 className="lw-settings-section-title">{t("settings.microphone")}</h2>
      <Field label={t("settings.selectMic")}>
        <div className="lw-inline" style={{ width: "100%" }}>
          <div style={{ flex: 1 }}>
            <Picker
              value={selected}
              options={options}
              onChange={(v) => void handleDeviceChange(v)}
              data-testid="mic-device-picker"
            />
          </div>
          <IconButton
            label={t("common.refresh")}
            icon={<RefreshCw size={14} />}
            onClick={() => void refresh()}
            data-testid="mic-refresh"
          />
          <Button size="sm" onClick={() => void handleTest()} data-testid="mic-test">
            {t("common.test")}
          </Button>
        </div>
      </Field>
      <Field label={t("settings.levelMonitor")}>
        <div className="lw-inline">
          <Toggle
            checked={monitorEnabled}
            onChange={handleMonitorToggle}
            label={t("settings.levelMonitor")}
            data-testid="mic-level-monitor-toggle"
          />
        </div>
        {monitorEnabled && (
          <div
            className="lw-mic-meter"
            role="meter"
            aria-valuenow={Math.round(level * 100)}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={t("settings.micLevelPreview")}
            data-testid="mic-level-bar"
          >
            <div className="lw-mic-meter-fill" style={{ width: `${Math.min(100, Math.round(level * 100))}%` }} />
          </div>
        )}
      </Field>
    </section>
  );
}
