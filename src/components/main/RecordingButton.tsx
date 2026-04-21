import { Mic, Square, Loader2 } from "lucide-react";
import { useTranslation } from "react-i18next";

export interface RecordingButtonProps {
  state: "idle" | "recording" | "processing" | "disabled";
  onClick: () => void;
  "data-testid"?: string;
}

export default function RecordingButton({ state, onClick, "data-testid": testId }: RecordingButtonProps) {
  const { t } = useTranslation();
  const label =
    state === "recording" ? t("recording.stop")
    : state === "processing" ? t("recording.processing")
    : t("recording.start");

  const icon =
    state === "recording" ? <Square size={28} strokeWidth={2} fill="currentColor" />
    : state === "processing" ? <Loader2 size={28} className="lw-spinner" />
    : <Mic size={32} strokeWidth={1.5} />;

  const cls = [
    "lw-record-btn",
    state === "recording" ? "lw-record-btn--recording" : "",
    state === "processing" ? "lw-record-btn--processing" : "",
  ].filter(Boolean).join(" ");

  return (
    <div className="lw-record-btn-wrap">
      {state === "recording" && <span className="lw-record-pulse" aria-hidden="true" />}
      <button
        type="button"
        className={cls}
        aria-label={label}
        aria-pressed={state === "recording"}
        disabled={state === "disabled" || state === "processing"}
        onClick={onClick}
        data-testid={testId ?? "main-record-btn"}
      >
        {icon}
      </button>
    </div>
  );
}
