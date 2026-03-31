import { useState, useEffect, useRef, useCallback } from "react";
import { Settings, Minus, X } from "lucide-react";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { copyToClipboard, hideMainWindow, submitUserCorrection } from "@/api/tauri";
import TitleBar from "@/components/TitleBar";
import RecordingButton from "@/components/RecordingButton";
import StatusIndicator from "@/components/StatusIndicator";
import TranscriptionResult from "@/components/TranscriptionResult";
import TranscriptionHistory from "@/components/TranscriptionHistory";
import { useDebouncedCallback } from "@/hooks/useDebouncedCallback";
import { useTranslation } from "react-i18next";
import { PADDING, ONBOARDING_DISMISSED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

export default function MainPage({ onNavigate }: {
  onNavigate: (v: "main" | "settings") => void;
}) {
  const { t } = useTranslation();
  const {
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, originalAsrText, setOriginalAsrText, setTranscriptionResult,
    durationSec, charCount, detectedLanguage, history, recordingMode, stage, isReady,
    device, gpuName, downloadProgress, downloadMessage,
    isDownloading, modelError, hotkeyDisplay,
    downloadModels: triggerDownload, cancelDownload, retryModel,
  } = useRecordingContext();

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const [onboardingDismissed, setOnboardingDismissed] = useState(() => readLocalStorage(ONBOARDING_DISMISSED_KEY) === "true");
  const isToggleMode = useRef(readLocalStorage(RECORDING_MODE_KEY) === "toggle").current;
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => { setErrorDismissed(false); }, [recordingError, modelError]);
  // Auto-dismiss onboarding after first successful transcription
  useEffect(() => {
    if (transcriptionResult && !onboardingDismissed) {
      setOnboardingDismissed(true);
      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
    }
  }, [transcriptionResult, onboardingDismissed]);
  useEffect(() => () => { if (copyTimerRef.current) clearTimeout(copyTimerRef.current); }, []);

  const correctionSubmit = useDebouncedCallback((previousText: string, nextText: string) => {
    submitUserCorrection(previousText, nextText)
      .then(() => toast.success(t("toast.correctionRecorded"), { duration: 1500 }))
      .catch(() => toast.error(t("toast.correctionFailed")));
  }, 900, { onUnmount: "flush" });

  const handleCopy = useCallback(async (text: string, id: string) => {
    try {
      await copyToClipboard(text);
      setCopiedId(id);
      toast.success(t("common.copiedToClipboard"));
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        setCopiedId(null);
        copyTimerRef.current = null;
      }, 1500);
    } catch {
      toast.error(t("common.copyFailed"));
    }
  }, [t]);

  const handleDraftChange = useCallback((newText: string) => {
    setTranscriptionResult(newText);
  }, [setTranscriptionResult]);

  const handleTextChange = useCallback((newText: string) => {
    if (recordingMode !== "dictation") {
      setTranscriptionResult(newText);
      return;
    }
    if (originalAsrText && newText !== originalAsrText) {
      const prevText = originalAsrText;
      setOriginalAsrText(newText);
      setTranscriptionResult(newText);
      correctionSubmit.schedule(prevText, newText);
    }
  }, [
    correctionSubmit,
    originalAsrText,
    recordingMode,
    setTranscriptionResult,
    setOriginalAsrText,
  ]);

  const flushPendingEditAndNavigate = useCallback((target: "main" | "settings") => {
    const active = document.activeElement;
    if (active instanceof HTMLElement && (active.isContentEditable || active instanceof HTMLTextAreaElement || active instanceof HTMLInputElement)) {
      active.blur();
    }
    correctionSubmit.flush();
    onNavigate(target);
  }, [correctionSubmit, onNavigate]);

  const handleToggleRecording = useCallback(() => {
    if (!isReady) return;
    isRecording ? stopRecording() : startRecording();
  }, [isReady, isRecording, stopRecording, startRecording]);

  return (
    <div className="page-root">

      <TitleBar
        title={t("app.title")}
        leftAction={
          <button aria-label={t("common.settings")} className="icon-btn plain icon-btn-gear" onClick={() => flushPendingEditAndNavigate("settings")}>
            <Settings size={13} strokeWidth={1.5} />
          </button>
        }
        rightActions={
          <>
            <button aria-label={t("common.minimize")} className="icon-btn" onClick={() => getCurrentWindow().minimize()}>
              <Minus size={12} strokeWidth={1.5} />
            </button>
            <button aria-label={t("common.close")} className="icon-btn" onClick={() => hideMainWindow()}>
              <X size={12} strokeWidth={1.5} />
            </button>
          </>
        }
      />

      {/* Recording zone */}
      <div className="recording-zone" style={{ padding: `16px ${PADDING}px 12px` }}>
        <StatusIndicator
          stage={stage}
          isReady={isReady}
          isRecording={isRecording}
          isProcessing={isProcessing}
          device={device}
          gpuName={gpuName}
          downloadProgress={downloadProgress}
          downloadMessage={downloadMessage}
          isDownloading={isDownloading}
          downloadModels={triggerDownload}
          cancelDownload={cancelDownload}
        >
          <RecordingButton
            isRecording={isRecording}
            isProcessing={isProcessing}
            isReady={isReady}
            onToggle={handleToggleRecording}
          />
        </StatusIndicator>
      </div>

      {/* Error */}
      {(recordingError || modelError) && !errorDismissed && (
        <div className="error-section animate-shake" style={{ margin: `0 ${PADDING}px 8px` }}>
          <div className="error-banner">
            <div className="error-banner-inner">
              <p style={{ fontSize: 12, color: "var(--color-error)", lineHeight: 1.6, flex: 1 }}>{recordingError || modelError}</p>
              <button onClick={() => setErrorDismissed(true)} aria-label={t("common.close")} className="error-dismiss-btn">
                <X size={12} />
              </button>
            </div>
            {stage === "error" && <button onClick={retryModel} className="error-retry-link">{t("common.retry")}</button>}
          </div>
        </div>
      )}

      {/* Results */}
      <div className="results-area" style={{ padding: `0 ${PADDING}px 12px` }}>
        <TranscriptionResult
          text={transcriptionResult}
          originalText={originalAsrText}
          isProcessing={isProcessing}
          copiedId={copiedId}
          onCopy={handleCopy}
          onDraftChange={handleDraftChange}
          onTextChange={handleTextChange}
          durationSec={durationSec}
          charCount={charCount}
          detectedLanguage={detectedLanguage}
        />
        <TranscriptionHistory
          history={history}
          currentResult={transcriptionResult}
          copiedId={copiedId}
          onCopy={handleCopy}
        />
        {/* First-use onboarding hint */}
        {!onboardingDismissed && !transcriptionResult && !isProcessing && isReady && history.length === 0 && (
          <div className="result-card animate-fade-in" style={{ marginTop: 8 }}>
              <div className="result-card-header">
                <span className="result-card-title">
                  <span className="result-dot" />
                  {t("main.quickStart")}
                </span>
                <button
                  aria-label={t("common.close")}
                  className="icon-btn"
                  style={{ padding: 6 }}
                  onClick={() => {
                    setOnboardingDismissed(true);
                    writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
                  }}
                >
                  <X size={12} strokeWidth={1.5} />
                </button>
              </div>
              <div style={{ padding: "10px 12px", fontSize: 13, lineHeight: 1.8, color: "var(--color-text-secondary)" }}>
                <p style={{ margin: "0 0 6px" }}>
                  {t("main.pressHotkey")} <strong style={{ color: "var(--color-accent)" }}>{hotkeyDisplay}</strong> {isToggleMode ? t("main.hotkeyHintToggle") : t("main.hotkeyHintHold")}
                </p>
                <p style={{ margin: "0 0 6px" }}>{t("main.autoInputHint")}</p>
                <p style={{ margin: 0, fontSize: 12, color: "var(--color-text-tertiary)" }}>
                  {t("main.settingsHint")}
                </p>
              </div>
          </div>
        )}
      </div>

    </div>
  );
}
