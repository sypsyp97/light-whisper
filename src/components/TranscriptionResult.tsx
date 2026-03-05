import { useRef, useCallback } from "react";
import { Loader2, Copy, Check } from "lucide-react";

interface TranscriptionResultProps {
  text: string | null;
  originalText: string | null;
  isProcessing: boolean;
  copiedId: string | null;
  onCopy: (text: string, id: string) => void;
  onTextChange?: (newText: string) => void;
  durationSec: number | null;
  charCount: number | null;
}

function formatStats(charCount: number, durationSec: number): string {
  const cpm = Math.round((charCount / durationSec) * 60);
  return `${charCount}字 · ${durationSec.toFixed(1)}秒 · ${cpm}字/分钟`;
}

export default function TranscriptionResult({
  text, originalText, isProcessing, copiedId, onCopy, onTextChange, durationSec, charCount,
}: TranscriptionResultProps) {
  const bodyRef = useRef<HTMLParagraphElement>(null);
  const showStats = text && durationSec && durationSec > 0 && charCount;

  const handleBlur = useCallback(() => {
    const edited = bodyRef.current?.textContent?.trim() ?? "";
    if (edited && originalText && edited !== originalText) {
      onTextChange?.(edited);
    }
  }, [originalText, onTextChange]);

  const handleCopy = useCallback(() => {
    const currentText = bodyRef.current?.textContent?.trim() ?? text ?? "";
    onCopy(currentText, "original");
  }, [text, onCopy]);

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
              <button aria-label="复制" className="icon-btn" style={{ padding: 6 }} onClick={handleCopy}>
                {copiedId === "original" ? <Check size={12} /> : <Copy size={12} strokeWidth={1.5} />}
              </button>
            </div>
            <p
              ref={bodyRef}
              className="result-card-body"
              contentEditable
              suppressContentEditableWarning
              onBlur={handleBlur}
              style={{ outline: "none", borderRadius: 4, padding: "2px 4px", transition: "background 0.15s" }}
            >
              {text}
            </p>
            {showStats && (
              <p className="result-card-stats">{formatStats(charCount, durationSec)}</p>
            )}
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
