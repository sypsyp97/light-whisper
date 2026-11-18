import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import i18n from "@/i18n";
import { RecordingProvider } from "@/contexts/RecordingContext";
import MainPage from "@/pages/MainPage";

const events = await vi.hoisted(async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController();
});
const api = vi.hoisted(() => ({
  cancelDictationPolish: vi.fn<(sessionId: number) => Promise<boolean>>(async () => true),
  startRecording: vi.fn(async () => 1),
  stopRecording: vi.fn(async () => undefined),
  copyToClipboard: vi.fn(),
  hideMainWindow: vi.fn(),
  submitUserCorrection: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => events.module);
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));
vi.mock("@/hooks/useHotkey", () => ({
  useHotkey: () => ({ hotkeyDisplay: "Ctrl+Space" }),
}));
vi.mock("@/hooks/useModelStatus", () => ({
  useModelStatus: () => ({
    stage: "ready",
    isReady: true,
    device: "cuda",
    gpuName: "Test GPU",
    downloadProgress: 0,
    downloadMessage: null,
    isDownloading: false,
    error: null,
  }),
}));

beforeEach(async () => {
  localStorage.clear();
  events.reset();
  vi.clearAllMocks();
  await i18n.changeLanguage("en");
});

afterEach(() => vi.unstubAllGlobals());

function layout(showMain = true) {
  return <RecordingProvider>{showMain && <MainPage onNavigate={vi.fn()} />}</RecordingProvider>;
}

function recordingState(
  sessionId: number,
  revision: number,
  phase: "starting" | "recording" | "processing" | "idle",
  mode = "dictation",
) {
  act(() => events.emit("recording-state", {
    sessionId, revision, isStarting: phase === "starting", isRecording: phase === "recording",
    isProcessing: phase === "processing", mode,
  }));
}

function result(sessionId: number, text: string) {
  act(() => events.emit("transcription-result", { sessionId, text, interim: false, mode: "dictation" }));
}

function polish(sessionId: number, status: string) {
  act(() => events.emit("ai-polish-status", { sessionId, status }));
}

it("announces one persistent recording status driven by the active session's polish events", () => {
  const { container } = render(layout());
  const zone = container.querySelector<HTMLElement>(".recording-zone")!;
  const liveRegions = () => [...zone.querySelectorAll('[aria-live="polite"], [role="status"]')];
  const status = liveRegions()[0];
  const expectStatus = (expected: RegExp) => {
    expect.soft(liveRegions()).toHaveLength(1);
    expect.soft(liveRegions()[0]).toBe(status);
    expect.soft(liveRegions()[0]).toHaveTextContent(expected);
  };

  expect(liveRegions()).toHaveLength(1);
  expectStatus(/click to start recording/i);
  expect(status).not.toHaveTextContent("Test GPU");
  expect(screen.getByText("Test GPU")).toBeVisible();
  recordingState(30, 1, "starting");
  expectStatus(/connecting to microphone/i);
  recordingState(30, 2, "recording");
  expectStatus(/listening/i);
  recordingState(30, 3, "processing");
  expectStatus(/recognizing|transcribing/i);

  polish(29, "polishing");
  polish(31, "polishing");
  expectStatus(/recognizing|transcribing/i);
  polish(30, "polishing");
  expectStatus(/polishing/i);
  act(() => events.emit("ai-polish-status", { sessionId: 30, status: "error", message: "Transport retry" }));
  expectStatus(/polishing/i);
  polish(30, "fallback");
  expectStatus(/polishing/i);
  polish(30, "applied");
  expect(liveRegions()[0]).not.toHaveTextContent(/polishing/i);
  polish(30, "fallback");
  expect(liveRegions()[0]).not.toHaveTextContent(/polishing/i);
  recordingState(30, 4, "idle");
  polish(30, "polishing");
  expectStatus(/click to start recording/i);

  recordingState(31, 1, "processing");
  expectStatus(/recognizing|transcribing/i);
  polish(30, "polishing");
  expectStatus(/recognizing|transcribing/i);
  polish(31, "polishing");
  expectStatus(/polishing/i);
  result(31, "Final result");
  expect(liveRegions()[0]).not.toHaveTextContent(/polishing/i);
  polish(31, "polishing");
  expect(liveRegions()[0]).not.toHaveTextContent(/polishing/i);
});

it("keeps Use original visible during dictation processing and sends only that session to cancellation", async () => {
  const { container } = render(layout());
  const zone = within(container.querySelector<HTMLElement>(".recording-zone")!);
  const useOriginal = () => zone.queryByRole("button", { name: "Use original" });
  expect(useOriginal()).toBeNull();
  recordingState(40, 1, "processing", "assistant");
  expect(useOriginal()).toBeNull();

  recordingState(41, 1, "processing");
  const label = zone.getByText("Use original");
  const action = zone.getByRole("button", { name: "Use original" });
  expect(label).toBeVisible();
  expect(action).toContainElement(label);
  expect(zone.getByRole("button", { name: "Processing" })).toBeDisabled();
  await act(async () => fireEvent.click(action));
  expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(41);
  expect(api.startRecording).not.toHaveBeenCalled();
  expect(api.stopRecording).not.toHaveBeenCalled();

  recordingState(42, 1, "recording");
  expect(useOriginal()).toBeNull();
});

it.each([false, true])(
  "shows existing results immediately and animates only the new and newly archived results (reduced motion: %s)",
  (reducedMotion) => {
    vi.stubGlobal("matchMedia", () => ({ matches: reducedMotion }));
    const oldAnimate = Object.getOwnPropertyDescriptor(Element.prototype, "animate");
    const animate = vi.fn();
    Object.defineProperty(Element.prototype, "animate", { value: animate, configurable: true });
    try {
      const view = render(layout(false));
      result(50, "First result");
      result(51, "Second result");
      result(52, "Current result");
      view.rerender(layout());

      expect(screen.getByText("First result")).toBeVisible();
      expect(screen.getByText("Second result")).toBeVisible();
      expect(screen.getByDisplayValue("Current result")).toBeVisible();
      const rows = [...view.container.querySelectorAll<HTMLElement>(".history-item")];
      expect(rows).toHaveLength(2);
      expect.soft(rows.map((row) => row.style.animationDelay)).toEqual(["", ""]);
      expect(animate).not.toHaveBeenCalled();

      result(53, "Newest result");
      const archived = screen.getByText("Current result").closest(".history-item");
      const newest = screen.getByDisplayValue("Newest result");
      expect(archived).toBeVisible();
      expect(newest).toBeVisible();
      if (reducedMotion) {
        expect(animate).not.toHaveBeenCalled();
      } else {
        expect(animate.mock.contexts).toEqual([archived, newest]);
      }
      rows.forEach((row) => expect(row).toBeInTheDocument());
    } finally {
      if (oldAnimate) Object.defineProperty(Element.prototype, "animate", oldAnimate);
      else Reflect.deleteProperty(Element.prototype, "animate");
    }
  },
);
