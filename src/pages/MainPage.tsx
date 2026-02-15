import { useState, useEffect, useRef } from "react";
import { Settings, Minus, X, Copy, Download, Cpu, Loader2, Check } from "lucide-react";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { copyToClipboard, hideMainWindow } from "@/api/tauri";
import TitleBar from "@/components/TitleBar";
import { PADDING } from "@/lib/constants";
const EQ_BAR_COUNT = 5;
const EQ_BAR_DELAY_STEP = 0.12; // seconds between each bar's animation

export default function MainPage({ onNavigate }: {
  onNavigate: (v: "main" | "settings") => void;
}) {
  const {
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, history, stage, isReady,
    device, gpuName, downloadProgress, downloadMessage,
    isDownloading, modelError,
    downloadModels: triggerDownload, cancelDownload, retryModel,
  } = useRecordingContext();

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const prevRecording = useRef(isRecording);

  // Reset error dismissed when error changes
  useEffect(() => {
    setErrorDismissed(false);
  }, [recordingError, modelError]);

  // Trigger record-enter animation on recording state change
  useEffect(() => {
    prevRecording.current = isRecording;
  }, [isRecording]);

  const handleCopy = async (text: string, id: string) => {
    try {
      await copyToClipboard(text);
      setCopiedId(id);
      toast.success("已复制到剪贴板");
      setTimeout(() => setCopiedId(null), 1500);
    } catch {
      toast.error("复制失败");
    }
  };

  const isIdle = !isRecording && !isProcessing;
  const recordBtnLabel = isRecording ? "停止录音" : isProcessing ? "识别中" : "开始录音";

  function getStatusText(): string {
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

  function getChipLabel(): string | null {
    if (stage === "downloading") return "下载中";
    if (stage === "loading") return downloadMessage || "加载中";
    return "准备中";
  }

  return (
    <div className="page-root">

      <TitleBar
        title="轻语 Whisper"
        leftAction={
          <button aria-label="设置" className="icon-btn plain" onClick={() => onNavigate("settings")}>
            <Settings size={13} strokeWidth={1.5} />
          </button>
        }
        rightActions={
          <>
            <button aria-label="最小化" className="icon-btn" onClick={() => getCurrentWindow().minimize()}>
              <Minus size={12} strokeWidth={1.5} />
            </button>
            <button aria-label="关闭" className="icon-btn" onClick={() => hideMainWindow()}>
              <X size={12} strokeWidth={1.5} />
            </button>
          </>
        }
      />

      {/* Recording zone */}
      <div className="recording-zone" style={{ padding: `16px ${PADDING}px 12px` }}>
        <div style={{ minHeight: 20 }}>
          {isReady && device && (
            <span className="chip animate-success">
              <Cpu size={10} strokeWidth={1.8} />
              {device === "cuda" || device === "gpu" ? (gpuName || "GPU") : "CPU"}
            </span>
          )}
          {!isReady && stage !== "need_download" && (
            <span className="chip animate-fade-in">
              <Loader2 size={10} className="animate-spin" />
              {getChipLabel()}
            </span>
          )}
        </div>

        <div style={{ position: "relative", display: "flex", alignItems: "center", justifyContent: "center" }}>
          {isRecording && (
            <span style={{
              position: "absolute", width: 50, height: 50, borderRadius: "50%",
              border: "2px solid var(--color-accent)",
              animation: "recording-pulse 1.8s ease-out infinite",
              pointerEvents: "none",
            }} />
          )}
          <button
            key={isRecording ? "recording" : isProcessing ? "processing" : "idle"}
            className={`record-btn${isRecording !== prevRecording.current ? " animate-record-enter" : ""}`}
            aria-label={recordBtnLabel}
            aria-pressed={isRecording}
            disabled={!isReady || isProcessing}
            onClick={() => { if (!isReady) return; isRecording ? stopRecording() : startRecording(); }}
            style={{
              border: isRecording ? "none" : "1px solid var(--color-border)",
              background: isRecording ? "var(--color-accent)" : isProcessing ? "var(--color-bg-tertiary)" : "var(--color-bg-elevated)",
              color: isRecording ? "white" : isProcessing ? "var(--color-text-tertiary)" : "var(--color-accent)",
              boxShadow: isRecording ? "0 0 0 4px rgba(193, 95, 60, 0.12), var(--shadow-lg)" : "var(--shadow-md)",
              cursor: !isReady ? "not-allowed" : isProcessing ? "wait" : "pointer",
              opacity: !isReady ? 0.4 : 1,
            }}
          >
            {isRecording && (
              <div style={{ display: "flex", alignItems: "center", gap: 2.5 }}>
                {Array.from({ length: EQ_BAR_COUNT }, (_, i) => (
                  <span key={i} className="eq-bar" style={{ animationDelay: `${i * EQ_BAR_DELAY_STEP}s` }} />
                ))}
              </div>
            )}
            {isIdle && isReady && (
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
                <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
                <line x1="12" x2="12" y1="19" y2="22" />
              </svg>
            )}
            {isProcessing && <Loader2 size={20} className="animate-spin" strokeWidth={1.5} />}
            {!isReady && isIdle && <Loader2 size={18} className="animate-spin" strokeWidth={1.5} />}
          </button>
        </div>

        <p aria-live="polite" className="status-text">
          {getStatusText()}
        </p>

        {stage === "need_download" && !isDownloading && (
          <button onClick={() => triggerDownload()} className="btn-primary" style={{ marginTop: 4, fontSize: 12, padding: "8px 16px" }}>
            <Download size={12} /> 开始下载
          </button>
        )}
        {stage === "need_download" && isDownloading && (
          <div style={{ marginTop: 4, display: "flex", flexDirection: "column", alignItems: "center", gap: 8, fontSize: 11, color: "var(--color-text-tertiary)" }}>
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
      </div>

      {/* Error */}
      {(recordingError || modelError) && !errorDismissed && (
        <div style={{ margin: `0 ${PADDING}px 8px`, flexShrink: 0 }} className="animate-shake">
          <div className="error-banner">
            <div className="error-banner-inner">
              <p style={{ fontSize: 12, color: "var(--color-error)", lineHeight: 1.6, flex: 1 }}>{recordingError || modelError}</p>
              <button onClick={() => setErrorDismissed(true)} aria-label="关闭" style={{ flexShrink: 0, padding: 2, background: "none", border: "none", cursor: "pointer", color: "var(--color-error)", opacity: 0.6, lineHeight: 1 }}>
                <X size={12} />
              </button>
            </div>
            {stage === "error" && <button onClick={retryModel} style={{ marginTop: 8, fontSize: 11, fontWeight: 500, color: "var(--color-error)", background: "none", border: "none", cursor: "pointer", textDecoration: "underline" }}>重试</button>}
          </div>
        </div>
      )}

      {/* Results */}
      <div className="results-area" style={{ padding: `0 ${PADDING}px 12px` }}>
        {transcriptionResult && (
          <div style={{ marginBottom: 12 }} className="animate-slide-up">
            <div className="result-card">
              <div className="result-card-header">
                <span className="result-card-title">
                  <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--color-accent)", flexShrink: 0 }} />
                  识别结果
                </span>
                <button aria-label="复制" className="icon-btn" style={{ padding: 6 }} onClick={() => handleCopy(transcriptionResult, "original")}>
                  {copiedId === "original" ? <Check size={12} /> : <Copy size={12} strokeWidth={1.5} />}
                </button>
              </div>
              <p className="result-card-body">{transcriptionResult}</p>
            </div>
          </div>
        )}
        {isProcessing && !transcriptionResult && (
          <div className="animate-fade-in">
            <div className="skeleton-shimmer" style={{ borderRadius: 8, padding: "10px 12px", display: "flex", alignItems: "center", gap: 10, background: "var(--color-bg-elevated)", border: "1px solid var(--color-border-subtle)" }}>
              <Loader2 size={14} className="animate-spin" style={{ color: "var(--color-text-tertiary)" }} />
              <span style={{ fontSize: 12, color: "var(--color-text-tertiary)" }}>正在识别语音...</span>
            </div>
          </div>
        )}
        {/* History (skip the first item if it matches current result) */}
        {history.length > 0 && (
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {history
              .filter((item, idx) => !(idx === 0 && transcriptionResult && item.text === transcriptionResult))
              .map((item) => (
                <div key={item.id} style={{
                  padding: "8px 10px",
                  borderRadius: 6,
                  background: "var(--color-bg-elevated)",
                  border: "1px solid var(--color-border-subtle)",
                  display: "flex",
                  alignItems: "flex-start",
                  gap: 8,
                }}>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <p style={{ fontSize: 12, color: "var(--color-text-secondary)", lineHeight: 1.5, margin: 0, wordBreak: "break-all" }}>{item.text}</p>
                    <span style={{ fontSize: 10, color: "var(--color-text-tertiary)", marginTop: 2, display: "block" }}>
                      {new Date(item.timestamp).toLocaleTimeString()}
                    </span>
                  </div>
                  <button aria-label="复制" className="icon-btn" style={{ padding: 4, flexShrink: 0 }} onClick={() => handleCopy(item.text, item.id)}>
                    {copiedId === item.id ? <Check size={11} /> : <Copy size={11} strokeWidth={1.5} />}
                  </button>
                </div>
              ))}
          </div>
        )}
      </div>

    </div>
  );
}
