import { useState, useEffect, useRef, useCallback } from "react";
import { Settings, Minus, X } from "lucide-react";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRecordingContext } from "@/contexts/RecordingContext";
import { copyToClipboard, hideMainWindow } from "@/api/tauri";
import TitleBar from "@/components/TitleBar";
import RecordingButton from "@/components/RecordingButton";
import StatusIndicator from "@/components/StatusIndicator";
import TranscriptionResult from "@/components/TranscriptionResult";
import TranscriptionHistory from "@/components/TranscriptionHistory";
import { PADDING } from "@/lib/constants";

export default function MainPage({ onNavigate }: {
  onNavigate: (v: "main" | "settings") => void;
}) {
  const {
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, history, stage, isReady,
    device, gpuName, downloadProgress, downloadMessage,
    isDownloading, modelError,
    downloadModels: triggerDownload, cancelDownload, retryModel,
  } = useRecordingContext();

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [errorDismissed, setErrorDismissed] = useState(false);
  const copyTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => { setErrorDismissed(false); }, [recordingError, modelError]);
  useEffect(() => () => { if (copyTimerRef.current) clearTimeout(copyTimerRef.current); }, []);

  const handleCopy = useCallback(async (text: string, id: string) => {
    try {
      await copyToClipboard(text);
      setCopiedId(id);
      toast.success("已复制到剪贴板");
      if (copyTimerRef.current) clearTimeout(copyTimerRef.current);
      copyTimerRef.current = setTimeout(() => {
        setCopiedId(null);
        copyTimerRef.current = null;
      }, 1500);
    } catch {
      toast.error("复制失败");
    }
  }, []);

  const handleToggleRecording = useCallback(() => {
    if (!isReady) return;
    isRecording ? stopRecording() : startRecording();
  }, [isReady, isRecording, stopRecording, startRecording]);

  return (
    <div className="page-root">

      <TitleBar
        title="轻语 Whisper"
        leftAction={
          <button aria-label="设置" className="icon-btn plain" onClick={() => onNavigate("settings")}>
            <Settings size={13} strokeWidth={1.5} />
          </button>
        }
        rightActions={
          <>
            <button aria-label="最小化" className="icon-btn" onClick={() => getCurrentWindow().minimize()}>
              <Minus size={12} strokeWidth={1.5} />
            </button>
            <button aria-label="关闭" className="icon-btn" onClick={() => hideMainWindow()}>
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
              <button onClick={() => setErrorDismissed(true)} aria-label="关闭" className="error-dismiss-btn">
                <X size={12} />
              </button>
            </div>
            {stage === "error" && <button onClick={retryModel} className="error-retry-link">重试</button>}
          </div>
        </div>
      )}

      {/* Results */}
      <div className="results-area" style={{ padding: `0 ${PADDING}px 12px` }}>
        <TranscriptionResult
          text={transcriptionResult}
          isProcessing={isProcessing}
          copiedId={copiedId}
          onCopy={handleCopy}
        />
        <TranscriptionHistory
          history={history}
          currentResult={transcriptionResult}
          copiedId={copiedId}
          onCopy={handleCopy}
        />
      </div>

    </div>
  );
}
