import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    events.handlers.set(event, handler);
    return () => events.handlers.delete(event);
  },
}));
vi.mock("@/api/tauri", () => ({
  startRecording: vi.fn(), stopRecording: vi.fn(), copyToClipboard: vi.fn(),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));
vi.mock("@/i18n", () => ({
  default: { t: (key: string) => key, language: "en" },
}));

import { useRecording } from "@/hooks/useRecording";

beforeEach(() => events.handlers.clear());

function emit(event: string, payload: unknown) {
  act(() => events.handlers.get(event)?.({ payload }));
}

function finalResult(sessionId: number, text = "Final edit") {
  return {
    sessionId, text, originalText: "Original ASR", interim: false,
    mode: "dictation", durationSec: 2.5, charCount: text.length,
    language: "en", editGrabStatus: "ok",
    timing: { asrMs: 40, polishMs: 60, totalMs: 140 },
  };
}

describe("useRecording automatic output timing", () => {
  it("updates the matching final and history timing without changing content or metadata", () => {
    const { result } = renderHook(() => useRecording());
    emit("transcription-result", finalResult(41));
    const before = result.current;
    const historyBefore = before.history[0];

    emit("transcription-timing", { sessionId: 41, totalMs: 460 });

    expect(result.current.timing).toEqual({ asrMs: 40, polishMs: 60, totalMs: 460 });
    expect(result.current.history).toEqual([{
      ...historyBefore, timing: { asrMs: 40, polishMs: 60, totalMs: 460 },
    }]);
    for (const key of ["transcriptionResult", "originalAsrText", "editBaselineText",
      "durationSec", "charCount", "detectedLanguage", "editGrabStatus", "resultMode"] as const) {
      expect(result.current[key]).toEqual(before[key]);
    }
  });

  it("updates older history after a newer final without replacing the displayed result", () => {
    const { result } = renderHook(() => useRecording());
    emit("transcription-result", finalResult(41, "Older dictation"));
    emit("transcription-result", finalResult(42, "Newer answer"));
    const displayedTiming = result.current.timing;
    const newerHistory = result.current.history[0];

    emit("transcription-timing", { sessionId: 41, totalMs: 900 });

    expect(result.current.transcriptionResult).toBe("Newer answer");
    expect(result.current.timing).toEqual(displayedTiming);
    expect(result.current.history[0]).toEqual(newerHistory);
    expect(result.current.history[1]).toEqual(expect.objectContaining({
      id: "session-41", text: "Older dictation",
      timing: { asrMs: 40, polishMs: 60, totalMs: 900 },
    }));
  });

  it("updates older history while a newer recording is active without changing displayed timing", () => {
    const { result } = renderHook(() => useRecording());
    emit("transcription-result", finalResult(41, "Previous result"));
    const displayedTiming = result.current.timing;
    emit("recording-state", {
      sessionId: 42, revision: 1, isRecording: true, isProcessing: false,
    });

    emit("transcription-timing", { sessionId: 41, totalMs: 900 });

    expect(result.current.isRecording).toBe(true);
    expect(result.current.transcriptionResult).toBe("Previous result");
    expect(result.current.timing).toEqual(displayedTiming);
    expect(result.current.history[0].timing?.totalMs).toBe(900);
  });

  it("retains timing received before the final without creating placeholder history", () => {
    const { result } = renderHook(() => useRecording());
    emit("recording-state", {
      sessionId: 41, revision: 1, isRecording: false, isProcessing: true, mode: "dictation",
    });
    emit("transcription-timing", { sessionId: 41, totalMs: 460 });
    expect(result.current.transcriptionResult).toBeNull();
    expect(result.current.history).toEqual([]);

    emit("transcription-result", finalResult(41));

    expect(result.current.timing).toEqual({ asrMs: 40, polishMs: 60, totalMs: 460 });
    expect(result.current.history[0].timing).toEqual(result.current.timing);
    expect(result.current.history[0].text).toBe("Final edit");
  });

  it("keeps an older pre-final timing scoped to its late final after a newer result", () => {
    const { result } = renderHook(() => useRecording());
    emit("recording-state", {
      sessionId: 41, revision: 1, isRecording: false, isProcessing: true,
    });
    emit("transcription-timing", { sessionId: 41, totalMs: 460 });
    emit("transcription-result", finalResult(42, "Current result"));
    emit("transcription-result", finalResult(41, "Late result"));

    expect(result.current.transcriptionResult).toBe("Current result");
    expect(result.current.timing?.totalMs).toBe(140);
    expect(result.current.history.find(({ id }) => id === "session-41")?.timing?.totalMs).toBe(460);
  });

  it("retains the latest twenty pre-final timings and evicts older unmatched sessions", () => {
    const { result } = renderHook(() => useRecording());
    for (let sessionId = 1; sessionId <= 21; sessionId++) {
      emit("recording-state", {
        sessionId, revision: 1, isRecording: false, isProcessing: true,
      });
      emit("transcription-timing", { sessionId, totalMs: 1000 + sessionId });
    }
    expect(result.current.history).toEqual([]);

    emit("transcription-result", finalResult(1, "Evicted timing"));
    expect(result.current.history[0].timing?.totalMs).toBe(140);
    for (let sessionId = 2; sessionId <= 21; sessionId++) {
      emit("transcription-result", finalResult(sessionId, `Final ${sessionId}`));
      expect(result.current.history.find(({ id }) => id === `session-${sessionId}`)?.timing?.totalMs)
        .toBe(1000 + sessionId);
    }
    expect(result.current.transcriptionResult).toBe("Final 21");
    expect(result.current.timing?.totalMs).toBe(1021);
    expect(result.current.history).toHaveLength(20);
  });

  it("rejects malformed session IDs and timing values without mutating current history", () => {
    const { result } = renderHook(() => useRecording());
    emit("transcription-result", finalResult(41));
    const historyBefore = result.current.history;
    const timingBefore = result.current.timing;
    for (const sessionId of [0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1,
      "41", "", null, undefined]) {
      emit("transcription-timing", { sessionId, totalMs: 460 });
    }
    for (const totalMs of [-1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1,
      "460", "", null, undefined]) {
      emit("transcription-timing", { sessionId: 41, totalMs });
    }
    emit("transcription-timing", { sessionId: 17, totalMs: 460 });

    expect(result.current.transcriptionResult).toBe("Final edit");
    expect(result.current.timing).toEqual(timingBefore);
    expect(result.current.history).toEqual(historyBefore);
  });

  it("unregisters the timing listener on unmount", async () => {
    const { result, unmount } = renderHook(() => useRecording());
    await act(async () => { await Promise.resolve(); });
    emit("transcription-result", finalResult(41));
    const handler = events.handlers.get("transcription-timing");
    expect(handler).toBeDefined();
    const before = result.current;
    unmount();
    expect(events.handlers.has("transcription-timing")).toBe(false);

    act(() => handler!({ payload: { sessionId: 41, totalMs: 460 } }));

    expect(result.current).toBe(before);
  });
});
