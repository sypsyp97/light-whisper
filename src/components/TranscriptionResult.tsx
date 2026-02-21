import { Loader2, Copy, Check } from "lucide-react";

interface TranscriptionResultProps {
  text: string | null;
  isProcessing: boolean;
  copiedId: string | null;
  onCopy: (text: string, id: string) => void;
}

export default function TranscriptionResult({
  text, isProcessing, copiedId, onCopy,
}: TranscriptionResultProps) {
  return (
    <>
      {text && (
        <div style={{ marginBottom: 12 }} className="animate-slide-up">
          <div className="result-card">
            <div className="result-card-header">
              <span className="result-card-title">
                <span className="result-dot" />
                识别结果
              </span>
              <button aria-label="复制" className="icon-btn" style={{ padding: 6 }} onClick={() => onCopy(text, "original")}>
                {copiedId === "original" ? <Check size={12} /> : <Copy size={12} strokeWidth={1.5} />}
              </button>
            </div>
            <p className="result-card-body">{text}</p>
          </div>
        </div>
      )}
      {isProcessing && !text && (
        <div className="animate-fade-in">
          <div className="skeleton-shimmer skeleton-indicator">
            <Loader2 size={14} className="animate-spin icon-tertiary" />
            <span className="skeleton-indicator-text">正在识别语音...</span>
          </div>
        </div>
      )}
    </>
  );
}
