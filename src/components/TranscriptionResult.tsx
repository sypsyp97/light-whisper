import { useRef, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Copy, Check } from "lucide-react";
import type { EditGrabStatus } from "@/types";

interface TranscriptionResultProps {
  text: string | null;
  originalText: string | null;
  isProcessing: boolean;
  copiedId: string | null;
  onCopy: (text: string, id: string) => void;
  onDraftChange?: (newText: string) => void;
  onTextChange?: (newText: string) => void;
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage?: string | null;
  editGrabStatus?: EditGrabStatus | null;
}

export default function TranscriptionResult({
  text,
  originalText,
  isProcessing,
  copiedId,
  onCopy,
  onDraftChange,
  onTextChange,
  durationSec,
  charCount,
  detectedLanguage,
  editGrabStatus,
}: TranscriptionResultProps) {
  const { t } = useTranslation();
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const [draftText, setDraftText] = useState(text ?? "");
  const hasResult = text !== null;
  const hasStats = !!text && durationSec && durationSec > 0 && charCount;
  const editGrabHintKey =
    editGrabStatus === "timeout" || editGrabStatus === "empty"
      ? `result.editGrab.${editGrabStatus}`
      : null;
  const showMeta = hasStats || editGrabHintKey;

  useEffect(() => {
    setDraftText(text ?? "");
  }, [text]);

  const handleChange = useCallback((newText: string) => {
    setDraftText(newText);
    onDraftChange?.(newText);
  }, [onDraftChange]);

  const handleBlur = useCallback(() => {
    const edited = draftText.trim();
    const baseline = originalText?.trim() ?? "";
    if (edited && baseline && edited !== baseline) {
      onTextChange?.(edited);
    }
  }, [draftText, originalText, onTextChange]);

  const handleCopy = useCallback(() => {
    const currentText = bodyRef.current?.value.trim() ?? draftText.trim() ?? text ?? "";
    onCopy(currentText, "original");
  }, [draftText, text, onCopy]);

  return (
    <>
      {hasResult && (
        <div style={{ marginBottom: 12 }} className="animate-slide-up">
          <div className="result-card">
            <div className="result-card-header">
              <span className="result-card-title">
                <span className="result-dot" />
                {t("result.title")}
              </span>
              <button aria-label={t("common.copy")} className="icon-btn icon-btn-sm" onClick={handleCopy}>
                {copiedId === "original"
                  ? <span className="animate-check-draw"><Check size={12} /></span>
                  : <Copy size={12} strokeWidth={1.5} />}
              </button>
            </div>
            <textarea
              ref={bodyRef}
              className="result-card-body"
              aria-label={t("result.editableTranscription")}
              value={draftText}
              rows={Math.max(3, draftText.split(/\r?\n/).length)}
              onChange={(event) => handleChange(event.target.value)}
              onBlur={handleBlur}
              spellCheck={false}
            />
            {showMeta && (
              <p className="result-card-stats">
                {hasStats && detectedLanguage && (
                  <span className="result-lang-tag">{detectedLanguage}</span>
                )}
                {hasStats && t("result.stats", { chars: charCount, duration: durationSec.toFixed(1), cpm: Math.round((charCount / durationSec) * 60) })}
                {editGrabHintKey && (
                  <span className="result-edit-grab-hint">{t(editGrabHintKey)}</span>
                )}
              </p>
            )}
          </div>
        </div>
      )}
      {isProcessing && !text && (
        <div className="animate-fade-in" style={{ marginBottom: 12 }}>
          <div className="result-card result-card-skeleton">
            <div className="skeleton-card-header">
              <Loader2 size={12} className="animate-spin icon-tertiary" strokeWidth={1.75} />
              <span>{t("result.recognizingSpeech")}</span>
            </div>
            <div className="skeleton-paragraph">
              <div className="skeleton-shimmer skeleton-line" style={{ width: "94%" }} />
              <div className="skeleton-shimmer skeleton-line" style={{ width: "78%" }} />
              <div className="skeleton-shimmer skeleton-line" style={{ width: "62%" }} />
            </div>
          </div>
        </div>
      )}
    </>
  );
}
