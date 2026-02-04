import { useState, useEffect, useRef } from "react";
import {
  Settings,
  Minus,
  X,
  Copy,
  Download,
  Cpu,
  Loader2,
  Check,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { useWindowDrag } from "@/hooks/useWindowDrag";
import { copyToClipboard } from "@/api/clipboard";
import { hideMainWindow } from "@/api/window";

/* ───── Typing animation ───── */
function TypingText({ text, className }: { text: string; className?: string }) {
  const [displayed, setDisplayed] = useState("");
  const prevTextRef = useRef("");

  useEffect(() => {
    if (text === prevTextRef.current) return;
    prevTextRef.current = text;
    setDisplayed("");
    if (!text) return;

    let idx = 0;
    const interval = setInterval(() => {
      idx++;
      setDisplayed(text.slice(0, idx));
      if (idx >= text.length) clearInterval(interval);
    }, 18);
    return () => clearInterval(interval);
  }, [text]);

  return <span className={className}>{displayed}</span>;
}

/* ───── Main page — 400×500 ───── */
export default function MainPage({ onNavigate }: { onNavigate: (v: "main" | "settings") => void }) {
  const {
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    recordingError,
    transcriptionResult,
    stage,
    isReady,
    device,
    gpuName,
    downloadProgress,
    downloadMessage,
    isDownloading,
    modelError,
    downloadModels: triggerDownload,
    cancelDownload,
    retryModel,
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
    <div className="flex flex-col h-screen w-screen select-none overflow-hidden text-[var(--color-text-primary)]">

      {/* ═══ Title bar — 32 px ═══ */}
      <header
        className="relative z-20 flex items-center justify-between px-3 h-8 shrink-0 border-b border-[var(--color-border-subtle)] bg-[var(--color-bg-overlay)] backdrop-blur-sm"
        onMouseDown={startDrag}
      >
        <span
          className="text-[12px] font-semibold tracking-[0.02em] text-[var(--color-text-primary)]"
          style={{ fontFamily: "var(--font-display)" }}
        >
          轻语 Whisper
        </span>

        <div className="flex items-center" onMouseDown={(e) => e.stopPropagation()}>
          <button
            onClick={() => import("@tauri-apps/api/window").then((m) => m.getCurrentWindow().minimize())}
            className="p-1.5 rounded text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)] hover:bg-[var(--color-accent-muted)] transition-colors"
            title="最小化"
          >
            <Minus size={11} strokeWidth={1.6} />
          </button>
          <button
            onClick={() => hideMainWindow()}
            className="p-1.5 rounded text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] hover:bg-[var(--color-error-bg)] transition-colors"
            title="隐藏到托盘"
          >
            <X size={11} strokeWidth={1.6} />
          </button>
        </div>
      </header>

      {/* ═══ Recording zone ═══ */}
      <div className="shrink-0 flex flex-col items-center pt-8 pb-5 px-4 gap-4">

        {/* Device chip — fixed row */}
        <div className="min-h-[18px]">
          {isReady && device && (
            <span className="chip">
              <Cpu size={9} strokeWidth={1.8} />
              {device === "cuda" || device === "gpu" ? (gpuName || "GPU") : "CPU"}
            </span>
          )}
          {!isReady && stage !== "need_download" && (
            <span className="chip">
              <Loader2 size={9} className="animate-spin" />
              {stage === "downloading" ? "下载中" : stage === "loading" ? "加载中" : "准备中"}
            </span>
          )}
        </div>

        {/* Recording button — no absolute overflow */}
        <button
          disabled={!isReady || isProcessing}
          onClick={() => {
            if (!isReady) return;
            if (isRecording) stopRecording();
            else startRecording();
          }}
          className={cn(
            "w-14 h-14 rounded-full flex items-center justify-center",
            "transition-all duration-300 ease-out",
            "focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-accent)]",

            isIdle && isReady &&
              "bg-[var(--color-bg-elevated)] border border-[var(--color-border)] shadow-[var(--shadow-md)] hover:shadow-[var(--shadow-lg)] hover:scale-[1.04] active:scale-[0.96] text-[var(--color-accent)]",

            isRecording &&
              "bg-[var(--color-accent)] border border-transparent shadow-[0_0_0_4px_rgba(193,95,60,0.12),var(--shadow-lg)] scale-[1.02] text-white",

            isProcessing &&
              "bg-[var(--color-bg-tertiary)] border border-[var(--color-border)] text-[var(--color-text-tertiary)] cursor-wait",

            !isReady && "opacity-40 cursor-not-allowed",
          )}
        >
          {isRecording && (
            <div className="flex items-center gap-[2.5px]">
              {[0, 1, 2, 3, 4].map((i) => (
                <span
                  key={i}
                  className="w-[2px] rounded-full bg-white animate-[eq_0.9s_ease-in-out_infinite_alternate]"
                  style={{ animationDelay: `${i * 0.12}s`, height: "12px" }}
                />
              ))}
            </div>
          )}
          {isIdle && isReady && (
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
              <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
              <line x1="12" x2="12" y1="19" y2="22" />
            </svg>
          )}
          {isProcessing && <Loader2 size={18} className="animate-spin" strokeWidth={1.6} />}
          {!isReady && isIdle && <Loader2 size={16} className="animate-spin" strokeWidth={1.5} />}
        </button>

        {/* Status line */}
        <p className="text-[11px] leading-[1.6] text-[var(--color-text-tertiary)] text-center">
          {isRecording
            ? "正在聆听 · 按 F2 结束"
            : isProcessing
              ? "识别中..."
            : isReady
                  ? "按 F2 开始录音"
                  : stage === "downloading"
                    ? (downloadProgress > 1
                      ? `模型下载中 ${Math.round(downloadProgress ?? 0)}%`
                      : downloadMessage ?? "模型下载准备中...")
                    : stage === "need_download"
                      ? "需要下载模型"
                      : stage === "loading"
                        ? "模型加载中..."
                        : "准备中..."}
        </p>

        {/* Download button */}
        {stage === "need_download" && !isDownloading && (
          <button
            onClick={() => triggerDownload()}
            className="btn-primary mt-3 text-[11px] py-1 px-3"
          >
            <Download size={11} />
            开始下载
          </button>
        )}
        {stage === "need_download" && isDownloading && (
          <div className="mt-3 flex flex-col items-center gap-2 text-[10px] text-[var(--color-text-tertiary)]">
            <span>另一个模型正在下载中</span>
            <button
              onClick={() => cancelDownload()}
              className="text-[10px] px-2 py-0.5 rounded-sm border border-[var(--color-border-subtle)] hover:bg-[var(--color-bg-tertiary)] transition-colors"
            >
              取消下载
            </button>
          </div>
        )}

        {/* Download progress bar */}
        {stage === "downloading" && (
          <div className="w-full max-w-[220px] mt-3 flex flex-col items-center gap-2">
            <div className="w-full rounded-full h-1 bg-[var(--color-border)] overflow-hidden">
              {downloadProgress > 1 ? (
                <div
                  className="h-full bg-[var(--color-accent)] rounded-full transition-all duration-500 ease-out"
                  style={{ width: `${downloadProgress ?? 0}%` }}
                />
              ) : (
                <div className="h-full w-1/2 bg-[var(--color-accent)] rounded-full animate-[downloadPulse_1.2s_ease-in-out_infinite]" />
              )}
            </div>
            <button
              onClick={() => cancelDownload()}
              className="text-[10px] px-2 py-0.5 rounded-sm border border-[var(--color-border-subtle)] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] transition-colors"
            >
              取消下载
            </button>
          </div>
        )}

        {stage !== "downloading" && isDownloading && (
          <p className="text-[10px] text-[var(--color-text-tertiary)]">
            后台正在下载模型，可继续使用当前引擎
          </p>
        )}
      </div>

      {/* ═══ Error ═══ */}
      {(recordingError || modelError) && (
        <div className="mx-4 mb-3 shrink-0">
          <div className="rounded-sm p-3 bg-[var(--color-error-bg)] border border-[var(--color-error)]/15">
            <p className="text-[11px] text-[var(--color-error)] leading-relaxed">
              {recordingError || modelError}
            </p>
            {stage === "error" && (
              <button onClick={retryModel} className="mt-2 text-[10px] font-medium text-[var(--color-error)] underline">
                重试
              </button>
            )}
          </div>
        </div>
      )}

      {/* ═══ Results ═══ */}
      <div className="flex-1 overflow-y-auto px-4 pb-3 min-h-0">

        {/* Original transcription */}
        {transcriptionResult && (
          <div className="mb-3 animate-fade-in">
            <div className="rounded-sm bg-[var(--color-bg-elevated)] border border-[var(--color-border-subtle)] shadow-[var(--shadow-xs)]">
              <div className="flex items-center justify-between px-4 py-2 border-b border-[var(--color-border-subtle)]">
                <span className="text-[9px] font-medium tracking-[0.08em] uppercase text-[var(--color-text-tertiary)]">
                  原始识别
                </span>
                <button
                  onClick={() => handleCopy(transcriptionResult, "original")}
                  className="p-1 rounded hover:bg-[var(--color-accent-muted)] transition-colors text-[var(--color-text-tertiary)] hover:text-[var(--color-accent)]"
                >
                  {copiedId === "original" ? <Check size={11} /> : <Copy size={11} strokeWidth={1.6} />}
                </button>
              </div>
              <p className="px-4 py-3 text-[12px] leading-[1.8] text-[var(--color-text-secondary)]">
                {transcriptionResult}
              </p>
            </div>
          </div>
        )}

        {/* Processing indicator */}
        {isProcessing && !transcriptionResult && (
          <div className="animate-fade-in">
            <div className="rounded-sm px-4 py-3 flex items-center gap-2 bg-[var(--color-bg-elevated)] border border-[var(--color-border-subtle)]">
              <Loader2 size={12} className="animate-spin text-[var(--color-text-tertiary)]" />
              <span className="text-[11px] text-[var(--color-text-tertiary)]">正在识别语音...</span>
            </div>
          </div>
        )}

        {/* Empty state */}
        {!isProcessing && !transcriptionResult && (
          <div className="flex flex-col items-center justify-center py-8 gap-2">
            <p className="text-[12px] text-[var(--color-text-tertiary)]">
              转录结果将显示在这里
            </p>
          </div>
        )}
      </div>

      {/* ═══ Bottom toolbar ═══ */}
      <div className="shrink-0 h-9 flex items-center justify-between px-3 border-t border-[var(--color-border-subtle)]">
        <button
          onClick={() => onNavigate("settings")}
          className="flex items-center gap-1 px-2 py-1 rounded-md text-[10px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)] hover:bg-[var(--color-accent-muted)] transition-colors"
        >
          <Settings size={11} strokeWidth={1.5} />
          设置
        </button>
        <span className="text-[9px] text-[var(--color-text-tertiary)] font-mono">F2</span>
      </div>

      <style>{`
        @keyframes eq {
          0% { height: 3px; }
          100% { height: 12px; }
        }
        @keyframes downloadPulse {
          0% { transform: translateX(-60%); }
          100% { transform: translateX(120%); }
        }
      `}</style>
    </div>
  );
}
