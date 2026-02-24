import { createContext, useContext, useCallback, useEffect, useMemo, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus, type ModelStage } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";
import { setInputMethodCommand } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { INPUT_METHOD_KEY } from "@/lib/constants";
import type { TranscriptionResult, HistoryItem } from "@/types";

interface RecordingContextValue {
  // recording
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  recordingError: string | null;
  transcriptionResult: string | null;
  history: HistoryItem[];
  // model
  stage: ModelStage;
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
  // hotkey
  hotkeyDisplay: string;
  hotkeyError: string | null;
  setHotkey: (shortcut: string) => Promise<void>;
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
    history,
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

  // F2 push-to-talk: press to start, release to stop
  const hotkeyStart = useCallback(() => {
    if (!isReady || isRecording || isProcessing) return;
    startRecording();
  }, [isReady, isRecording, isProcessing, startRecording]);

  const hotkeyStop = useCallback(() => {
    if (!isRecording) return;
    stopRecording();
  }, [isRecording, stopRecording]);

  const {
    hotkeyDisplay,
    setHotkey,
    error: hotkeyError,
  } = useHotkey(hotkeyStart, hotkeyStop);

  // 启动时将 localStorage 中持久化的输入方式同步到后端
  useEffect(() => {
    const stored = readLocalStorage(INPUT_METHOD_KEY);
    if (stored === "clipboard") {
      setInputMethodCommand("clipboard").catch(() => {});
    }
  }, []);

  const contextValue = useMemo<RecordingContextValue>(() => ({
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    recordingError,
    transcriptionResult,
    history,
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
    hotkeyDisplay,
    hotkeyError,
    setHotkey,
  }), [
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, history,
    stage, isReady, device, gpuName,
    downloadProgress, downloadMessage, isDownloading, modelError,
    downloadModels, cancelDownload, retryModel,
    hotkeyDisplay, hotkeyError, setHotkey,
  ]);

  return (
    <RecordingContext.Provider value={contextValue}>
      {children}
    </RecordingContext.Provider>
  );
}

export function useRecordingContext() {
  const ctx = useContext(RecordingContext);
  if (!ctx) throw new Error("useRecordingContext must be used within RecordingProvider");
  return ctx;
}
