import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import i18n from "@/i18n";
import type { FunASRStatus } from "@/types";
import {
  cancelModelDownload,
  checkFunASRStatus,
  downloadModels,
  restartFunASR,
  startFunASR,
} from "@/api/tauri";
export type ModelStage =
  | "checking"
  | "need_download"
  | "need_repair"
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
/** Auto-download retries when app cold-starts without models. */
const AUTO_DOWNLOAD_MAX_RETRIES = 1;
const AUTO_DOWNLOAD_RETRY_DELAY_MS = 3000;
function getEngineStartFallbackMessage(): string {
  return i18n.t("model.engineStartFailed");
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
 * React hook that tracks the FunASR lifecycle:
 *   checking -> need_download -> downloading -> loading -> ready
 *
 * Polls the backend every 6 seconds while in transient states and listens
 * for download-progress events from the Rust side.
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
  const autoDownloadAllowedRef = useRef(true);
  const downloadGenerationRef = useRef(0);
  const statusRequestIdRef = useRef(0);
  const startRequestIdRef = useRef(0);
  const autoDownloadTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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

  const invalidateDownload = useCallback(() => {
    downloadGenerationRef.current += 1;
    pendingAutoDownloadRef.current = false;
    if (autoDownloadTimerRef.current !== null) {
      clearTimeout(autoDownloadTimerRef.current);
      autoDownloadTimerRef.current = null;
    }
  }, []);

  const scheduleAutoDownload = useCallback((delay = 0) => {
    if (!mountedRef.current || !autoDownloadAllowedRef.current) return;
    if (downloadListenerReadyRef.current) {
      if (autoDownloadTimerRef.current !== null) clearTimeout(autoDownloadTimerRef.current);
      const generation = downloadGenerationRef.current;
      autoDownloadTimerRef.current = setTimeout(() => {
        autoDownloadTimerRef.current = null;
        if (!mountedRef.current || !autoDownloadAllowedRef.current
          || generation !== downloadGenerationRef.current || downloadingRef.current) return;
        triggerDownloadRef.current?.("auto");
      }, delay);
    } else {
      pendingAutoDownloadRef.current = true;
    }
  }, []);

  const enterNeedDownloadState = useCallback(() => {
    startRequestIdRef.current += 1;
    restartAttemptedRef.current = false;
    loadingChecksRef.current = 0;
    setStage("need_download");
    setError(null);
    setDownloadMessage(null);
    if (!autoDownloadTriggeredRef.current && !downloadingRef.current) {
      autoDownloadTriggeredRef.current = true;
      scheduleAutoDownload();
    }
  }, [scheduleAutoDownload]);

  const enterErrorState = useCallback((message: string) => {
    statusRequestIdRef.current += 1;
    startRequestIdRef.current += 1;
    setError(message);
    setStage("error");
    clearPolling();
  }, [clearPolling]);

  const enterNeedRepairState = useCallback(() => {
    invalidateDownload();
    clearDownloadWatchdog();
    clearPolling();
    setDownloadingState(false);
    restartAttemptedRef.current = false;
    loadingChecksRef.current = 0;
    setStage("need_repair");
    setError(null);
    setDownloadMessage(null);
  }, [clearDownloadWatchdog, clearPolling, invalidateDownload, setDownloadingState]);

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
    const generation = downloadGenerationRef.current;
    const requestId = ++statusRequestIdRef.current;
    const currentGeneration = () => mountedRef.current && generation === downloadGenerationRef.current;
    const current = () => currentGeneration() && requestId === statusRequestIdRef.current;

    try {
      const status = await checkFunASRStatus();
      if (!current()) return;

      applyStatusSnapshot(status);

      if (status.model_state === "corrupt") {
        enterNeedRepairState();
        return;
      }

      if (status.model_state === "missing"
        || (!status.model_state && status.models_present === false)) {
        enterNeedDownloadState();
        return;
      }

      if (status.running && status.ready) {
        startRequestIdRef.current += 1;
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

      const startRequestId = ++startRequestIdRef.current;
      // Progress events and loading polls do not retire the active native start.
      const currentStart = () => currentGeneration() && startRequestId === startRequestIdRef.current;
      try {
        await startFunASR();
        if (!currentStart()) return;
        startFailuresRef.current = 0;
      } catch (startErr) {
        if (!currentStart()) return;
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
      if (!current()) return;
      enterErrorState(toErrorMessage(err, i18n.t("model.checkStatusFailed")));
    }
  }, [applyStatusSnapshot, clearPolling, enterErrorState, enterNeedDownloadState, enterNeedRepairState]);

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
      invalidateDownload();
      downloadListenerReadyRef.current = false;
      pendingAutoDownloadRef.current = false;
      clearDownloadWatchdog();
      clearPolling();
    };
  }, [checkStatus, clearDownloadWatchdog, clearPolling, invalidateDownload, startPolling]);

  // Listen for funasr-status events (loading progress, crashed, etc.)
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    type FunasrStatusPayload = {
      status: string;
      message?: string;
      device?: string | null;
      gpu_name?: string | null;
    } & Pick<FunASRStatus, "models_present" | "missing_models" | "model_state" | "corrupt_models">;

    const setup = async () => {
      unlisten = await listen<FunasrStatusPayload>(
        "funasr-status",
        (event) => {
          if (disposed || !mountedRef.current) return;
          statusRequestIdRef.current += 1;
          const { status, message } = event.payload;
          applyStatusSnapshot(event.payload);

          if (event.payload.model_state === "corrupt") {
            enterNeedRepairState();
            return;
          }
          if (event.payload.model_state === "missing") {
            enterNeedDownloadState();
            return;
          }

          if (status === "ready") {
            startRequestIdRef.current += 1;
            setStage("ready");
            setError(null);
            clearPolling();
          } else if (status === "need_api_key") {
            enterErrorState(message ?? i18n.t("model.needApiKey"));
          } else if (status === "loading") {
            setStage("loading");
            setDownloadMessage(message ?? null);
          } else if (status === "error") {
            if (!event.payload.model_state && message?.includes("模型文件未下载")) {
              enterNeedDownloadState();
              return;
            }
            enterErrorState(message ?? getEngineStartFallbackMessage());
          } else if (status === "crashed") {
            startRequestIdRef.current += 1;
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
      if (disposed) unlisten();
    };

    setup();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyStatusSnapshot, checkStatus, clearPolling, enterErrorState, enterNeedDownloadState, enterNeedRepairState, startPolling]);

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
            if (disposed || !mountedRef.current) return;
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
                autoDownloadAllowedRef.current = false;
                invalidateDownload();
                clearDownloadWatchdog();
                setDownloadingState(false);
                setDownloadProgress(0);
                setDownloadMessage(message ?? i18n.t("model.downloadCancelled"));
                autoDownloadTriggeredRef.current = false;
                setStage("need_download");
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
          scheduleAutoDownload();
        }
      } catch (err) {
        if (disposed || !mountedRef.current) return;
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
  }, [clearDownloadWatchdog, invalidateDownload, scheduleAutoDownload, setDownloadingState, startDownloadWatchdog]);

  const triggerDownload = useCallback(async (source: "auto" | "manual" = "manual") => {
    if (!mountedRef.current || downloadingRef.current
      || (source === "auto" && !autoDownloadAllowedRef.current)) return;
    if (source === "manual") {
      invalidateDownload();
      autoDownloadAllowedRef.current = true;
    }
    const generation = downloadGenerationRef.current;
    const current = () => mountedRef.current && generation === downloadGenerationRef.current;
    try {

      if (source === "manual") {
        autoDownloadTriggeredRef.current = true;
      }

      setDownloadingState(true);
      lastDownloadEventAtRef.current = Date.now();
      startDownloadWatchdog();
      setStage("downloading");
      setDownloadProgress(0);
      setDownloadMessage(i18n.t("model.preparingDownload"));
      setError(null);
      await downloadModels();
      if (!current()) return;

      autoDownloadRetryRef.current = 0;
      if (downloadingRef.current) {
        clearDownloadWatchdog();
        setDownloadingState(false);
        setStage("loading");
        setDownloadProgress((prev) => Math.max(prev, 100));
      }
      void checkStatus();
    } catch (err) {
      if (!current()) return;
      clearDownloadWatchdog();
      setDownloadingState(false);
      const message = toErrorMessage(err, i18n.t("model.downloadFailed"));

      if (
        source === "auto" &&
        autoDownloadRetryRef.current < AUTO_DOWNLOAD_MAX_RETRIES
      ) {
        autoDownloadRetryRef.current += 1;
        setError(null);
        setStage("need_download");
        setDownloadMessage(
          i18n.t("model.retryIn", { seconds: Math.ceil(AUTO_DOWNLOAD_RETRY_DELAY_MS / 1000) })
        );
        scheduleAutoDownload(AUTO_DOWNLOAD_RETRY_DELAY_MS);
        return;
      }

      setError(message);
      setStage("error");
    }
  }, [checkStatus, clearDownloadWatchdog, invalidateDownload, scheduleAutoDownload, setDownloadingState, startDownloadWatchdog]);

  triggerDownloadRef.current = triggerDownload;

  const cancelDownload = useCallback(async () => {
    autoDownloadAllowedRef.current = false;
    invalidateDownload();
    const generation = downloadGenerationRef.current;
    try {
      await cancelModelDownload();
      if (!mountedRef.current || generation !== downloadGenerationRef.current) return;
      clearDownloadWatchdog();
      setDownloadingState(false);
      setStage("need_download");
    } catch (err) {
      if (!mountedRef.current || generation !== downloadGenerationRef.current) return;
      setError(toErrorMessage(err, i18n.t("model.cancelFailed")));
      setStage("error");
    }
  }, [clearDownloadWatchdog, invalidateDownload, setDownloadingState]);

  const retry = useCallback(() => {
    invalidateDownload();
    autoDownloadAllowedRef.current = true;
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
  }, [checkStatus, clearDownloadWatchdog, clearPolling, invalidateDownload, setDownloadingState, startPolling]);

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
