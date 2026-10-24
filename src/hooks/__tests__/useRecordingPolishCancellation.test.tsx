import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const events = await vi.hoisted(async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController();
});
const api = vi.hoisted(() => ({
  startRecording: vi.fn<() => Promise<number>>(),
  stopRecording: vi.fn<() => Promise<void>>(),
  cancelDictationPolish: vi.fn<(sessionId: number) => Promise<boolean>>(),
  copyToClipboard: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => events.module);
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));

import { useRecording } from "@/hooks/useRecording";

beforeEach(() => {
  events.reset();
  api.startRecording.mockReset();
  api.stopRecording.mockReset();
  api.cancelDictationPolish.mockReset();
});

async function flushSubscriptions() {
  await act(async () => { await Promise.resolve(); });
}

function recordingState(sessionId: number, revision: number, processing = true, mode = "dictation") {
  act(() => events.emit("recording-state", {
    sessionId,
    revision,
    isStarting: false,
    isRecording: !processing,
    isProcessing: processing,
    mode,
  }));
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  void promise.catch(() => undefined);
  return { promise, resolve, reject };
}

describe("useRecording explicit polish skip", () => {
  it("sends the latest processing session and waits for the backend to settle state", async () => {
    const pending = deferred<boolean>();
    api.cancelDictationPolish.mockReturnValue(pending.promise);
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    recordingState(61, 1);
    recordingState(62, 1);

    let cancellation!: Promise<void>;
    act(() => {
      cancellation = result.current.cancelPolish();
      void cancellation.catch(() => undefined);
    });

    expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(62);
    expect(result.current.isProcessing).toBe(true);
    await act(async () => {
      pending.resolve(true);
      await cancellation;
    });
    expect(result.current.isProcessing).toBe(true);

    act(() => events.emit("transcription-result", {
      sessionId: 62,
      text: "recognized original",
      originalText: "recognized original",
      interim: false,
      mode: "dictation",
    }));
    act(() => events.emit("recording-state", {
      sessionId: 62,
      revision: 2,
      isRecording: false,
      isProcessing: false,
      mode: "dictation",
    }));

    expect(result.current.isProcessing).toBe(false);
    expect(result.current.transcriptionResult).toBe("recognized original");
    expect(api.stopRecording).not.toHaveBeenCalled();
  });

  it.each([false, "rejected"] as const)(
    "keeps processing when cancellation is %s until the backend finishes",
    async (outcome) => {
      if (outcome === false) api.cancelDictationPolish.mockResolvedValue(false);
      else api.cancelDictationPolish.mockRejectedValue(new Error("cancel command failed"));
      const { result } = renderHook(() => useRecording());
      await flushSubscriptions();
      recordingState(63, 1);

      await act(async () => { await result.current.cancelPolish(); });

      expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(63);
      expect(result.current.isProcessing).toBe(true);
      expect(result.current.isRecording).toBe(false);
    },
  );

  it.each(["resolved", "rejected"] as const)(
    "ignores a %s cancellation response after a newer recording starts",
    async (outcome) => {
      const pending = deferred<boolean>();
      api.cancelDictationPolish.mockReturnValue(pending.promise);
      const { result } = renderHook(() => useRecording());
      await flushSubscriptions();
      recordingState(64, 1);
      let cancellation!: Promise<void>;
      act(() => {
        cancellation = result.current.cancelPolish();
        void cancellation.catch(() => undefined);
      });
      recordingState(65, 1, false);

      await act(async () => {
        if (outcome === "resolved") pending.resolve(true);
        else pending.reject(new Error("stale cancellation error"));
        await cancellation;
      });

      expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(64);
      expect(result.current.isRecording).toBe(true);
      expect(result.current.isProcessing).toBe(false);
      expect(result.current.error).toBeNull();
    },
  );

  it("does not send skip for an idle, recording, or assistant session", async () => {
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    await act(async () => { await result.current.cancelPolish(); });
    recordingState(66, 1, false);
    await act(async () => { await result.current.cancelPolish(); });
    recordingState(67, 1, true, "assistant");
    await act(async () => { await result.current.cancelPolish(); });

    expect(api.cancelDictationPolish).not.toHaveBeenCalled();
  });
});

describe("useRecording skip while stop is in flight", () => {
  it("queues skip until stop settles, then cancels the same session once", async () => {
    const pendingStop = deferred<void>();
    api.stopRecording.mockReturnValue(pendingStop.promise);
    api.cancelDictationPolish.mockResolvedValue(true);
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    recordingState(71, 1, false);
    let stopping!: Promise<void>;
    let cancellation!: Promise<void>;
    act(() => { stopping = result.current.stopRecording(); });
    expect(result.current.isProcessing).toBe(true);
    act(() => { cancellation = result.current.cancelPolish(); });

    expect(api.stopRecording).toHaveBeenCalledOnce();
    expect(api.cancelDictationPolish).not.toHaveBeenCalled();

    await act(async () => {
      pendingStop.resolve(undefined);
      await stopping;
      await cancellation;
    });

    expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(71);
    expect(result.current.isProcessing).toBe(true);
  });

  it("does not dispatch the queued skip if a newer recording starts first", async () => {
    const pendingStop = deferred<void>();
    api.stopRecording.mockReturnValue(pendingStop.promise);
    api.cancelDictationPolish.mockResolvedValue(true);
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    recordingState(72, 1, false);
    let stopping!: Promise<void>;
    let cancellation!: Promise<void>;
    act(() => { stopping = result.current.stopRecording(); });
    act(() => { cancellation = result.current.cancelPolish(); });
    recordingState(73, 1, false);

    await act(async () => {
      pendingStop.resolve(undefined);
      await stopping;
      await cancellation;
    });

    expect(api.cancelDictationPolish).not.toHaveBeenCalled();
    expect(result.current.isRecording).toBe(true);
    expect(result.current.isProcessing).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it("does not dispatch skip when stop fails and keeps the stop error", async () => {
    const pendingStop = deferred<void>();
    const stopError = new Error("stop IPC rejected");
    api.stopRecording.mockReturnValue(pendingStop.promise);
    api.cancelDictationPolish.mockResolvedValue(true);
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    recordingState(74, 1, false);
    let stopping!: Promise<void>;
    let cancellation!: Promise<void>;
    act(() => { stopping = result.current.stopRecording(); });
    act(() => { cancellation = result.current.cancelPolish(); });

    await act(async () => {
      pendingStop.reject(stopError);
      await stopping;
      await cancellation;
    });

    expect(api.cancelDictationPolish).not.toHaveBeenCalled();
    expect(result.current.error).toBe(stopError.message);
    expect(result.current.isRecording).toBe(false);
    expect(result.current.isProcessing).toBe(false);
  });

  it("cancels newer processing without waiting for an older stop or applying its failure", async () => {
    const pendingStop = deferred<void>();
    api.stopRecording.mockReturnValue(pendingStop.promise);
    api.cancelDictationPolish.mockResolvedValue(true);
    const { result } = renderHook(() => useRecording());
    await flushSubscriptions();
    recordingState(75, 1, false);
    let stopping!: Promise<void>;
    let cancellation!: Promise<void>;
    act(() => { stopping = result.current.stopRecording(); });
    recordingState(76, 1);
    act(() => { cancellation = result.current.cancelPolish(); });

    expect.soft(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(76);

    await act(async () => {
      pendingStop.reject(new Error("older stop IPC rejected"));
      await stopping;
      await cancellation;
    });

    expect.soft(result.current.isProcessing).toBe(true);
    expect.soft(result.current.error).toBeNull();
    expect(api.cancelDictationPolish).toHaveBeenCalledExactlyOnceWith(76);
  });
});
