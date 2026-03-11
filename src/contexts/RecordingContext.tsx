import { createContext, useContext, useEffect, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus, type ModelStage } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";
import { setInputMethodCommand, setAiPolishConfig, getAiPolishApiKey, setInputDevice, setSoundEnabled, setRecordingMode } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { INPUT_METHOD_KEY, INPUT_DEVICE_STORAGE_KEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import type { HistoryItem, HotkeyDiagnostic, RecordingMode, TranscriptionResult } from "@/types";

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
  setOriginalAsrText: (text: string | null) => void;
  durationSec: number | null;
  charCount: number | null;
  detectedLanguage: string | null;
  history: HistoryItem[];
  recordingMode: RecordingMode;
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
    setOriginalAsrText,
    durationSec,
    charCount,
    detectedLanguage,
    history,
    resultMode,
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

  // 启动时将 localStorage 持久化的各项设置同步到后端
  useEffect(() => {
    const storedInputMethod = readLocalStorage(INPUT_METHOD_KEY);
    const storedInputDevice = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
    const storedSoundEnabled = readLocalStorage(SOUND_ENABLED_KEY);
    const storedRecordingMode = readLocalStorage(RECORDING_MODE_KEY);
    const aiPolishEnabled = readLocalStorage(AI_POLISH_ENABLED_KEY) === "true";

    if (storedInputMethod === "clipboard") {
      setInputMethodCommand("clipboard").catch(() => {});
    }
    if (storedInputDevice != null) {
      setInputDevice(storedInputDevice).catch(() => {});
    }
    if (storedSoundEnabled === "false") {
      setSoundEnabled(false).catch(() => {});
    }
    if (storedRecordingMode === "toggle") {
      setRecordingMode(true).catch(() => {});
    }
    if (aiPolishEnabled) {
      getAiPolishApiKey().then(apiKey => setAiPolishConfig(aiPolishEnabled, apiKey)).catch(() => {});
    }
  }, []);

  const contextValue: RecordingContextValue = {
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    recordingError,
    transcriptionResult,
    setTranscriptionResult,
    originalAsrText,
    setOriginalAsrText,
    durationSec,
    charCount,
    detectedLanguage,
    history,
    recordingMode: resultMode,
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
  };

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
