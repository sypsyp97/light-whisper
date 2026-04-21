import { useTranslation } from "react-i18next";
import Kbd from "@/components/ui/Kbd";
import RecordingButton from "./RecordingButton";

export interface RecordingStageProps {
  isRecording: boolean;
  isProcessing: boolean;
  isReady: boolean;
  hotkeyDisplay: string;
  recordingMode: "hold" | "toggle";
  error: string | null;
  onToggle: () => void;
}

export function RecordingStage({
  isRecording,
  isProcessing,
  isReady,
  hotkeyDisplay,
  recordingMode,
  error,
  onToggle,
}: RecordingStageProps) {
  const { t } = useTranslation();
  const state = !isReady
    ? "disabled"
    : isRecording
      ? "recording"
      : isProcessing
        ? "processing"
        : "idle";

  return (
    <div className="lw-stage" data-testid="main-record-stage">
      <RecordingButton state={state} onClick={onToggle} />
      <div className="lw-stage-hint">
        {hotkeyDisplay && <Kbd combo={hotkeyDisplay} />}
        <span>
          {recordingMode === "toggle" ? t("main.hotkeyHintToggle") : t("main.hotkeyHintHold")}
        </span>
      </div>
      {error && <div className="lw-field-error" role="status">{error}</div>}
    </div>
  );
}

export default RecordingStage;
