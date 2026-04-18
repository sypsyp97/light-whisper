import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus, type ModelStage } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";
import { setInputMethodCommand, setAiPolishConfig, getAiPolishApiKey, setInputDevice, setSoundEnabled, setRecordingMode } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { INPUT_METHOD_KEY, INPUT_DEVICE_STORAGE_KEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY, RECORDING_MODE_KEY } from "@/lib/constants";
import type { HistoryItem, HotkeyDiagnostic, RecordingMode } from "@/types";

interface RecordingContextValue {
  // recording
  isRecording: boolean;
  isProcessing: boolean;
  startRecording: () => Promise<void>;
  stopRecording: () => Promise<void>;
  recordingError: string | null;
  transcriptionResult: string | null;
  setTranscriptionResult: (text: string) => void;
  originalAsrText: string | null;
  editBaselineText: string | null;
  setEditBaselineText: (text: string | null) => void;
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
    editBaselineText,
    setEditBaselineText,
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

  // 启动时将 localStorage 持久化的各项设置同步到后端。
  // 任何一个 backend 调用失败都要在 console 上留痕，便于排障；
  // 但不能让错误逃逸到 React 渲染，否则会炸整个 provider。
  useEffect(() => {
    const storedInputMethod = readLocalStorage(INPUT_METHOD_KEY);
    const storedInputDevice = readLocalStorage(INPUT_DEVICE_STORAGE_KEY);
    const storedSoundEnabled = readLocalStorage(SOUND_ENABLED_KEY);
    const storedRecordingMode = readLocalStorage(RECORDING_MODE_KEY);
    const aiPolishEnabled = readLocalStorage(AI_POLISH_ENABLED_KEY) === "true";

    const tasks: Array<{ name: string; run: () => Promise<unknown> }> = [];
    if (storedInputMethod === "clipboard") {
      tasks.push({
        name: "setInputMethodCommand",
        run: () => setInputMethodCommand("clipboard"),
      });
    }
    if (storedInputDevice != null) {
      tasks.push({
        name: "setInputDevice",
        run: () => setInputDevice(storedInputDevice),
      });
    }
    if (storedSoundEnabled === "false") {
      tasks.push({
        name: "setSoundEnabled",
        run: () => setSoundEnabled(false),
      });
    }
    if (storedRecordingMode === "toggle") {
      tasks.push({
        name: "setRecordingMode",
        run: () => setRecordingMode(true),
      });
    }
    if (aiPolishEnabled) {
      tasks.push({
        name: "getAiPolishApiKey/setAiPolishConfig",
        run: () =>
          getAiPolishApiKey().then((apiKey) =>
            setAiPolishConfig(aiPolishEnabled, apiKey),
          ),
      });
    }

    if (tasks.length === 0) return;

    Promise.allSettled(tasks.map((t) => t.run())).then((results) => {
      results.forEach((result, idx) => {
        if (result.status === "rejected") {
          console.error(
            `[RecordingProvider startup] ${tasks[idx].name} failed:`,
            result.reason,
          );
        }
      });
    });
  }, []);

  const contextValue: RecordingContextValue = useMemo(() => ({
    isRecording,
    isProcessing,
    startRecording,
    stopRecording,
    recordingError,
    transcriptionResult,
    setTranscriptionResult,
    originalAsrText,
    editBaselineText,
    setEditBaselineText,
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
  }), [
    isRecording, isProcessing, startRecording, stopRecording, recordingError,
    transcriptionResult, setTranscriptionResult, originalAsrText,
    editBaselineText, setEditBaselineText,
    durationSec, charCount, detectedLanguage, history, resultMode,
    stage, isReady, device, gpuName, downloadProgress, downloadMessage, isDownloading,
    modelError, downloadModels, cancelDownload, retryModel,
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
