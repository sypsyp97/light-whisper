import { useCallback, useEffect, useState } from "react";
import { Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import IconButton from "@/components/ui/IconButton";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";

export interface TranscriptionResultProps {
  text: string;
  originalText: string | null;
  mode: "dictation" | "assistant";
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
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
        data-testid="main-result-text"
      />
      {charCount != null && durationSec != null && durationSec > 0 && (
        <p className="lw-result-stats" data-testid="main-result-stats">
          {detectedLanguage && <span>{detectedLanguage} · </span>}
          {t("result.stats", {
            chars: charCount,
            duration: durationSec.toFixed(1),
            cpm,
          })}
        </p>
      )}
    </div>
  );
}

export default TranscriptionResult;
