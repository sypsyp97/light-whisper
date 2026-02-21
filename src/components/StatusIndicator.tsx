import type { ReactNode } from "react";
import { Cpu, Loader2, Download } from "lucide-react";
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

function getStatusText(
  isRecording: boolean,
  isProcessing: boolean,
  isReady: boolean,
  stage: ModelStage,
  downloadProgress: number,
  downloadMessage: string | null,
): string {
  if (isRecording) return "正在聆听...";
  if (isProcessing) return "识别中...";
  if (isReady) return "点击开始录音";
  if (stage === "downloading") {
    return downloadProgress > 1
      ? `模型下载中 ${Math.round(downloadProgress)}%`
      : (downloadMessage ?? "模型下载准备中...");
  }
  if (stage === "need_download") return "需要下载模型";
  if (stage === "loading") return downloadMessage || "模型加载中...";
  return "准备中...";
}

function getChipLabel(stage: ModelStage, downloadMessage: string | null): string {
  if (stage === "downloading") return "下载中";
  if (stage === "loading") return downloadMessage || "加载中";
  return "准备中";
}

export default function StatusIndicator({
  stage, isReady, isRecording, isProcessing,
  device, gpuName, downloadProgress, downloadMessage,
  isDownloading, downloadModels, cancelDownload, children,
}: StatusIndicatorProps) {
  return (
    <>
      <div className="chip-container">
        {isReady && device && (
          <span className="chip animate-success">
            <Cpu size={10} strokeWidth={1.8} />
            {device === "cuda" || device === "gpu" ? (gpuName || "GPU") : "CPU"}
          </span>
        )}
        {!isReady && stage !== "need_download" && (
          <span className="chip animate-fade-in">
            <Loader2 size={10} className="animate-spin" />
            {getChipLabel(stage, downloadMessage)}
          </span>
        )}
      </div>

      {children}

      <p aria-live="polite" className="status-text">
        {getStatusText(isRecording, isProcessing, isReady, stage, downloadProgress, downloadMessage)}
      </p>

      {stage === "need_download" && !isDownloading && (
        <button onClick={() => downloadModels()} className="btn-primary" style={{ marginTop: 4, fontSize: 12, padding: "8px 16px" }}>
          <Download size={12} /> 开始下载
        </button>
      )}
      {stage === "need_download" && isDownloading && (
        <div className="download-info">
          <span>另一个模型正在下载中</span>
          <button onClick={() => cancelDownload()} className="btn-ghost" style={{ fontSize: 11, padding: "4px 10px" }}>取消下载</button>
        </div>
      )}
      {stage === "downloading" && (
        <div className="download-progress">
          <div
            role="progressbar"
            aria-valuenow={downloadProgress > 1 ? Math.round(downloadProgress) : undefined}
            aria-valuemin={0}
            aria-valuemax={100}
            className="download-progress-track"
          >
            {downloadProgress > 1
              ? <div style={{ height: "100%", background: downloadProgress >= 100 ? "var(--color-success)" : "var(--color-accent)", borderRadius: 4, transition: "width 0.5s ease, background 0.3s ease", width: `${downloadProgress ?? 0}%` }} />
              : <div className="download-pulse-bar" />}
          </div>
          <button onClick={() => cancelDownload()} className="btn-ghost" style={{ fontSize: 11, padding: "4px 10px" }}>取消下载</button>
        </div>
      )}
    </>
  );
}
