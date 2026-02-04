import { createContext, useContext, useCallback, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";

interface RecordingContextValue {
  // recording
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<any>;
  recordingError: string | null;
  transcriptionResult: string | null;
  // model
  stage: string;
  isReady: boolean;
  device: string | null;
  gpuName: string | null;
  downloadProgress: number;
  downloadMessage: string | null;
  isDownloading: boolean;
  modelError: string | null;
  downloadModels: () => void;
  cancelDownload: () => void;
  retryModel: () => void;
}

const RecordingContext = createContext<RecordingContextValue | null>(null);

export function RecordingProvider({ children }: { children: ReactNode }) {
  const {
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    error: recordingError,
    transcriptionResult,
  } = useRecording();

  const {
    stage,
    isReady,
    device,
    gpuName,
    downloadProgress,
    downloadMessage,
    isDownloading,
    error: modelError,
    downloadModels,
    cancelDownload,
    retry: retryModel,
  } = useModelStatus();

  const toggleRecording = useCallback(() => {
    if (!isReady) return;
    if (isRecording) stopRecording();
    else startRecording();
  }, [isReady, isRecording, startRecording, stopRecording]);

  // F2 hotkey — always mounted at root, never unregistered during navigation
  useHotkey(toggleRecording);

  return (
    <RecordingContext.Provider
      value={{
        isRecording,
        isProcessing,
        startRecording,
        stopRecording,
        recordingError,
        transcriptionResult,
        stage,
        isReady,
        device,
        gpuName,
        downloadProgress,
        downloadMessage,
        isDownloading,
        modelError,
        downloadModels,
        cancelDownload,
        retryModel,
      }}
    >
      {children}
    </RecordingContext.Provider>
  );
}

export function useRecordingContext() {
  const ctx = useContext(RecordingContext);
  if (!ctx) throw new Error("useRecordingContext must be used within RecordingProvider");
  return ctx;
}
