import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import i18n from "@/i18n";
import {
  cancelModelDownload,
  checkFunASRStatus,
  restartFunASR,
  startFunASR,
} from "@/api/tauri";
export type ModelStage =
  | "checking"
  | "need_download"
  | "downloading"
  | "loading"
  | "ready"
  | "error";

interface UseModelStatusReturn {
  stage: ModelStage;
  isReady: boolean;
  device: string | null;
  gpuName: string | null;
  downloadProgress: number;
  downloadMessage: string | null;
  isDownloading: boolean;
  error: string | null;
  downloadModels: () => void;
  cancelDownload: () => void;
  retry: () => void;
}

/** Maximum consecutive start failures before switching to error state. */
const MAX_START_FAILURES = 3;
/** Consecutive loading checks before attempting a restart. */
const MAX_LOADING_CHECKS = 10;
/** Polling interval in milliseconds for transient states. */
const POLL_INTERVAL_MS = 6000;
/** Consider download stalled if no progress event arrives within this period. */
const DOWNLOAD_STALL_HINT_MS = 20000;
function getEngineStartFallbackMessage(): string {
  return i18n.t("model.engineStartFailed");
}

function getOnlineOnlyMessage(): string {
  return i18n.t("model.onlineOnlyNoLocalAsr");
}

function toErrorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

function normalizeProgress(progress: number | undefined, current: number): number {
  if (typeof progress !== "number") {
    return Math.max(current, 1);
  }
  return Math.max(0, Math.min(100, progress));
}

/**
 * React hook that tracks the online ASR lifecycle:
 *   checking -> loading -> ready / error
 *
 * Polls the backend every 6 seconds while in transient states and still keeps
 * the legacy download plumbing no-op-safe, so historical callers fail clearly
 * instead of reviving local-model flows on this mac branch.
 */
export function useModelStatus(): UseModelStatusReturn {
  const [stage, setStage] = useState<ModelStage>("checking");
  const [device, setDevice] = useState<string | null>(null);
  const [gpuName, setGpuName] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState(0);
  const [downloadMessage, setDownloadMessage] = useState<string | null>(null);
  const [downloadActive, setDownloadActive] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const mountedRef = useRef(true);
  const startFailuresRef = useRef(0);
  const restartAttemptedRef = useRef(false);
  const loadingChecksRef = useRef(0);
  const downloadingRef = useRef(false);
  const autoDownloadTriggeredRef = useRef(false);
  const autoDownloadRetryRef = useRef(0);
  const pendingAutoDownloadRef = useRef(false);
  const downloadListenerReadyRef = useRef(false);
  const lastDownloadEventAtRef = useRef(0);
  const downloadWatchdogRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const triggerDownloadRef = useRef<
    ((source?: "auto" | "manual") => void) | null
  >(null);

  const applyStatusSnapshot = useCallback((payload: {
    device?: string | null;
    gpu_name?: string | null;
  }) => {
    if ("device" in payload) {
      setDevice(payload.device ?? null);
    }
    if ("gpu_name" in payload) {
      setGpuName(payload.gpu_name ?? null);
    }
  }, []);

  const clearPolling = useCallback(() => {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  const clearDownloadWatchdog = useCallback(() => {
    if (downloadWatchdogRef.current !== null) {
      clearTimeout(downloadWatchdogRef.current);
      downloadWatchdogRef.current = null;
    }
  }, []);

  const setDownloadingState = useCallback((value: boolean) => {
    downloadingRef.current = value;
    setDownloadActive(value);
  }, []);

  const enterNeedDownloadState = useCallback(() => {
    restartAttemptedRef.current = false;
    loadingChecksRef.current = 0;
    setDownloadingState(false);
    setStage("error");
    setError(getOnlineOnlyMessage());
    setDownloadMessage(null);
    clearPolling();
  }, [clearPolling, setDownloadingState]);

  const enterErrorState = useCallback((message: string) => {
    setError(message);
    setStage("error");
    clearPolling();
  }, [clearPolling]);

  const startDownloadWatchdog = useCallback(() => {
    clearDownloadWatchdog();
    downloadWatchdogRef.current = setTimeout(() => {
      if (!mountedRef.current || !downloadingRef.current) return;
      const silentFor = Date.now() - lastDownloadEventAtRef.current;
      if (silentFor >= DOWNLOAD_STALL_HINT_MS) {
        setDownloadMessage((prev) => {
          if (prev && prev.includes(i18n.t("model.slowNetwork"))) return prev;
          return prev
            ? `${prev} (${i18n.t("model.slowNetworkStillDownloading")})`
            : i18n.t("model.slowNetworkStillDownloading");
        });
      }
      startDownloadWatchdog();
    }, DOWNLOAD_STALL_HINT_MS);
  }, [clearDownloadWatchdog]);

  const checkStatus = useCallback(async () => {
    if (!mountedRef.current) return;
    if (downloadingRef.current) return;

    try {
      const status = await checkFunASRStatus();
      if (!mountedRef.current) return;

      applyStatusSnapshot(status);

      if (status.running && status.ready) {
        startFailuresRef.current = 0;
        autoDownloadRetryRef.current = 0;
        restartAttemptedRef.current = false;
        loadingChecksRef.current = 0;
        setStage("ready");
        setError(null);
        setDownloadMessage(null);
        clearPolling();
        return;
      }

      if (status.models_present === false) {
        enterNeedDownloadState();
        return;
      }

      // 在线引擎 running 但 not ready = 缺 API Key
      if (status.running && !status.ready && status.device === "cloud") {
        enterErrorState(status.message || i18n.t("model.needApiKey"));
        return;
      }

      if (!status.running) {
        loadingChecksRef.current = 0;
      }

      setStage("loading");

      if (status.running) {
        loadingChecksRef.current += 1;
        if (
          loadingChecksRef.current >= MAX_LOADING_CHECKS &&
          !restartAttemptedRef.current
        ) {
          restartAttemptedRef.current = true;
          restartFunASR().catch(() => undefined);
        }
        return;
      }

      try {
        await startFunASR();
        startFailuresRef.current = 0;
      } catch (startErr) {
        startFailuresRef.current += 1;
        if (startFailuresRef.current < MAX_START_FAILURES) {
          return;
        }
        enterErrorState(
          toErrorMessage(
            startErr,
            getEngineStartFallbackMessage()
          )
        );
      }
    } catch (err) {
      if (!mountedRef.current) return;
      enterErrorState(toErrorMessage(err, i18n.t("model.checkStatusFailed")));
    }
  }, [applyStatusSnapshot, clearPolling, enterErrorState, enterNeedDownloadState]);

  const startPolling = useCallback(() => {
    if (intervalRef.current !== null) return;
    intervalRef.current = setInterval(() => {
      void checkStatus();
    }, POLL_INTERVAL_MS);
  }, [checkStatus]);

  useEffect(() => {
    mountedRef.current = true;
    void checkStatus();
    startPolling();

    return () => {
      mountedRef.current = false;
      downloadListenerReadyRef.current = false;
      pendingAutoDownloadRef.current = false;
      clearDownloadWatchdog();
      clearPolling();
    };
  }, [checkStatus, clearDownloadWatchdog, clearPolling, startPolling]);

  // Listen for funasr-status events (loading progress, crashed, etc.)
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    type FunasrStatusPayload = {
      status: string;
      message?: string;
      device?: string | null;
      gpu_name?: string | null;
      models_present?: boolean;
      missing_models?: string[];
    };

    const setup = async () => {
      unlisten = await listen<FunasrStatusPayload>(
        "funasr-status",
        (event) => {
          if (!mountedRef.current) return;
          const { status, message } = event.payload;
          applyStatusSnapshot(event.payload);

          if (status === "ready") {
            setStage("ready");
            setError(null);
            clearPolling();
          } else if (status === "need_api_key") {
            setStage("error");
            setError(message ?? i18n.t("model.needApiKey"));
            clearPolling();
          } else if (status === "loading") {
            setStage("loading");
            setDownloadMessage(message ?? null);
          } else if (status === "error") {
            if (message?.includes("模型文件未下载")) {
              enterNeedDownloadState();
              return;
            }
            enterErrorState(message ?? getEngineStartFallbackMessage());
          } else if (status === "crashed") {
            setStage("loading");
            setError(null);
            setDownloadMessage(message ?? i18n.t("model.serviceRestarting"));
            // Reset counters and trigger restart via polling
            startFailuresRef.current = 0;
            restartAttemptedRef.current = false;
            loadingChecksRef.current = 0;
            // Ensure polling is active
            void checkStatus();
            startPolling();
          }
        }
      );
    };

    setup();
    return () => {
      unlisten?.();
    };
  }, [applyStatusSnapshot, checkStatus, clearPolling, enterErrorState, enterNeedDownloadState, startPolling]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    type DownloadStatusPayload = {
      status: string;
      message?: string;
      progress?: number;
      error?: string;
    };

    const setup = async () => {
      try {
        unlisten = await listen<DownloadStatusPayload>(
          "model-download-status",
          (event) => {
            if (!mountedRef.current) return;
            const { status, progress, message, error: payloadError } = event.payload;

            switch (status) {
              case "downloading":
              case "progress": {
                lastDownloadEventAtRef.current = Date.now();
                startDownloadWatchdog();
                setDownloadingState(true);
                setStage("downloading");
                setError(null);
                setDownloadProgress((prev) => normalizeProgress(progress, prev));
                setDownloadMessage(message ?? null);
                break;
              }
              case "completed": {
                clearDownloadWatchdog();
                autoDownloadRetryRef.current = 0;
                setDownloadingState(false);
                setDownloadProgress(100);
                setDownloadMessage(message ?? null);
                setStage("loading");
                restartFunASR().catch(() => {});
                break;
              }
              case "cancelled": {
                clearDownloadWatchdog();
                setDownloadingState(false);
                setDownloadProgress(0);
                setDownloadMessage(null);
                autoDownloadTriggeredRef.current = false;
                setError(message ?? getOnlineOnlyMessage());
                setStage("error");
                break;
              }
              case "error": {
                clearDownloadWatchdog();
                setDownloadingState(false);
                autoDownloadTriggeredRef.current = false;
                setError(payloadError || message || i18n.t("model.downloadFailed"));
                setStage("error");
                break;
              }
              default:
                break;
            }
          }
        );

        if (disposed) {
          unlisten?.();
          return;
        }

        downloadListenerReadyRef.current = true;
        if (pendingAutoDownloadRef.current && !downloadingRef.current) {
          pendingAutoDownloadRef.current = false;
          setTimeout(() => triggerDownloadRef.current?.("auto"), 0);
        }
      } catch (err) {
        if (!mountedRef.current) return;
        setError(toErrorMessage(err, i18n.t("model.listenFailed")));
        setStage("error");
      }
    };

    setup();
    return () => {
      disposed = true;
      downloadListenerReadyRef.current = false;
      clearDownloadWatchdog();
      unlisten?.();
    };
  }, [clearDownloadWatchdog, setDownloadingState, startDownloadWatchdog]);

  const triggerDownload = useCallback(async (source: "auto" | "manual" = "manual") => {
    if (!mountedRef.current) return;
    clearDownloadWatchdog();
    setDownloadingState(false);
    autoDownloadTriggeredRef.current = false;
    pendingAutoDownloadRef.current = false;
    autoDownloadRetryRef.current = 0;
    setDownloadProgress(0);
    setDownloadMessage(null);
    setError(getOnlineOnlyMessage());
    setStage("error");
    void source;
  }, [clearDownloadWatchdog, setDownloadingState]);

  triggerDownloadRef.current = triggerDownload;

  const cancelDownload = useCallback(async () => {
    try {
      await cancelModelDownload();
    } catch (err) {
      if (!mountedRef.current) return;
      setError(toErrorMessage(err, i18n.t("model.cancelFailed")));
      setStage("error");
    }
  }, []);

  const retry = useCallback(() => {
    clearDownloadWatchdog();
    setDownloadingState(false);
    autoDownloadRetryRef.current = 0;
    pendingAutoDownloadRef.current = false;
    setError(null);
    setDownloadProgress(0);
    setDownloadMessage(null);
    setStage("checking");
    startFailuresRef.current = 0;
    autoDownloadTriggeredRef.current = false;

    clearPolling();
    void checkStatus();
    startPolling();
  }, [checkStatus, clearDownloadWatchdog, clearPolling, setDownloadingState, startPolling]);

  return {
    stage,
    isReady: stage === "ready",
    device,
    gpuName,
    downloadProgress,
    downloadMessage,
    isDownloading: downloadActive,
    error,
    downloadModels: triggerDownload,
    cancelDownload,
    retry,
  };
}
