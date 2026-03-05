import { createContext, useContext, useCallback, useEffect, useMemo, useRef, type ReactNode } from "react";
import { useRecording } from "@/hooks/useRecording";
import { useModelStatus, type ModelStage } from "@/hooks/useModelStatus";
import { useHotkey } from "@/hooks/useHotkey";
import { setInputMethodCommand, setAiPolishConfig, getAiPolishApiKey, setSoundEnabled } from "@/api/tauri";
import { readLocalStorage } from "@/lib/storage";
import { INPUT_METHOD_KEY, AI_POLISH_ENABLED_KEY, SOUND_ENABLED_KEY } from "@/lib/constants";
import type { TranscriptionResult, HistoryItem } from "@/types";

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
    setTranscriptionResult,
    originalAsrText,
    durationSec,
    charCount,
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

  // 录音结束后的冷却期，防止快捷键误触发新录音
  const cooldownUntilRef = useRef(0);

  // isProcessing 从 true 变为 false 时设置冷却期
  const prevProcessingRef = useRef(false);
  useEffect(() => {
    if (prevProcessingRef.current && !isProcessing) {
      cooldownUntilRef.current = Date.now() + 600;
    }
    prevProcessingRef.current = isProcessing;
  }, [isProcessing]);

  // F2 push-to-talk: press to start, release to stop
  const hotkeyStart = useCallback(() => {
    if (!isReady || isRecording || isProcessing) return;
    if (Date.now() < cooldownUntilRef.current) return;
    startRecording();
  }, [isReady, isRecording, isProcessing, startRecording]);

  const hotkeyStop = useCallback(() => {
    if (!isRecording) return;
    cooldownUntilRef.current = Date.now() + 600;
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
    recordingError, transcriptionResult, setTranscriptionResult, originalAsrText,
    durationSec, charCount, history,
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
