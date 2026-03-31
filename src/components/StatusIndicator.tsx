import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Cpu, Globe, Loader2, Download } from "lucide-react";
import type { ModelStage } from "@/hooks/useModelStatus";

interface StatusIndicatorProps {
  stage: ModelStage;
  isReady: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  device: string | null;
  gpuName: string | null;
  downloadProgress: number;
  downloadMessage: string | null;
  isDownloading: boolean;
  downloadModels: () => void;
  cancelDownload: () => void;
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
  isRecording: boolean,
  isProcessing: boolean,
  isReady: boolean,
  stage: ModelStage,
  downloadProgress: number,
  downloadMessage: string | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (isRecording) return t("status.listening");
  if (isProcessing) return t("status.recognizing");
  if (isReady) return t("status.clickToStart");
  if (stage === "downloading") {
    return downloadProgress > 1
      ? t("status.downloadingModel", { progress: Math.round(downloadProgress) })
      : (downloadMessage ?? t("status.downloadPreparing"));
  }
  if (stage === "need_download") return t("status.needDownload");
  if (stage === "loading") return downloadMessage || t("status.modelLoading");
  return t("status.preparing");
}

function getChipLabel(stage: ModelStage, downloadMessage: string | null, t: (key: string, options?: Record<string, unknown>) => string): string {
  if (stage === "downloading") return t("status.chipDownloading");
  if (stage === "loading") return downloadMessage || t("status.chipLoading");
  return t("status.chipPreparing");
}

export default function StatusIndicator({
  stage, isReady, isRecording, isProcessing,
  device, gpuName, downloadProgress, downloadMessage,
  isDownloading, downloadModels, cancelDownload, children,
}: StatusIndicatorProps) {
  const { t } = useTranslation();
  const showProgressBar = stage === "downloading" || stage === "loading";
  const loadingProgress = stage === "loading" ? getLoadingProgress(downloadMessage) : null;
  const determinateProgress = stage === "downloading"
    ? (downloadProgress > 1 ? downloadProgress : null)
    : loadingProgress;

  return (
    <>
      <div className="chip-container">
        {device && (
          <span className={`chip ${isReady ? "animate-success" : "animate-fade-in"}`}>
            {device === "cloud" ? <Globe size={10} strokeWidth={1.8} /> : <Cpu size={10} strokeWidth={1.8} />}
            {getDeviceLabel(device, gpuName, t)}
          </span>
        )}
        {!isReady && stage !== "need_download" && (
          <span className="chip animate-fade-in">
            <Loader2 size={10} className="animate-spin" />
            {getChipLabel(stage, downloadMessage, t)}
          </span>
        )}
      </div>

      {children}

      <p
        key={isRecording ? "recording" : isProcessing ? "processing" : isReady ? "ready" : stage}
        aria-live="polite"
        className="status-text animate-text-swap"
      >
        {getStatusText(isRecording, isProcessing, isReady, stage, downloadProgress, downloadMessage, t)}
      </p>

      {stage === "need_download" && !isDownloading && (
        <button onClick={() => downloadModels()} className="btn-primary" style={{ marginTop: 4, fontSize: 12, padding: "8px 16px" }}>
          <Download size={12} /> {t("status.startDownload")}
        </button>
      )}
      {stage === "need_download" && isDownloading && (
        <div className="download-info">
          <span>{t("status.anotherDownloading")}</span>
          <button onClick={() => cancelDownload()} className="btn-ghost" style={{ fontSize: 11, padding: "4px 10px" }}>{t("status.cancelDownload")}</button>
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
            <button onClick={() => cancelDownload()} className="btn-ghost" style={{ fontSize: 11, padding: "4px 10px" }}>{t("status.cancelDownload")}</button>
          )}
        </div>
      )}
    </>
  );
}
