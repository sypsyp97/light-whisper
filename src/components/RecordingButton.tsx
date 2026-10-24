import { useRef, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Loader2, Mic, Undo2 } from "lucide-react";

const EQ_BAR_COUNT = 5;

interface RecordingButtonProps {
  isStarting: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  isReady: boolean;
  onToggle: () => void;
  onCancelProcessing?: () => void;
}

export default function RecordingButton({
  isStarting, isRecording, isProcessing, isReady, onToggle, onCancelProcessing,
}: RecordingButtonProps) {
  const { t } = useTranslation();
  const isActive = isStarting || isRecording;
  const [bars, setBars] = useState<number[]>(Array(EQ_BAR_COUNT).fill(0));
  const latestSession = useRef(0);

  useEffect(() => {
    if (!isRecording) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<{ sessionId: number; bars: number[] }>("waveform", ({ payload }) => {
      if (disposed || !Number.isSafeInteger(payload.sessionId) || payload.sessionId <= 0 || payload.sessionId < latestSession.current) return;
      latestSession.current = payload.sessionId;
      setBars(Array.from({ length: EQ_BAR_COUNT }, (_, index) => {
        const value = payload.bars[Math.round(index * (payload.bars.length - 1) / (EQ_BAR_COUNT - 1))];
        return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
      }));
    }).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    }).catch(() => undefined);
    return () => { disposed = true; unlisten?.(); };
  }, [isRecording]);

  const isIdle = !isStarting && !isRecording && !isProcessing;
  const canCancelProcessing = isProcessing && Boolean(onCancelProcessing);
  const label = isStarting
    ? t("recording.cancelStart")
    : isRecording
      ? t("recording.stop")
      : isProcessing
        ? t(canCancelProcessing ? "recording.useOriginal" : "recording.processing")
        : t("recording.start");

  return (
    <div className="record-btn-wrapper">
      {isRecording && (
        <>
          <span className="recording-pulse-ring" />
          <span className="recording-pulse-ring-outer" />
        </>
      )}
      <button
        className="record-btn"
        aria-label={label}
        aria-pressed={isActive}
        disabled={!canCancelProcessing && (!isReady || isProcessing)}
        onClick={canCancelProcessing ? onCancelProcessing : onToggle}
        title={label}
        style={{
          border: "1px solid",
          borderColor: isRecording ? "transparent" : "var(--color-border)",
          background: isRecording ? "var(--color-accent)" : isProcessing ? "var(--color-bg-tertiary)" : "var(--color-bg-elevated)",
          color: isRecording ? "white" : isProcessing ? "var(--color-text-tertiary)" : "var(--color-accent)",
          boxShadow: isRecording ? "var(--shadow-record-ring), var(--shadow-lg)" : "var(--shadow-md)",
          cursor: canCancelProcessing ? "pointer" : !isReady ? "not-allowed" : isProcessing ? "wait" : "pointer",
          opacity: !isReady && !canCancelProcessing ? 0.4 : 1,
        }}
      >
        <span className="record-icon" data-visible={isRecording} aria-hidden="true">
          <span className="eq-bar-container">
            {bars.map((level, i) => (
              <span key={i} className="eq-bar" style={{ transform: `scaleY(${Math.max(0.12, level)})` }} />
            ))}
          </span>
        </span>
        <span className="record-icon" data-visible={isIdle && isReady} aria-hidden="true"><Mic size={18} /></span>
        <span className="record-icon" data-visible={isStarting || isProcessing || (!isReady && isIdle)} aria-hidden="true">
          {canCancelProcessing ? <Undo2 size={18} /> : <Loader2 size={18} className="animate-spin" />}
        </span>
      </button>
    </div>
  );
}
