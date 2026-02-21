import { useRef, useEffect } from "react";
import { Loader2 } from "lucide-react";

const EQ_BAR_COUNT = 5;
const EQ_BAR_DELAY_STEP = 0.12;

interface RecordingButtonProps {
  isRecording: boolean;
  isProcessing: boolean;
  isReady: boolean;
  onToggle: () => void;
}

export default function RecordingButton({
  isRecording, isProcessing, isReady, onToggle,
}: RecordingButtonProps) {
  const prevRecording = useRef(isRecording);

  useEffect(() => {
    prevRecording.current = isRecording;
  }, [isRecording]);

  const isIdle = !isRecording && !isProcessing;
  const label = isRecording ? "停止录音" : isProcessing ? "识别中" : "开始录音";

  return (
    <div className="record-btn-wrapper">
      {isRecording && <span className="recording-pulse-ring" />}
      <button
        key={isRecording ? "recording" : isProcessing ? "processing" : "idle"}
        className={`record-btn${isRecording !== prevRecording.current ? " animate-record-enter" : ""}`}
        aria-label={label}
        aria-pressed={isRecording}
        disabled={!isReady || isProcessing}
        onClick={onToggle}
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
          <div className="eq-bar-container">
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
  );
}
