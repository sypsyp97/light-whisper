import { useState } from "react";
import { Settings, Minus, X, Copy, Download, Cpu, Loader2, Check } from "lucide-react";
import { toast } from "sonner";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { useWindowDrag } from "@/hooks/useWindowDrag";
import { copyToClipboard } from "@/api/clipboard";
import { hideMainWindow } from "@/api/window";

const PADDING = 24;

const iconBtnStyle: React.CSSProperties = {
  padding: 8,
  borderRadius: 4,
  border: "none",
  background: "transparent",
  color: "var(--color-text-tertiary)",
  cursor: "pointer",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};

export default function MainPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const {
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, stage, isReady,
    device, gpuName, downloadProgress, downloadMessage,
    isDownloading, modelError,
    downloadModels: triggerDownload, cancelDownload, retryModel,
  } = useRecordingContext();

  const { startDrag } = useWindowDrag();
  const [copiedId, setCopiedId] = useState<string | null>(null);

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

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh", width: "100vw", userSelect: "none", overflow: "hidden", color: "var(--color-text-primary)" }}>

      {/* Title bar */}
      <header onMouseDown={startDrag} style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: `0 ${PADDING - 8}px`, height: 36, flexShrink: 0, borderBottom: "1px solid var(--color-border-subtle)", background: "var(--color-bg-overlay)", backdropFilter: "blur(8px)" }}>
        <span style={{ fontSize: 13, fontWeight: 600, letterSpacing: "0.01em", fontFamily: "var(--font-display)", marginLeft: 8 }}>
          轻语 Whisper
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 2 }} onMouseDown={e => e.stopPropagation()}>
          <button onClick={() => import("@tauri-apps/api/window").then(m => m.getCurrentWindow().minimize())} style={iconBtnStyle}><Minus size={12} strokeWidth={1.5} /></button>
          <button onClick={() => hideMainWindow()} style={iconBtnStyle}><X size={12} strokeWidth={1.5} /></button>
        </div>
      </header>

      {/* Recording zone */}
      <div style={{ flexShrink: 0, display: "flex", flexDirection: "column", alignItems: "center", padding: `28px ${PADDING}px 20px`, gap: 16 }}>
        <div style={{ minHeight: 20 }}>
          {isReady && device && (
            <span className="chip">
              <Cpu size={10} strokeWidth={1.8} />
              {device === "cuda" || device === "gpu" ? (gpuName || "GPU") : "CPU"}
            </span>
          )}
          {!isReady && stage !== "need_download" && (
            <span className="chip">
              <Loader2 size={10} className="animate-spin" />
              {stage === "downloading" ? "下载中" : stage === "loading" ? "加载中" : "准备中"}
            </span>
          )}
        </div>

        <button
          disabled={!isReady || isProcessing}
          onClick={() => { if (!isReady) return; isRecording ? stopRecording() : startRecording(); }}
          style={{
            width: 64, height: 64, borderRadius: "50%",
            display: "flex", alignItems: "center", justifyContent: "center",
            border: isRecording ? "none" : "1px solid var(--color-border)",
            background: isRecording ? "var(--color-accent)" : isProcessing ? "var(--color-bg-tertiary)" : "var(--color-bg-elevated)",
            color: isRecording ? "white" : isProcessing ? "var(--color-text-tertiary)" : "var(--color-accent)",
            boxShadow: isRecording ? "0 0 0 4px rgba(193, 95, 60, 0.12), var(--shadow-lg)" : "var(--shadow-md)",
            cursor: !isReady ? "not-allowed" : isProcessing ? "wait" : "pointer",
            opacity: !isReady ? 0.4 : 1, transition: "all 0.3s ease",
          }}
        >
          {isRecording && (
            <div style={{ display: "flex", alignItems: "center", gap: 2.5 }}>
              {[0, 1, 2, 3, 4].map(i => (
                <span key={i} style={{ width: 2, height: 12, borderRadius: 1, background: "white", animation: "eq 0.9s ease-in-out infinite alternate", animationDelay: `${i * 0.12}s` }} />
              ))}
            </div>
          )}
          {isIdle && isReady && (
            <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
              <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
              <line x1="12" x2="12" y1="19" y2="22" />
            </svg>
          )}
          {isProcessing && <Loader2 size={20} className="animate-spin" strokeWidth={1.5} />}
          {!isReady && isIdle && <Loader2 size={18} className="animate-spin" strokeWidth={1.5} />}
        </button>

        <p style={{ fontSize: 12, lineHeight: 1.6, color: "var(--color-text-tertiary)", textAlign: "center" }}>
          {isRecording ? "正在聆听..." : isProcessing ? "识别中..." : isReady ? "点击开始录音"
            : stage === "downloading" ? (downloadProgress > 1 ? `模型下载中 ${Math.round(downloadProgress ?? 0)}%` : downloadMessage ?? "模型下载准备中...")
            : stage === "need_download" ? "需要下载模型" : stage === "loading" ? "模型加载中..." : "准备中..."}
        </p>

        {stage === "need_download" && !isDownloading && (
          <button onClick={() => triggerDownload()} className="btn-primary" style={{ marginTop: 4, fontSize: 12, padding: "8px 16px" }}>
            <Download size={12} /> 开始下载
          </button>
        )}
        {stage === "need_download" && isDownloading && (
          <div style={{ marginTop: 4, display: "flex", flexDirection: "column", alignItems: "center", gap: 8, fontSize: 11, color: "var(--color-text-tertiary)" }}>
            <span>另一个模型正在下载中</span>
            <button onClick={() => cancelDownload()} style={{ fontSize: 11, padding: "4px 10px", borderRadius: 4, border: "1px solid var(--color-border-subtle)", background: "transparent", color: "var(--color-text-tertiary)", cursor: "pointer" }}>取消下载</button>
          </div>
        )}
        {stage === "downloading" && (
          <div style={{ width: "100%", maxWidth: 200, marginTop: 4, display: "flex", flexDirection: "column", alignItems: "center", gap: 10 }}>
            <div style={{ width: "100%", borderRadius: 4, height: 4, background: "var(--color-border)", overflow: "hidden" }}>
              {downloadProgress > 1
                ? <div style={{ height: "100%", background: "var(--color-accent)", borderRadius: 4, transition: "width 0.5s ease", width: `${downloadProgress ?? 0}%` }} />
                : <div style={{ height: "100%", width: "50%", background: "var(--color-accent)", borderRadius: 4, animation: "downloadPulse 1.2s ease-in-out infinite" }} />}
            </div>
            <button onClick={() => cancelDownload()} style={{ fontSize: 11, padding: "4px 10px", borderRadius: 4, border: "1px solid var(--color-border-subtle)", background: "transparent", color: "var(--color-text-tertiary)", cursor: "pointer" }}>取消下载</button>
          </div>
        )}
      </div>

      {/* Error */}
      {(recordingError || modelError) && (
        <div style={{ margin: `0 ${PADDING}px 12px`, flexShrink: 0 }}>
          <div style={{ borderRadius: 6, padding: 14, background: "var(--color-error-bg)", border: "1px solid rgba(192, 57, 43, 0.15)" }}>
            <p style={{ fontSize: 12, color: "var(--color-error)", lineHeight: 1.6 }}>{recordingError || modelError}</p>
            {stage === "error" && <button onClick={retryModel} style={{ marginTop: 8, fontSize: 11, fontWeight: 500, color: "var(--color-error)", background: "none", border: "none", cursor: "pointer", textDecoration: "underline" }}>重试</button>}
          </div>
        </div>
      )}

      {/* Results */}
      <div style={{ flex: 1, overflowY: "auto", padding: `0 ${PADDING}px 12px`, minHeight: 0 }}>
        {transcriptionResult && (
          <div style={{ marginBottom: 12 }} className="animate-fade-in">
            <div style={{ borderRadius: 8, background: "var(--color-bg-elevated)", border: "1px solid var(--color-border-subtle)", boxShadow: "var(--shadow-xs)" }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "10px 14px", borderBottom: "1px solid var(--color-border-subtle)" }}>
                <span style={{ fontSize: 10, fontWeight: 500, letterSpacing: "0.06em", textTransform: "uppercase", color: "var(--color-text-tertiary)" }}>识别结果</span>
                <button onClick={() => handleCopy(transcriptionResult, "original")} style={{ ...iconBtnStyle, padding: 6 }}>
                  {copiedId === "original" ? <Check size={12} /> : <Copy size={12} strokeWidth={1.5} />}
                </button>
              </div>
              <p style={{ padding: "12px 14px", fontSize: 13, lineHeight: 1.8, color: "var(--color-text-secondary)" }}>{transcriptionResult}</p>
            </div>
          </div>
        )}
        {isProcessing && !transcriptionResult && (
          <div className="animate-fade-in">
            <div style={{ borderRadius: 8, padding: "12px 14px", display: "flex", alignItems: "center", gap: 10, background: "var(--color-bg-elevated)", border: "1px solid var(--color-border-subtle)" }}>
              <Loader2 size={14} className="animate-spin" style={{ color: "var(--color-text-tertiary)" }} />
              <span style={{ fontSize: 12, color: "var(--color-text-tertiary)" }}>正在识别语音...</span>
            </div>
          </div>
        )}
        {!isProcessing && !transcriptionResult && (
          <div style={{ display: "flex", alignItems: "center", justifyContent: "center", padding: "20px 0" }}>
            <p style={{ fontSize: 13, color: "var(--color-text-tertiary)" }}>转录结果将显示在这里</p>
          </div>
        )}
      </div>

      {/* Bottom toolbar */}
      <div style={{ flexShrink: 0, height: 44, display: "flex", alignItems: "center", padding: `0 ${PADDING - 12}px`, borderTop: "1px solid var(--color-border-subtle)" }}>
        <button onClick={() => onNavigate("settings")} style={{ display: "flex", alignItems: "center", gap: 6, padding: "8px 12px", borderRadius: 6, border: "none", background: "transparent", fontSize: 12, color: "var(--color-text-tertiary)", cursor: "pointer" }}>
          <Settings size={14} strokeWidth={1.5} /> 设置
        </button>
      </div>

      <style>{`
        @keyframes eq { 0% { height: 3px; } 100% { height: 12px; } }
        @keyframes downloadPulse { 0% { transform: translateX(-60%); } 100% { transform: translateX(120%); } }
      `}</style>
    </div>
  );
}
