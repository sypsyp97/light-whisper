/**
 * TDD red-state tests for `RecordingContext`.
 *
 * Contract: startup sync failures in the provider's useEffect
 * (setInputMethodCommand / setInputDevice / setSoundEnabled /
 * setRecordingMode / getAiPolishApiKey) must be surfaced via a
 * observable channel — specifically `console.error` — instead of
 * silently swallowed with `.catch(() => {})`.
 *
 * These tests are expected to FAIL against the current implementation in
 * `RecordingContext.tsx` and pass once the five `.catch(() => {})` calls
 * are replaced with something that reports the failure.
 */
import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// --- mocks: hoisted so they apply before provider import ---

const storageMock = vi.hoisted(() => ({
  readLocalStorage: vi.fn<(key: string) => string | null>(),
  writeLocalStorage: vi.fn<(key: string, value: string) => void>(),
}));

const tauriMock = vi.hoisted(() => ({
  setInputMethodCommand: vi.fn<(method: string) => Promise<void>>(),
  setInputDevice: vi.fn<(name?: string | null) => Promise<void>>(),
  setSoundEnabled: vi.fn<(enabled: boolean) => Promise<void>>(),
  setRecordingMode: vi.fn<(toggle: boolean) => Promise<void>>(),
  setAiPolishConfig: vi.fn<(enabled: boolean, apiKey: string) => Promise<void>>(),
  getAiPolishApiKey: vi.fn<() => Promise<string>>(),
}));

// Stub the three hooks pulled in by RecordingProvider so we don't spin up
// real Tauri listeners / pollers.
vi.mock("@/hooks/useRecording", () => ({
  useRecording: () => ({
    isRecording: false,
    isProcessing: false,
    startRecording: vi.fn(),
    stopRecording: vi.fn(),
    error: null,
    transcriptionResult: null,
    setTranscriptionResult: vi.fn(),
    originalAsrText: null,
    setOriginalAsrText: vi.fn(),
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

// Map keys -> the values that trigger each sync path.
const STORAGE_TRIGGERS: Record<string, string> = {
  [INPUT_METHOD_KEY]: "clipboard",
  [INPUT_DEVICE_STORAGE_KEY]: "mic1",
  [SOUND_ENABLED_KEY]: "false",
  [RECORDING_MODE_KEY]: "toggle",
  [AI_POLISH_ENABLED_KEY]: "true",
};

let consoleErrorSpy: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  // Default every read to `null` — tests opt-in to specific keys.
  storageMock.readLocalStorage.mockImplementation(() => null);

  // Default every Tauri call to succeed so tests opt-in to failures.
  tauriMock.setInputMethodCommand.mockResolvedValue(undefined);
  tauriMock.setInputDevice.mockResolvedValue(undefined);
  tauriMock.setSoundEnabled.mockResolvedValue(undefined);
  tauriMock.setRecordingMode.mockResolvedValue(undefined);
  tauriMock.setAiPolishConfig.mockResolvedValue(undefined);
  tauriMock.getAiPolishApiKey.mockResolvedValue("dummy-api-key");

  consoleErrorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  consoleErrorSpy.mockRestore();
  vi.clearAllMocks();
});

/** Wait for queued microtasks so provider's useEffect promises settle. */
async function flushPromises() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
  }
}

describe("RecordingProvider startup sync error reporting", () => {
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
    // Must mention the failing command in some observable form.
    expect(logged).toMatch(/setInputMethod|set_input_method/i);
  });

  it("reports getAiPolishApiKey rejection via console.error and does not throw", async () => {
    storageMock.readLocalStorage.mockImplementation(
      (key) => (key === AI_POLISH_ENABLED_KEY ? STORAGE_TRIGGERS[key] : null),
    );
    tauriMock.getAiPolishApiKey.mockRejectedValueOnce(
      new Error("keyring locked"),
    );

    expect(() =>
      render(
        <RecordingProvider>
          <div />
        </RecordingProvider>,
      ),
    ).not.toThrow();
    await flushPromises();

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
