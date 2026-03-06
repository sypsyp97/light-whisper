import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus, type ModelStage } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";
import { setInputMethodCommand, setAiPolishConfig, getAiPolishApiKey, setInputDevice, setSoundEnabled } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { INPUT_METHOD_KEY, INPUT_DEVICE_STORAGE_KEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY } from "@/lib/constants";
import type { HotkeyDiagnostic, TranscriptionResult, HistoryItem } from "@/types";

interface RecordingContextValue {
  // recording
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<TranscriptionResult | null>;
  recordingError: string | null;
  transcriptionResult: string | null;
  setTranscriptionResult: (text: string) => void;
  originalAsrText: string | null;
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
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
  hotkeyDiagnostic: HotkeyDiagnostic | null;
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
    setTranscriptionResult,
    originalAsrText,
    durationSec,
    charCount,
    detectedLanguage,
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

  const {
    hotkeyDisplay,
    setHotkey,
    error: hotkeyError,
    diagnostic: hotkeyDiagnostic,
  } = useHotkey();

  // 启动时将 localStorage 中持久化的输入方式同步到后端
  useEffect(() => {
    const stored = readLocalStorage(INPUT_METHOD_KEY);
    if (stored === "clipboard") {
      setInputMethodCommand("clipboard").catch(() => {});
    }
  }, []);

  // 启动时将持久化的麦克风选择同步到后端
  useEffect(() => {
    const stored = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
    if (stored) {
      setInputDevice(stored).catch(() => {});
    }
  }, []);

  // 启动时将音效开关同步到后端（默认开启）
  useEffect(() => {
    const stored = readLocalStorage(SOUND_ENABLED_KEY);
    if (stored === "false") {
      setSoundEnabled(false).catch(() => {});
    }
  }, []);

  // 启动时将 AI 润色开关同步到后端（API Key 已在后端 setup 从密钥环加载）
  useEffect(() => {
    const enabled = readLocalStorage(AI_POLISH_ENABLED_KEY) === "true";
    if (enabled) {
      getAiPolishApiKey()
        .then(apiKey => setAiPolishConfig(enabled, apiKey))
        .catch(() => {});
    }
  }, []);

  const contextValue = useMemo<RecordingContextValue>(() => ({
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    recordingError,
    transcriptionResult,
    setTranscriptionResult,
    originalAsrText,
    durationSec,
    charCount,
    detectedLanguage,
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
    hotkeyDiagnostic,
  }), [
    isRecording, isProcessing, startRecording, stopRecording,
    recordingError, transcriptionResult, setTranscriptionResult, originalAsrText,
    durationSec, charCount, detectedLanguage, history,
    stage, isReady, device, gpuName,
    downloadProgress, downloadMessage, isDownloading, modelError,
    downloadModels, cancelDownload, retryModel,
    hotkeyDisplay, hotkeyError, setHotkey, hotkeyDiagnostic,
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
