import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { toast, Toaster } from "sonner";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  copyToClipboard: vi.fn<(text: string) => Promise<string>>(),
  pasteText: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    mocks.handlers.set(event, handler);
    return () => mocks.handlers.delete(event);
  },
}));
vi.mock("@/api/tauri", () => ({
  startRecording: vi.fn(),
  stopRecording: vi.fn(),
  copyToClipboard: mocks.copyToClipboard,
  pasteText: mocks.pasteText,
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));

import { useRecording } from "@/hooks/useRecording";

beforeEach(() => {
  mocks.handlers.clear();
  mocks.pasteText.mockReset();
  mocks.copyToClipboard.mockReset().mockResolvedValue("copied");
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

async function advance(ms: number) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

function emit(event: string, payload: unknown) {
  act(() => mocks.handlers.get(event)?.({ payload }));
}

describe("useRecording deferred output recovery", () => {
  it.each(["target_changed", "target_unknown", "paste_failed"])(
    "keeps older dictation recoverable after a later assistant session for %s",
    async (reason) => {
      render(<Toaster />);
      const { result } = renderHook(() => useRecording());
      await act(async () => { await Promise.resolve(); });

      emit("recording-state", {
        sessionId: 42,
        revision: 2,
        isRecording: true,
        isProcessing: false,
        mode: "assistant",
      });
      emit("recording-state", {
        sessionId: 42,
        revision: 3,
        isRecording: false,
        isProcessing: true,
        mode: "assistant",
      });
      emit("transcription-result", {
        sessionId: 42,
        text: "Assistant answer",
        interim: false,
        mode: "assistant",
      });
      emit("recording-state", {
        sessionId: 42,
        revision: 4,
        isRecording: false,
        isProcessing: false,
        mode: "assistant",
      });

      const originalText = "  旧听写 A\nOriginal punctuation!  ";
      emit("paste-recovery", { sessionId: 41, text: originalText, reason });

      expect(result.current.transcriptionResult).toBe("Assistant answer");
      expect(result.current.history.map(({ text }) => text)).toEqual(["Assistant answer"]);
      expect(result.current.isRecording).toBe(false);
      expect(result.current.isProcessing).toBe(false);
      expect(result.current.error).toBeNull();
      expect(result.current.resultMode).toBe("assistant");
      const copy = await screen.findByRole("button", { name: "common.copy" });
      expect(mocks.copyToClipboard).not.toHaveBeenCalled();
      expect(mocks.pasteText).not.toHaveBeenCalled();

      fireEvent.click(copy);
      expect(mocks.copyToClipboard).toHaveBeenCalledExactlyOnceWith(originalText);
      expect(mocks.pasteText).not.toHaveBeenCalled();
      expect(result.current.transcriptionResult).toBe("Assistant answer");
      expect(result.current.history.map(({ text }) => text)).toEqual(["Assistant answer"]);
      expect(result.current.isRecording).toBe(false);
      expect(result.current.isProcessing).toBe(false);
      expect(result.current.error).toBeNull();
      expect(result.current.resultMode).toBe("assistant");
    },
  );

  it("expires a previous dictation warning while the app is hidden", async () => {
    vi.useFakeTimers();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    render(<Toaster toastOptions={{ duration: 1800 }} />);
    const { result } = renderHook(() => useRecording());
    emit("paste-recovery", { sessionId: 41, text: "Earlier result", reason: "paste_failed" });
    await advance(0);

    // Give the user time to use Copy beyond the ordinary brief status toast.
    await advance(2500);
    expect(screen.getByRole("button", { name: "common.copy" })).toBeInTheDocument();
    emit("transcription-result", { sessionId: 42, text: "Later result", interim: false, mode: "dictation" });
    await advance(10_000);

    hidden.mockReturnValue(false);
    fireEvent(document, new Event("visibilitychange"));
    expect(screen.queryByText("toast.pasteRecovery")).not.toBeInTheDocument();
    expect(result.current.transcriptionResult).toBe("Later result");
    expect(mocks.copyToClipboard).not.toHaveBeenCalled();
    expect(mocks.pasteText).not.toHaveBeenCalled();
  });

  it("dismisses only already shown older warnings when output succeeds", async () => {
    vi.useFakeTimers();
    render(<Toaster />);
    renderHook(() => useRecording());
    emit("api-warning", { message: "Unrelated API warning" });
    for (const id of [51, 52, 54]) {
      emit("paste-recovery", { sessionId: id, text: `Result ${id}`, reason: "target_changed" });
    }
    await advance(0);
    expect(screen.getAllByRole("button", { name: "common.copy" })).toHaveLength(3);

    // A final transcript precedes actual output. A malformed receipt is no proof of success either.
    emit("transcription-result", { sessionId: 53, text: "Next dictation", interim: false });
    emit("transcription-timing", { sessionId: 53, totalMs: -1 });
    await advance(300);
    expect(screen.getAllByRole("button", { name: "common.copy" })).toHaveLength(3);

    emit("transcription-timing", { sessionId: 53, totalMs: 120 });
    await advance(0); // Commit the dismissal before Sonner starts its exit animation.
    await advance(300); // Includes the toast exit animation.
    const remaining = screen.getAllByRole("button", { name: "common.copy" });
    expect(remaining).toHaveLength(1);
    expect(screen.getByText("Unrelated API warning")).toBeInTheDocument();
    fireEvent.click(remaining[0]);
    expect(mocks.copyToClipboard).toHaveBeenCalledExactlyOnceWith("Result 54");
    expect(mocks.pasteText).not.toHaveBeenCalled();
  });

  it("leaves unrelated notifications with numeric and numeric-string IDs intact", async () => {
    vi.useFakeTimers();
    render(<Toaster />);
    renderHook(() => useRecording());
    act(() => {
      toast.info("Number ID"); // Sonner generates numeric IDs by default.
      toast.info("String ID", { id: "12" });
    });
    await advance(0);

    expect(() => emit("transcription-timing", { sessionId: 43, totalMs: 300 })).not.toThrow();
    await advance(0);
    await advance(300);
    expect(screen.getByText("Number ID")).toBeInTheDocument();
    expect(screen.getByText("String ID")).toBeInTheDocument();
  });

  it("retires a recovery already issued when success arrives before Sonner paints", async () => {
    vi.useFakeTimers();
    render(<Toaster />);
    renderHook(() => useRecording());
    emit("paste-recovery", { sessionId: 71, text: "Queued warning", reason: "paste_failed" });
    emit("transcription-timing", { sessionId: 72, totalMs: 100 });
    await advance(0); // Allow queued toast updates to render before advancing the exit animation.
    await advance(300);
    expect(screen.queryByText("toast.pasteRecovery")).not.toBeInTheDocument();
    expect(mocks.pasteText).not.toHaveBeenCalled();
  });

  it("offers a genuine older failure delivered after a later output succeeded", async () => {
    render(<Toaster />);
    renderHook(() => useRecording());
    emit("transcription-timing", { sessionId: 62, totalMs: 100 });
    const original = "  delayed 61\n";
    emit("paste-recovery", { sessionId: 61, text: original, reason: "paste_failed" });
    fireEvent.click(await screen.findByRole("button", { name: "common.copy" }));
    expect(mocks.copyToClipboard).toHaveBeenCalledExactlyOnceWith(original);
    expect(mocks.pasteText).not.toHaveBeenCalled();
  });

  it("ignores a recovery event after the hook unmounts", async () => {
    const warning = vi.spyOn(toast, "warning");
    const { unmount } = renderHook(() => useRecording());
    await act(async () => { await Promise.resolve(); });
    const handler = mocks.handlers.get("paste-recovery");
    expect(handler).toBeDefined();
    unmount();

    act(() => handler!({
      payload: { sessionId: 41, text: "Retired dictation", reason: "target_changed" },
    }));

    expect(warning).not.toHaveBeenCalled();
    expect(mocks.copyToClipboard).not.toHaveBeenCalled();
  });
});
