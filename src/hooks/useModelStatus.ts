import { useState, useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  checkFunASRStatus,
  checkModelFiles,
  cancelModelDownload,
  downloadModels,
  startFunASR,
  restartFunASR,
} from "../api/funasr";

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
  const triggerDownloadRef = useRef<(() => void) | null>(null);

  const clearPolling = useCallback(() => {
    if (intervalRef.current !== null) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
  }, []);

  const checkStatus = useCallback(async () => {
    if (!mountedRef.current) return;
    if (downloadingRef.current) return;

    const handleModelsNotPresent = () => {
      restartAttemptedRef.current = false;
      loadingChecksRef.current = 0;
      setStage("need_download");
      setDownloadMessage(null);
      if (!autoDownloadTriggeredRef.current && !downloadingRef.current) {
        autoDownloadTriggeredRef.current = true;
        setTimeout(() => triggerDownloadRef.current?.(), 0);
      }
    };

    try {
      const status = await checkFunASRStatus();
      if (!mountedRef.current) return;

      setDevice(status.device ?? null);
      setGpuName(status.gpu_name ?? null);

      if (status.running && status.ready) {
        startFailuresRef.current = 0;
        restartAttemptedRef.current = false;
        loadingChecksRef.current = 0;
        setStage("ready");
        setError(null);
        setDownloadMessage(null);
        clearPolling();
        return;
      }

      if (status.running && !status.ready) {
        const modelCheck = await checkModelFiles();
        if (!mountedRef.current) return;
        if (!modelCheck.all_present) {
          handleModelsNotPresent();
          return;
        }

        loadingChecksRef.current += 1;
        setStage("loading");
        if (
          loadingChecksRef.current >= MAX_LOADING_CHECKS &&
          !restartAttemptedRef.current
        ) {
          restartAttemptedRef.current = true;
          restartFunASR().catch(() => {});
        }
        return;
      }

      loadingChecksRef.current = 0;

      const modelCheck = await checkModelFiles();
      if (!mountedRef.current) return;

      if (!modelCheck.all_present) {
        handleModelsNotPresent();
        return;
      }

      setStage("loading");
      try {
        if (!status.running) {
          await startFunASR();
        }
        startFailuresRef.current = 0;
      } catch (startErr) {
        startFailuresRef.current += 1;

        if (startFailuresRef.current >= MAX_START_FAILURES) {
          const message =
            startErr instanceof Error
              ? startErr.message
              : "FunASR 引擎启动失败，请检查 Python 环境是否安装了 funasr 包";
          setError(message);
          setStage("error");
          clearPolling();
        }
      }
    } catch (err) {
      if (!mountedRef.current) return;
      const message =
        err instanceof Error ? err.message : "检查模型状态失败";
      setError(message);
      setStage("error");
      clearPolling();
    }
  }, [clearPolling]);

  useEffect(() => {
    mountedRef.current = true;
    checkStatus();

    intervalRef.current = setInterval(() => {
      checkStatus();
    }, POLL_INTERVAL_MS);

    return () => {
      mountedRef.current = false;
      clearPolling();
    };
  }, [checkStatus, clearPolling]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    type DownloadStatusPayload = {
      status: string;
      message?: string;
      progress?: number;
      error?: string;
    };

    const setup = async () => {
      unlisten = await listen<DownloadStatusPayload>(
        "model-download-status",
        (event) => {
          if (!mountedRef.current) return;
          const { status, progress, message, error: payloadError } = event.payload;

          if (status === "downloading" || status === "progress") {
            downloadingRef.current = true;
            setDownloadActive(true);
            setStage("downloading");
            if (typeof progress === "number") {
              setDownloadProgress(Math.max(0, Math.min(100, progress)));
            } else {
              setDownloadProgress((prev) => Math.max(prev, 1));
            }
            setDownloadMessage(message ?? null);
          } else if (status === "completed") {
            downloadingRef.current = false;
            setDownloadActive(false);
            setDownloadProgress(100);
            setDownloadMessage(message ?? null);
            setStage("loading");
            restartFunASR().catch(() => {});
          } else if (status === "cancelled") {
            downloadingRef.current = false;
            setDownloadActive(false);
            setDownloadProgress(0);
            setDownloadMessage(message ?? "下载已取消");
            autoDownloadTriggeredRef.current = false;
            setStage("need_download");
          } else if (status === "error") {
            downloadingRef.current = false;
            setDownloadActive(false);
            setError(payloadError || message || "模型下载失败");
            setStage("error");
          }
        }
      );
    };

    setup();
    return () => {
      unlisten?.();
    };
  }, []);

  const triggerDownload = useCallback(async () => {
    try {
      if (downloadingRef.current) return;
      downloadingRef.current = true;
      setDownloadActive(true);
      setStage("downloading");
      setDownloadProgress(0);
      setDownloadMessage("准备下载...");
      setError(null);
      await downloadModels();
      if (!mountedRef.current) return;
      if (downloadingRef.current) {
        downloadingRef.current = false;
        setDownloadActive(false);
        setStage("loading");
        checkStatus();
      }
    } catch (err) {
      downloadingRef.current = false;
      setDownloadActive(false);
      if (!mountedRef.current) return;
      const message =
        err instanceof Error ? err.message : "模型下载失败";
      setError(message);
      setStage("error");
    }
  }, [checkStatus]);

  triggerDownloadRef.current = triggerDownload;

  const cancelDownload = useCallback(async () => {
    try {
      await cancelModelDownload();
    } catch (err) {
      if (!mountedRef.current) return;
      const message =
        err instanceof Error ? err.message : "取消下载失败";
      setError(message);
      setStage("error");
    }
  }, []);

  const retry = useCallback(() => {
    setError(null);
    setDownloadProgress(0);
    setDownloadMessage(null);
    setDownloadActive(false);
    setStage("checking");
    startFailuresRef.current = 0;
    autoDownloadTriggeredRef.current = false;

    clearPolling();
    checkStatus();
    intervalRef.current = setInterval(() => {
      checkStatus();
    }, POLL_INTERVAL_MS);
  }, [checkStatus, clearPolling]);

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
