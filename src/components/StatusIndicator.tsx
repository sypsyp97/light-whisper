import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Cpu, Globe, Download, Undo2 } from "lucide-react";
import type { ModelStage } from "@/hooks/useModelStatus";
import type { PolishStage } from "@/hooks/useRecording";

interface StatusIndicatorProps {
  stage: ModelStage;
  isReady: boolean;
  isStarting: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  polishStage?: PolishStage | null;
  device: string | null;
  gpuName: string | null;
  downloadProgress: number;
  downloadMessage: string | null;
  isDownloading: boolean;
  downloadModels: () => void;
  cancelDownload: () => void;
  onUseOriginal?: () => void;
  children?: ReactNode;
}

function getDeviceLabel(device: string, gpuName: string | null, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (device === "cloud") return t("status.online");
  if (device === "cuda" || device === "gpu") return gpuName || "GPU";
  return "CPU";
}

function getLoadingProgress(message: string | null): number | null {
  if (!message) return null;
  const match = message.match(/(\d{1,3})%/);
  if (!match) return null;
  const value = Number(match[1]);
  if (!Number.isFinite(value)) return null;
  return Math.max(0, Math.min(100, value));
}

function getStatusText(
  isStarting: boolean,
  isRecording: boolean,
  isProcessing: boolean,
  polishStage: PolishStage | null,
  isReady: boolean,
  stage: ModelStage,
  downloadProgress: number,
  downloadMessage: string | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (isStarting) return t("status.connectingMicrophone");
  if (isRecording) return t("status.listening");
  if (isProcessing) return t(polishStage ? `status.${polishStage}` : "status.recognizing");
  if (isReady) return t("status.clickToStart");
  if (stage === "downloading") {
    return downloadProgress > 1
      ? t("status.downloadingModel", { progress: Math.round(downloadProgress) })
      : (downloadMessage ?? t("status.downloadPreparing"));
  }
  if (stage === "need_download") return t("status.needDownload");
  if (stage === "need_repair") return t("status.needRepair");
  if (stage === "loading") return downloadMessage || t("status.modelLoading");
  return t("status.preparing");
}

export default function StatusIndicator({
  stage, isReady, isStarting, isRecording, isProcessing, polishStage = null,
  device, gpuName, downloadProgress, downloadMessage,
  isDownloading, downloadModels, cancelDownload, onUseOriginal, children,
}: StatusIndicatorProps) {
  const { t } = useTranslation();
  const showProgressBar = stage === "downloading" || stage === "loading";
  const loadingProgress = stage === "loading" ? getLoadingProgress(downloadMessage) : null;
  const determinateProgress = stage === "downloading"
    ? (downloadProgress > 1 ? downloadProgress : null)
    : loadingProgress;

  return (
    <>
      {children}

      <p role="status" aria-live="polite" className="status-text">
        {getStatusText(isStarting, isRecording, isProcessing, polishStage, isReady, stage, downloadProgress, downloadMessage, t)}
      </p>
      {device && (
        <span className="recording-device">
          {device === "cloud" ? <Globe size={11} aria-hidden="true" /> : <Cpu size={11} aria-hidden="true" />}
          {getDeviceLabel(device, gpuName, t)}
        </span>
      )}
      {isProcessing && onUseOriginal && (
        <button type="button" className="btn-ghost-sm recording-use-original" onClick={onUseOriginal}>
          <Undo2 size={13} aria-hidden="true" /> {t("recording.useOriginal")}
        </button>
      )}

      {(stage === "need_download" || stage === "need_repair") && !isDownloading && (
        <button onClick={() => downloadModels()} className="btn-primary btn-primary-sm" style={{ marginTop: 4 }}>
          <Download size={12} /> {t(stage === "need_repair" ? "status.repairModels" : "status.startDownload")}
        </button>
      )}
      {stage === "need_download" && isDownloading && (
        <div className="download-info">
          <span>{t("status.anotherDownloading")}</span>
          <button onClick={() => cancelDownload()} className="btn-ghost-sm">{t("status.cancelDownload")}</button>
        </div>
      )}
      {showProgressBar && (
        <div className="download-progress">
          <div
            role="progressbar"
            aria-valuenow={determinateProgress !== null ? Math.round(determinateProgress) : undefined}
            aria-valuemin={0}
            aria-valuemax={100}
            className={`download-progress-track${determinateProgress !== null && determinateProgress >= 100 ? " progress-complete" : ""}`}
          >
            {determinateProgress !== null
              ? <div style={{ height: "100%", background: determinateProgress >= 100 ? "var(--color-success)" : "var(--color-accent)", borderRadius: 4, transition: "width 0.5s ease, background 0.3s ease", width: `${determinateProgress}%` }} />
              : <div className="download-pulse-bar" />}
          </div>
          {stage === "downloading" && (
<button onClick={() => cancelDownload()} className="btn-ghost-sm">{t("status.cancelDownload")}</button>
          )}
        </div>
      )}
    </>
  );
}
