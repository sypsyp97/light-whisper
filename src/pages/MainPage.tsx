import { useCallback, useEffect, useState } from "react";
import { Settings } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRecordingContext } from "@/contexts/RecordingContext";
import {
  copyToClipboard,
  openPermissionSettings,
  resetPermission,
  submitUserCorrection,
  type PermissionKind,
} from "@/api/tauri";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import { ONBOARDING_DISMISSED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import TitleBar from "@/components/main/TitleBar";
import StatusIndicator from "@/components/main/StatusIndicator";
import RecordingStage from "@/components/main/RecordingStage";
import TranscriptionResult from "@/components/main/TranscriptionResult";
import TranscriptionHistory from "@/components/main/TranscriptionHistory";
import OnboardingHint from "@/components/main/OnboardingHint";
import Banner from "@/components/ui/Banner";
import type { HistoryItem } from "@/types";

interface MainPageProps {
  onNavigate?: (target: "main" | "settings") => void;
  animClass?: string;
}

export default function MainPage({ onNavigate, animClass }: MainPageProps) {
  const { t } = useTranslation();
  const {
    isRecording,
    isProcessing,
    isReady,
    startRecording,
    stopRecording,
    recordingError,
    recordingErrorPermission,
    transcriptionResult,
    originalAsrText,
    setTranscriptionResult,
    editBaselineText,
    setEditBaselineText,
    durationSec,
    charCount,
    detectedLanguage,
    editGrabStatus,
    timing,
    resultStage,
    history,
    recordingMode,
    stage,
    device,
    gpuName,
    downloadProgress,
    downloadMessage,
    modelError,
    retryModel,
    hotkeyDisplay,
  } = useRecordingContext();

  const [onboardingDismissed, setOnboardingDismissed] = useState(
    () => readLocalStorage(ONBOARDING_DISMISSED_KEY) === "true",
  );
  const [errorBannerDismissed, setErrorBannerDismissed] = useState(false);
  const storedMode = readLocalStorage(RECORDING_MODE_KEY) === "toggle" ? "toggle" : "hold";

  useEffect(() => { setErrorBannerDismissed(false); }, [recordingError, modelError]);

  useEffect(() => {
    if (transcriptionResult && !onboardingDismissed) {
      setOnboardingDismissed(true);
      writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
    }
  }, [transcriptionResult, onboardingDismissed]);

  const handleToggle = useCallback(() => {
    if (!isReady) return;
    if (isRecording) void stopRecording();
    else void startRecording();
  }, [isReady, isRecording, startRecording, stopRecording]);

  const handleCopyResult = useCallback(async () => {
    if (!transcriptionResult) return;
    try {
      await copyToClipboard(transcriptionResult);
      toast.success(t("common.copied"));
    } catch {
      toast.error(t("common.copyFailed"));
    }
  }, [transcriptionResult, t]);

  const handleCopyHistory = useCallback(async (item: HistoryItem) => {
    try {
      await copyToClipboard(item.text);
      toast.success(t("common.copied"));
    } catch {
      toast.error(t("common.copyFailed"));
    }
  }, [t]);

  const handleChange = useCallback((next: string) => {
    setTranscriptionResult(next);
    setEditBaselineText(next);
  }, [setTranscriptionResult, setEditBaselineText]);

  const handleSubmitCorrection = useCallback(
    (original: string, corrected: string, raw: string | null) => {
      submitUserCorrection(original, corrected, raw)
        .then(() => toast.success(t("toast.correctionRecorded"), { duration: 1500 }))
        .catch(() => toast.error(t("toast.correctionFailed")));
    },
    [t],
  );

  const handleDismissOnboarding = useCallback(() => {
    setOnboardingDismissed(true);
    writeLocalStorage(ONBOARDING_DISMISSED_KEY, "true");
  }, []);

  const handlePermissionRecovery = useCallback((kind: PermissionKind) => {
    resetPermission(kind)
      .then(() => toast.success(t("toast.permissionReset")))
      .catch(() => toast.error(t("toast.permissionResetFailed")))
      .finally(() => {
        void openPermissionSettings(kind);
      });
  }, [t]);

  const engineLabel = device === "cloud" ? "Online" : device ?? "";
  const showError = (recordingError || modelError) && !errorBannerDismissed;
  const errorMessage = recordingError || modelError || "";
  const errorIsModel = !!modelError;
  const errorIsPermission = !!recordingErrorPermission && !modelError;
  const showOnboarding = !onboardingDismissed && !transcriptionResult && !isProcessing && isReady && history.length === 0;

  const bannerAction = errorIsPermission && recordingErrorPermission
    ? {
        label: t("settings.permReset", { defaultValue: "Reset Permission" }),
        onClick: () => {
          handlePermissionRecovery(recordingErrorPermission.kind);
        },
        testId: "main-perm-reset-btn",
      }
    : errorIsModel && retryModel
      ? {
          label: t("common.retry"),
          onClick: retryModel,
          testId: "main-retry-btn",
        }
      : undefined;

  const indicatorStage: "checking" | "loading" | "ready" | "error" =
    stage === "ready" ? "ready"
    : stage === "error" ? "error"
    : stage === "loading" || stage === "downloading" || stage === "need_download" ? "loading"
    : "checking";

  return (
    <div className={`lw-root lw-main-page ${animClass ?? ""}`.trim()} data-testid="main-page">
      <TitleBar
        leftAction={{
          icon: <Settings size={14} />,
          label: t("common.settings"),
          onClick: () => onNavigate?.("settings"),
        }}
        onMinimize={() => { void getCurrentWindow().minimize(); }}
        onClose={() => { void getCurrentWindow().hide(); }}
      />
      <div className="lw-main-content">
        <StatusIndicator
          stage={indicatorStage}
          isReady={isReady}
          engineLabel={engineLabel || t("app.title")}
          device={device}
          gpuName={gpuName}
          downloadProgress={downloadProgress}
          downloadMessage={downloadMessage}
          error={modelError}
          onRetry={retryModel}
        />
        <RecordingStage
          isRecording={isRecording}
          isProcessing={isProcessing}
          isReady={isReady}
          hotkeyDisplay={hotkeyDisplay}
          recordingMode={storedMode}
          error={null}
          onToggle={handleToggle}
        />
        {showError && (
          <Banner
            tone="error"
            message={errorMessage}
            onDismiss={() => setErrorBannerDismissed(true)}
            action={bannerAction}
            data-testid="main-error-banner"
          />
        )}
        {transcriptionResult !== null && (
          <TranscriptionResult
            text={transcriptionResult}
            originalText={editBaselineText ?? originalAsrText}
            mode={recordingMode}
            durationSec={durationSec}
            charCount={charCount}
            detectedLanguage={detectedLanguage}
            editGrabStatus={editGrabStatus}
            timing={timing}
            resultStage={resultStage}
            onChange={handleChange}
            onSubmitCorrection={handleSubmitCorrection}
            onCopy={handleCopyResult}
          />
        )}
        {history.length > 0 && (
          <TranscriptionHistory items={history} onCopy={handleCopyHistory} />
        )}
        {showOnboarding && (
          <OnboardingHint
            hotkeyDisplay={hotkeyDisplay}
            mode={storedMode}
            onDismiss={handleDismissOnboarding}
          />
        )}
      </div>
    </div>
  );
}
