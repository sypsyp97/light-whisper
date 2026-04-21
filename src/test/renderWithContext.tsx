import React, { createContext, useContext, type ReactElement } from "react";
import { render, type RenderResult } from "@testing-library/react";
import { vi } from "vitest";
import type { HistoryItem, HotkeyDiagnostic, RecordingMode } from "@/types";

export interface RecordingContextValue {
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
  stage: "checking" | "loading" | "ready" | "error";
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
  hotkeyDisplay: string;
  hotkeyError: string | null;
  setHotkey: (shortcut: string) => Promise<void>;
  hotkeyDiagnostic: HotkeyDiagnostic | null;
}

const TestRecordingContext = createContext<RecordingContextValue | null>(null);

let fallbackCtx: RecordingContextValue | null = null;
function getFallback(): RecordingContextValue {
  if (!fallbackCtx) fallbackCtx = buildDefaultContext();
  return fallbackCtx;
}

export function useRecordingContext(): RecordingContextValue {
  return useContext(TestRecordingContext) ?? getFallback();
}

export function RecordingProvider({ children }: { children: React.ReactNode }) {
  return (
    <TestRecordingContext.Provider value={getFallback()}>
      {children}
    </TestRecordingContext.Provider>
  );
}

export function buildDefaultContext(overrides?: Partial<RecordingContextValue>): RecordingContextValue {
  const base: RecordingContextValue = {
    isRecording: false,
    isProcessing: false,
    startRecording: vi.fn(async () => {}),
    stopRecording: vi.fn(async () => {}),
    recordingError: null,
    transcriptionResult: null,
    setTranscriptionResult: vi.fn(),
    originalAsrText: null,
    editBaselineText: null,
    setEditBaselineText: vi.fn(),
    durationSec: null,
    charCount: null,
    detectedLanguage: null,
    history: [],
    recordingMode: "dictation",
    stage: "ready",
    isReady: true,
    device: "Default",
    gpuName: null,
    downloadProgress: 0,
    downloadMessage: null,
    isDownloading: false,
    modelError: null,
    downloadModels: vi.fn(),
    cancelDownload: vi.fn(),
    retryModel: vi.fn(),
    hotkeyDisplay: "F2",
    hotkeyError: null,
    setHotkey: vi.fn(async () => {}),
    hotkeyDiagnostic: null,
  };
  return { ...base, ...(overrides ?? {}) };
}

export function renderWithRecordingContext(
  ui: ReactElement,
  overrides?: Partial<RecordingContextValue>,
): RenderResult & { ctx: RecordingContextValue } {
  const ctx = buildDefaultContext(overrides);
  const result = render(
    <TestRecordingContext.Provider value={ctx}>{ui}</TestRecordingContext.Provider>,
  );
  return { ...result, ctx };
}
