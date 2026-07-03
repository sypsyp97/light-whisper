import { useCallback, useEffect, useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import IconButton from "@/components/ui/IconButton";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import type { EditGrabStatus, TranscriptionResultStage, TranscriptionTiming } from "@/types";

export interface TranscriptionResultProps {
  text: string;
  originalText: string | null;
  mode: "dictation" | "assistant";
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
  editGrabStatus?: EditGrabStatus | null;
  timing?: TranscriptionTiming | null;
  resultStage?: TranscriptionResultStage | null;
  onChange: (next: string) => void;
  onSubmitCorrection: (original: string, corrected: string, raw: string | null) => void;
  onCopy: () => void;
}

export function TranscriptionResult({
  text,
  originalText,
  mode,
  durationSec,
  charCount,
  detectedLanguage,
  editGrabStatus,
  timing,
  resultStage,
  onChange,
  onSubmitCorrection,
  onCopy,
}: TranscriptionResultProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState(text);

  useEffect(() => { setDraft(text); }, [text]);

  const debouncedCorrection = useDebouncedCallback(
    (original: string, corrected: string, raw: string | null) => {
      onSubmitCorrection(original, corrected, raw);
    },
    900,
    { onUnmount: "flush" },
  );

  const handleChange = useCallback((next: string) => {
    setDraft(next);
    onChange(next);
    if (mode === "dictation") {
      const baseline = originalText ?? text;
      if (baseline && next !== baseline) {
        debouncedCorrection.schedule(baseline, next, originalText);
      }
    }
  }, [mode, originalText, text, onChange, debouncedCorrection]);

  const cpm = durationSec && durationSec > 0 && charCount
    ? Math.round((charCount / durationSec) * 60)
    : 0;
  const editGrabHintKey =
    editGrabStatus === "timeout" || editGrabStatus === "empty"
      ? `result.editGrab.${editGrabStatus}`
      : null;
  const latencyParts = [
    timing?.asrMs != null ? t("result.latency.asr", { ms: timing.asrMs }) : null,
    timing?.polishMs != null ? t("result.latency.ai", { ms: timing.polishMs }) : null,
    timing?.totalMs != null ? t("result.latency.total", { ms: timing.totalMs }) : null,
  ].filter((part): part is string => Boolean(part));
  const rawFirstStatusKey =
    timing?.rawFirst?.status === "preview_only" && resultStage === "polished"
      ? "polished_preview"
      : timing?.rawFirst?.status;
  const rawFirstStatus = rawFirstStatusKey
    ? t(`result.rawFirst.${rawFirstStatusKey}`)
    : null;
  const isRawPreview = resultStage === "raw";
  const showMeta =
    (charCount != null && durationSec != null && durationSec > 0)
    || latencyParts.length > 0
    || rawFirstStatus
    || editGrabHintKey;

  return (
    <div className="lw-result" data-testid="main-result">
      <div className="lw-result-header">
        <span className="lw-result-title">{t("result.title")}</span>
        <IconButton
          label={t("common.copy")}
          icon={<Copy size={14} />}
          onClick={onCopy}
          data-testid="main-result-copy"
        />
      </div>
      <textarea
        className="lw-result-text"
        value={draft}
        onChange={(e) => handleChange(e.target.value)}
        aria-label={t("result.editableTranscription")}
        rows={Math.max(2, draft.split("\n").length)}
        readOnly={isRawPreview}
        data-testid="main-result-text"
      />
      {showMeta && (
        <p className="lw-result-stats" data-testid="main-result-stats">
          {charCount != null && durationSec != null && durationSec > 0 && (
            <>
              {detectedLanguage && <span>{detectedLanguage} · </span>}
              {t("result.stats", {
                chars: charCount,
                duration: durationSec.toFixed(1),
                cpm,
              })}
            </>
          )}
          {latencyParts.length > 0 && (
            <span className="result-latency">{latencyParts.join(" · ")}</span>
          )}
          {rawFirstStatus && (
            <span className="result-raw-first">{rawFirstStatus}</span>
          )}
          {editGrabHintKey && (
            <span className="result-edit-grab-hint">{t(editGrabHintKey)}</span>
          )}
        </p>
      )}
    </div>
  );
}

export default TranscriptionResult;
