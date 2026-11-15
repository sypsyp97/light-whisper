import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  listInputDevices,
  setInputDevice,
  startMicrophoneLevelMonitor,
  stopMicrophoneLevelMonitor,
  testMicrophone,
} from "@/api/tauri";
import type { InputDeviceInfo } from "@/types";
import { INPUT_DEVICE_STORAGE_KEY, MIC_LEVEL_MONITOR_ENABLED_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { useTranslation } from "react-i18next";

interface MicrophoneLevelPayload {
  deviceName?: string;
  level?: number;
}

interface UseMicrophoneSettingsOptions {
  active: boolean;
  closePicker: () => void;
  isRecording: boolean;
}


let monitorOwner: symbol | null = null;
let monitorQueue = Promise.resolve();

export function useMicrophoneSettings({
  active,
  closePicker,
  isRecording,
}: UseMicrophoneSettingsOptions) {
  const { t } = useTranslation();
  const tRef = useRef(t);
  tRef.current = t;
  const [inputDevices, setInputDevices] = useState<InputDeviceInfo[]>([]);
  const [selectedInputDeviceName, setSelectedInputDeviceName] = useState("");
  const [deviceListLoading, setDeviceListLoading] = useState(true);
  const [micLevel, setMicLevel] = useState(0);
  const [micMonitorReady, setMicMonitorReady] = useState(false);
  const [micLevelMonitorEnabled, setMicLevelMonitorEnabled] = useState(
    () => readLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY) === "true",
  );
  const mountedRef = useRef(true);
  const intentRef = useRef({ active, isRecording, enabled: micLevelMonitorEnabled });
  intentRef.current = { active, isRecording, enabled: micLevelMonitorEnabled };
  const monitorGenerationRef = useRef(0);
  const ownerRef = useRef(Symbol("microphone-settings"));
  const testGenerationRef = useRef(0);
  const pendingTestRef = useRef<number | null>(null);

  const reconcileMonitor = useCallback((resume = true) => {
    if (monitorOwner !== ownerRef.current) return Promise.resolve();
    const generation = ++monitorGenerationRef.current;
    if (mountedRef.current) {
      setMicMonitorReady(false);
      setMicLevel(0);
    }
    const allowed = () => {
      const intent = intentRef.current;
      return resume && mountedRef.current && monitorOwner === ownerRef.current
        && generation === monitorGenerationRef.current
        && intent.active && intent.enabled && !intent.isRecording && pendingTestRef.current === null;
    };
    const operation = monitorQueue.then(async () => {
      if (monitorOwner !== ownerRef.current || generation !== monitorGenerationRef.current) return;
      await stopMicrophoneLevelMonitor().catch(() => undefined);
      if (!allowed()) return;
      try {
        await startMicrophoneLevelMonitor();
        if (allowed()) setMicMonitorReady(true);
        else await stopMicrophoneLevelMonitor().catch(() => undefined);
      } catch {

      }
    });
    monitorQueue = operation;
    return operation;
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    monitorOwner = ownerRef.current;
    return () => {
      mountedRef.current = false;
      void reconcileMonitor(false);
    };
  }, [reconcileMonitor]);

  const refreshInputDevices = useCallback(async () => {
    setDeviceListLoading(true);
    try {
      const payload = await listInputDevices();
      setInputDevices(payload.devices);
      setSelectedInputDeviceName(payload.selectedDeviceName ?? "");
    } catch {
      toast.error(tRef.current("toast.micListFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      const stored = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
      if (stored) {
        await setInputDevice(stored).catch(() => {});
      }
      await refreshInputDevices();
    })();
  }, [refreshInputDevices]);

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    void (async () => {
      try {
        unlisten = await listen<MicrophoneLevelPayload>("microphone-level", (event) => {
          const intent = intentRef.current;
          if (disposed || !mountedRef.current || monitorOwner !== ownerRef.current
            || !intent.active || !intent.enabled
            || intent.isRecording || pendingTestRef.current !== null) return;
          const level = typeof event.payload?.level === "number" ? event.payload.level : 0;
          setMicLevel(Math.max(0, Math.min(1, level)));
        });
      } catch {

      }

      if (disposed && unlisten) {
        unlisten();
        unlisten = null;
      }
      if (!disposed) await reconcileMonitor();
    })();

    return () => {
      disposed = true;
      unlisten?.();
      void reconcileMonitor(false);
    };
  }, [active, isRecording, micLevelMonitorEnabled, selectedInputDeviceName, reconcileMonitor]);

  const handleInputDeviceChange = useCallback(async (name: string) => {
    closePicker();
    setDeviceListLoading(true);
    try {
      await setInputDevice(name || null);
      writeLocalStorage(INPUT_DEVICE_STORAGE_KEY, name || "");
      setSelectedInputDeviceName(name);
      await refreshInputDevices();
    } catch {
      toast.error(t("toast.micSwitchFailed"));
    } finally {
      setDeviceListLoading(false);
    }
  }, [closePicker, refreshInputDevices, t]);

  const handleMicLevelMonitorToggle = useCallback((enabled: boolean) => {
    intentRef.current.enabled = enabled;
    setMicLevelMonitorEnabled(enabled);
    writeLocalStorage(MIC_LEVEL_MONITOR_ENABLED_KEY, enabled ? "true" : "false");
    if (!enabled) {
      setMicMonitorReady(false);
      setMicLevel(0);
    }
  }, []);

  const handleTestMicrophone = useCallback(async () => {
    const generation = ++testGenerationRef.current;
    pendingTestRef.current = generation;
    const current = () => mountedRef.current && monitorOwner === ownerRef.current
      && pendingTestRef.current === generation;
    try {
      await reconcileMonitor(false);
      if (!mountedRef.current || monitorOwner !== ownerRef.current) return;
      const message = await testMicrophone();
      if (current()) toast.success(message);
    } catch {
      if (current()) toast.error(tRef.current("toast.micTestFailed"));
    } finally {
      if (pendingTestRef.current === generation) {
        pendingTestRef.current = null;
        await reconcileMonitor();
      }
    }
  }, [reconcileMonitor]);

  return {
    deviceListLoading,
    handleInputDeviceChange,
    handleMicLevelMonitorToggle,
    handleTestMicrophone,
    inputDevices,
    micLevel,
    micLevelMonitorEnabled,
    micMonitorReady,
    refreshInputDevices,
    selectedInputDeviceName,
  };
}
