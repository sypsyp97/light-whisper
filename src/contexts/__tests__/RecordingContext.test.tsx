










import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";



const storageMock = vi.hoisted(() => ({
  readLocalStorage: vi.fn<(key: string) => string | null>(),
  writeLocalStorage: vi.fn<(key: string, value: string) => void>(),
}));

const tauriMock = vi.hoisted(() => ({
  setInputMethodCommand: vi.fn<(method: string) => Promise<void>>(),
  setInputDevice: vi.fn<(name?: string | null) => Promise<void>>(),
  setSoundEnabled: vi.fn<(enabled: boolean) => Promise<void>>(),
  setRecordingMode: vi.fn<(toggle: boolean) => Promise<void>>(),
  setAiPolishConfig: vi.fn<(enabled: boolean, apiKey?: string, provider?: string) => Promise<void>>(),
  getAiPolishApiKey: vi.fn<() => Promise<string>>(),
  saveAiPolishApiKey: vi.fn<(apiKey: string, provider?: string) => Promise<void>>(),
}));



vi.mock("@/hooks/useRecording", () => ({
  useRecording: () => ({
    isStarting: false,
    isRecording: false,
    isProcessing: false,
    startRecording: vi.fn(),
    stopRecording: vi.fn(),
    error: null,
    transcriptionResult: null,
    setTranscriptionResult: vi.fn(),
    originalAsrText: null,
    editBaselineText: null,
    setEditBaselineText: vi.fn(),
    durationSec: null,
    charCount: null,
    detectedLanguage: null,
    history: [],
    resultMode: "dictation" as const,
  }),
}));

vi.mock("@/hooks/useModelStatus", () => ({
  useModelStatus: () => ({
    stage: "ready" as const,
    isReady: true,
    device: null,
    gpuName: null,
    downloadProgress: 0,
    downloadMessage: null,
    isDownloading: false,
    error: null,
    downloadModels: vi.fn(),
    cancelDownload: vi.fn(),
    retry: vi.fn(),
  }),
}));

vi.mock("@/hooks/useHotkey", () => ({
  useHotkey: () => ({
    hotkeyDisplay: "F2",
    setHotkey: vi.fn(),
    error: null,
    diagnostic: null,
  }),
}));

vi.mock("@/lib/storage", () => storageMock);

vi.mock("@/api/tauri", () => tauriMock);

import { RecordingProvider } from "@/contexts/RecordingContext";
import {
  AI_POLISH_ENABLED_KEY,
  INPUT_DEVICE_STORAGE_KEY,
  INPUT_METHOD_KEY,
  RECORDING_MODE_KEY,
  SOUND_ENABLED_KEY,
} from "@/lib/constants";


const STORAGE_TRIGGERS: Record<string, string> = {
  [INPUT_METHOD_KEY]: "clipboard",
  [INPUT_DEVICE_STORAGE_KEY]: "mic1",
  [SOUND_ENABLED_KEY]: "false",
  [RECORDING_MODE_KEY]: "toggle",
  [AI_POLISH_ENABLED_KEY]: "true",
};

let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {

  storageMock.readLocalStorage.mockImplementation(() => null);


  tauriMock.setInputMethodCommand.mockResolvedValue(undefined);
  tauriMock.setInputDevice.mockResolvedValue(undefined);
  tauriMock.setSoundEnabled.mockResolvedValue(undefined);
  tauriMock.setRecordingMode.mockResolvedValue(undefined);
  tauriMock.setAiPolishConfig.mockResolvedValue(undefined);
  tauriMock.getAiPolishApiKey.mockResolvedValue("dummy-api-key");
  tauriMock.saveAiPolishApiKey.mockResolvedValue(undefined);

  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  consoleErrorSpy.mockRestore();
  vi.clearAllMocks();
});


async function flushPromises() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
  }
}

describe("RecordingProvider startup sync error reporting", () => {
  it("enables polish without reading or writing a provider credential", async () => {
    storageMock.readLocalStorage.mockImplementation(
      (key) => (key === AI_POLISH_ENABLED_KEY ? STORAGE_TRIGGERS[key] : null),
    );
    tauriMock.getAiPolishApiKey.mockReturnValue(new Promise<string>(() => {}));

    render(
      <RecordingProvider>
        <div />
      </RecordingProvider>,
    );
    await flushPromises();

    expect(tauriMock.getAiPolishApiKey).not.toHaveBeenCalled();
    expect(tauriMock.saveAiPolishApiKey).not.toHaveBeenCalled();
    expect(tauriMock.setAiPolishConfig.mock.calls).toEqual([[true]]);
  });

  it("reports setInputMethodCommand rejection via console.error", async () => {
    storageMock.readLocalStorage.mockImplementation(
      (key) => (key === INPUT_METHOD_KEY ? STORAGE_TRIGGERS[key] : null),
    );
    tauriMock.setInputMethodCommand.mockRejectedValueOnce(
      new Error("backend unavailable"),
    );

    render(
      <RecordingProvider>
        <div />
      </RecordingProvider>,
    );
    await flushPromises();

    expect(consoleErrorSpy).toHaveBeenCalled();
    const logged = consoleErrorSpy.mock.calls
      .map((args: unknown[]) => args.map((a) => String(a)).join(" "))
      .join("\n");

    expect(logged).toMatch(/setInputMethod|set_input_method/i);
  });

  it("reports setAiPolishConfig rejection via console.error and does not throw", async () => {
    storageMock.readLocalStorage.mockImplementation(
      (key) => (key === AI_POLISH_ENABLED_KEY ? STORAGE_TRIGGERS[key] : null),
    );
    tauriMock.setAiPolishConfig.mockRejectedValueOnce(
      new Error("backend unavailable"),
    );

    expect(() =>
      render(
        <RecordingProvider>
          <div />
        </RecordingProvider>,
      ),
    ).not.toThrow();
    await flushPromises();

    expect(tauriMock.setAiPolishConfig.mock.calls).toEqual([[true]]);
    expect(tauriMock.getAiPolishApiKey).not.toHaveBeenCalled();
    expect(consoleErrorSpy).toHaveBeenCalled();
  });

  it("does not call console.error when all backend calls succeed", async () => {
    storageMock.readLocalStorage.mockImplementation((key) => STORAGE_TRIGGERS[key] ?? null);

    render(
      <RecordingProvider>
        <div />
      </RecordingProvider>,
    );
    await flushPromises();

    expect(consoleErrorSpy).not.toHaveBeenCalled();
  });
});
