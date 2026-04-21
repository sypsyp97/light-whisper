import { Loader2, Cpu, Globe } from "lucide-react";
import { useTranslation } from "react-i18next";
import Button from "@/components/ui/Button";
import Badge from "@/components/ui/Badge";
import ProgressBar from "@/components/ui/ProgressBar";

export interface StatusIndicatorProps {
  stage: "checking" | "loading" | "ready" | "error";
  isReady: boolean;
  engineLabel: string;
  device: string | null;
  gpuName: string | null;
  downloadProgress?: number;
  downloadMessage?: string | null;
  error: string | null;
  onRetry?: () => void;
  onCancelDownload?: () => void;
}

export default function StatusIndicator({
  stage,
  isReady,
  engineLabel,
  device,
  gpuName,
  downloadProgress,
  downloadMessage,
  error,
  onRetry,
  onCancelDownload,
}: StatusIndicatorProps) {
  const { t } = useTranslation();
  const deviceLabel =
    device === "cloud" ? t("status.online")
    : device === "cuda" || device === "gpu" ? gpuName ?? "GPU"
    : device ? "CPU"
    : null;

  return (
    <div className="lw-status" data-testid="main-status">
      {stage === "ready" && (
        <>
          <span className="lw-status-dot" />
          <span>{engineLabel}</span>
          {deviceLabel && (
            <Badge tone="neutral">
              {device === "cloud" ? <Globe size={10} /> : <Cpu size={10} />}
              {deviceLabel}
            </Badge>
          )}
        </>
      )}
      {stage === "loading" && (
        <>
          <Loader2 size={12} className="lw-spinner" />
          <span>{downloadMessage || t("status.modelLoading")}</span>
          {typeof downloadProgress === "number" && downloadProgress > 0 && (
            <div style={{ width: 100 }}><ProgressBar value={downloadProgress} /></div>
          )}
          {onCancelDownload && (
            <Button size="sm" variant="ghost" onClick={onCancelDownload}>
              {t("status.cancelDownload")}
            </Button>
          )}
        </>
      )}
      {stage === "checking" && (
        <>
          <span className="lw-status-dot lw-status-dot--loading" />
          <span>{t("status.preparing")}</span>
        </>
      )}
      {stage === "error" && (
        <>
          <span className="lw-status-dot lw-status-dot--error" />
          <span>{error ?? t("model.checkStatusFailed")}</span>
          {onRetry && (
            <Button
              size="sm"
              variant="ghost"
              onClick={onRetry}
              data-testid="main-retry-btn"
            >
              {t("common.retry")}
            </Button>
          )}
        </>
      )}
      {!isReady && stage !== "ready" && stage !== "error" && stage !== "loading" && stage !== "checking" && (
        <span>{t("status.preparing")}</span>
      )}
    </div>
  );
}
