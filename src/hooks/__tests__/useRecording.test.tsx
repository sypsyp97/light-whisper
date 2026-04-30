/**
 * Tests for `useRecording`.
 *
 * Note on state:
 *
 *   - The four session-ID filter assertions (late-arriving final must not
 *     overwrite current display, newer session's text must not be demoted,
 *     history must include both sessions, older recording-state must be
 *     ignored) are **characterization / regression tests**. They correspond
 *     to the existing `latestSessionIdRef` / `latestDisplayedFinalSessionIdRef`
 *     protections in `useRecording.ts` and should pass against the current
 *     implementation — they lock that behaviour in place against future
 *     refactors.
 *
 *   - The `stopRecording` return-type test locks in the current public
 *     contract: the hook resolves to `undefined` after the Tauri command
 *     completes.
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Hoisted: shared handler registry for the mocked `@tauri-apps/api/event`.
const tauriEvents = vi.hoisted(() => {
  type Handler = (event: { payload: unknown }) => void;
  const handlers = new Map<string, Set<Handler>>();
  const listen = async (event: string, handler: Handler) => {
    let set = handlers.get(event);
    if (!set) {
      set = new Set();
      handlers.set(event, set);
    }
    set.add(handler);
    return () => {
      set?.delete(handler);
    };
  };
  const emit = (event: string, payload: unknown) => {
    const set = handlers.get(event);
    if (!set) return;
    for (const h of Array.from(set)) h({ payload });
  };
  const reset = () => handlers.clear();
  return { listen, emit, reset };
});

const tauriInvokeMocks = vi.hoisted(() => ({
  startRecording: vi.fn<() => Promise<number>>(),
  stopRecording: vi.fn<() => Promise<void>>(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvents.listen,
}));

vi.mock("@/api/tauri", () => ({
  startRecording: tauriInvokeMocks.startRecording,
  stopRecording: tauriInvokeMocks.stopRecording,
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/i18n", () => ({
  default: {
    t: (key: string) => key,
  },
}));

import { useRecording } from "@/hooks/useRecording";

beforeEach(() => {
  tauriEvents.reset();
  tauriInvokeMocks.startRecording.mockReset();
  tauriInvokeMocks.stopRecording.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

/** Wait until the most recently registered `listen()` handler resolves. */
async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useRecording session-ID filtering (characterization / regression)", () => {
  it("late-arriving final result from an older session must NOT overwrite current display", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(5);
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(6);
    tauriInvokeMocks.stopRecording.mockResolvedValue(undefined);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    // Session 5 start
    await act(async () => {
      await result.current.startRecording();
    });
    // Session 6 start (becomes the current session)
    await act(async () => {
      await result.current.startRecording();
    });

    // Late final for session 5 — must be filtered from display
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 5,
        text: "旧结果",
        interim: false,
      });
    });

    // Final for current session 6
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 6,
        text: "新结果",
        interim: false,
      });
    });

    await waitFor(() => {
      expect(result.current.transcriptionResult).toBe("新结果");
    });
  });

  it("newer session's final payload must not be demoted by a later-arriving older-session final", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(7);
    tauriInvokeMocks.stopRecording.mockResolvedValue(undefined);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    await act(async () => {
      await result.current.startRecording();
    });

    // Session 7 final first
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 7,
        text: "hello",
        interim: false,
      });
    });

    // Then a late final from older session 6 — must not replace "hello"
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 6,
        text: "stale",
        interim: false,
      });
    });

    await waitFor(() => {
      expect(result.current.transcriptionResult).toBe("hello");
    });
  });

  it("history should include both old and new session results (session staleness only affects display)", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(5);
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(6);
    tauriInvokeMocks.stopRecording.mockResolvedValue(undefined);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    await act(async () => {
      await result.current.startRecording();
    });
    await act(async () => {
      await result.current.startRecording();
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 5,
        text: "旧结果",
        interim: false,
      });
    });
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 6,
        text: "新结果",
        interim: false,
      });
    });

    await waitFor(() => {
      const texts = result.current.history.map((h) => h.text);
      expect(texts).toContain("新结果");
      expect(texts).toContain("旧结果");
    });
  });

  it("stores edit-grab status from the current final result and preserves it in history", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(11);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    await act(async () => {
      await result.current.startRecording();
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 11,
        text: "current final",
        interim: false,
        editGrabStatus: "timeout",
      });
    });

    await waitFor(() => {
      expect(result.current.transcriptionResult).toBe("current final");
      expect(result.current.editGrabStatus).toBe("timeout");
      expect(result.current.history[0].editGrabStatus).toBe("timeout");
    });
  });

  it("keeps stale final edit-grab status in history without replacing the current displayed status", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(12);
    tauriInvokeMocks.startRecording.mockResolvedValueOnce(13);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    await act(async () => {
      await result.current.startRecording();
    });
    await act(async () => {
      await result.current.startRecording();
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 13,
        text: "new result",
        interim: false,
        editGrabStatus: "ok",
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 12,
        text: "stale result",
        interim: false,
        editGrabStatus: "empty",
      });
    });

    await waitFor(() => {
      expect(result.current.transcriptionResult).toBe("new result");
      expect(result.current.editGrabStatus).toBe("ok");
      const historyByText = new Map(result.current.history.map((item) => [item.text, item]));
      expect(historyByText.get("new result")?.editGrabStatus).toBe("ok");
      expect(historyByText.get("stale result")?.editGrabStatus).toBe("empty");
    });
  });

  it.each(["ok", "timeout", "empty", "unsupported"] as const)(
    "accepts %s as a frontend-visible edit-grab status",
    async (editGrabStatus) => {
      const { result } = renderHook(() => useRecording());
      await flushMicrotasks();

      await act(async () => {
        tauriEvents.emit("transcription-result", {
          sessionId: 20,
          text: `status ${editGrabStatus}`,
          interim: false,
          editGrabStatus,
        });
      });

      await waitFor(() => {
        expect(result.current.editGrabStatus).toBe(editGrabStatus);
        expect(result.current.history[0].editGrabStatus).toBe(editGrabStatus);
      });
    }
  );

  it("keeps both raw ASR text and editable baseline after a final result arrives", async () => {
    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 8,
        text: "润色后的结果",
        originalText: "润色前原文",
        interim: false,
      });
    });

    await waitFor(() => {
      expect(result.current.transcriptionResult).toBe("润色后的结果");
      expect(result.current.originalAsrText).toBe("润色前原文");
      expect(result.current.editBaselineText).toBe("润色后的结果");
    });
  });

  it("recording-state from an older session must be ignored", async () => {
    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    // Session 10 marks recording=true
    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });
    expect(result.current.isRecording).toBe(true);

    // Older session 5 tries to mark recording=false — must be ignored
    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 5,
        isRecording: false,
        isProcessing: false,
      });
    });
    expect(result.current.isRecording).toBe(true);
  });
});

describe("useRecording stopRecording return type", () => {
  it("stopRecording resolves to undefined (not null)", async () => {
    tauriInvokeMocks.startRecording.mockResolvedValue(1);
    tauriInvokeMocks.stopRecording.mockResolvedValue(undefined);

    const { result } = renderHook(() => useRecording());
    await flushMicrotasks();

    let resolved: unknown = "not-set";
    await act(async () => {
      resolved = await result.current.stopRecording();
    });

    expect(resolved).toBeUndefined();
  });
});
