import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type DownloadStatusEvent = {
  payload: {
    status: string;
    message?: string;
    progress?: number;
    error?: string;
    requestId?: string;
  };
};

type DownloadStatusHandler = (event: DownloadStatusEvent) => void;
const downloadHandlers = new Map<string, DownloadStatusHandler>();

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  check: vi.fn(),
  start: vi.fn(),
  restart: vi.fn(),
  download: vi.fn(),
  cancel: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/api/tauri", () => ({
  checkFunASRStatus: mocks.check,
  startFunASR: mocks.start,
  restartFunASR: mocks.restart,
  downloadModels: mocks.download,
  cancelModelDownload: mocks.cancel,
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));

import { useModelStatus } from "../useModelStatus";

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function advancePolling() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(18_000);
  });
}

describe("useModelStatus manual model downloads", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    downloadHandlers.clear();
    mocks.listen.mockImplementation((event: string, handler: DownloadStatusHandler) => {
      downloadHandlers.set(event, handler);
      return Promise.resolve(vi.fn());
    });
    mocks.check.mockResolvedValue({
      running: false,
      ready: false,
      models_present: false,
      model_state: "missing",
      device: "cpu",
      gpu_name: null,
    });
    mocks.start.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
    mocks.download.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  it("waits for user input when switching from a ready engine to an uncached model", async () => {
    mocks.check.mockResolvedValueOnce({ running: true, ready: true, device: "cloud" });
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    expect(hook.result.current.stage).toBe("ready");

    // Engine settings recheck the new selection with retryModel (the hook's retry).
    act(() => hook.result.current.retry());
    await flushMicrotasks();
    await advancePolling();

    expect(mocks.download).not.toHaveBeenCalled();
    expect(mocks.start).not.toHaveBeenCalled();
    expect(hook.result.current).toMatchObject({ stage: "need_download", isDownloading: false });
    hook.unmount();
  });

  it("ignores a cancelled download's late rejection and allows a new manual attempt", async () => {
    const pendingDownload = deferred<void>();
    const nextDownload = deferred<void>();
    mocks.download.mockReturnValueOnce(pendingDownload.promise).mockReturnValueOnce(nextDownload.promise);

    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    act(() => { hook.result.current.downloadModels(); });
    expect(mocks.download).toHaveBeenCalledOnce();

    await act(async () => {
      await hook.result.current.cancelDownload();
      pendingDownload.reject(new Error("download failed"));
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(mocks.cancel).toHaveBeenCalledOnce();
    expect(hook.result.current).toMatchObject({ stage: "need_download", isDownloading: false, error: null });
    await advancePolling();
    expect(mocks.download).toHaveBeenCalledOnce();
    act(() => { hook.result.current.downloadModels(); });
    expect(mocks.download).toHaveBeenCalledTimes(2);
    expect(hook.result.current.stage).toBe("downloading");

    const checksBeforeUnmount = mocks.check.mock.calls.length;
    hook.unmount();
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => { nextDownload.resolve(); });
    expect(mocks.check).toHaveBeenCalledTimes(checksBeforeUnmount);
  });

  it("offers a manual retry after a failed download without restarting the transfer on recheck", async () => {
    mocks.download.mockRejectedValueOnce(new Error("download failed"));
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await act(async () => { await hook.result.current.downloadModels(); });
    expect(hook.result.current).toMatchObject({ stage: "error", isDownloading: false, error: "download failed" });
    expect(mocks.download).toHaveBeenCalledOnce();

    act(() => hook.result.current.retry());
    await flushMicrotasks();
    await advancePolling();
    expect(mocks.download).toHaveBeenCalledOnce();
    expect(hook.result.current.stage).toBe("need_download");
    await act(async () => { await hook.result.current.downloadModels(); });
    expect(mocks.download).toHaveBeenCalledTimes(2);
    hook.unmount();
  });

  it("ignores a disposed download-status callback after StrictMode effect replay", async () => {
    const downloadHandlers: DownloadStatusHandler[] = [];
    mocks.check.mockResolvedValue({
      running: true,
      ready: true,
      device: "cpu",
      gpu_name: null,
    });
    mocks.listen.mockImplementation(
      (event: string, handler: DownloadStatusHandler) => {
        if (event === "model-download-status") downloadHandlers.push(handler);
        return Promise.resolve(vi.fn());
      },
    );

    const hook = renderHook(() => useModelStatus(), { wrapper: StrictMode });
    await flushMicrotasks();
    expect(downloadHandlers).toHaveLength(2);

    await act(async () => {
      downloadHandlers[0]({ payload: { status: "completed" } });
      await Promise.resolve();
    });

    expect(mocks.restart).not.toHaveBeenCalled();
    hook.unmount();
  });

  function visibleDownload(state: ReturnType<typeof useModelStatus>) {
    return {
      stage: state.stage,
      isDownloading: state.isDownloading,
      downloadProgress: state.downloadProgress,
      downloadMessage: state.downloadMessage,
      error: state.error,
    };
  }

  async function emitDownload(payload: DownloadStatusEvent["payload"]) {
    const handler = downloadHandlers.get("model-download-status");
    expect(handler).toBeTypeOf("function");
    await act(async () => { handler!({ payload }); });
  }

  it("assigns unique nonempty request identities across attempts and hook lifetimes", async () => {
    const pending = deferred<void>();
    mocks.download.mockReturnValue(pending.promise);
    const firstHook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    act(() => { firstHook.result.current.downloadModels(); });
    const firstId: unknown = mocks.download.mock.calls[0]?.[0];
    expect(firstId).toEqual(expect.any(String));
    expect(firstId).not.toBe("");

    await act(async () => { await firstHook.result.current.cancelDownload(); });
    expect(mocks.cancel).toHaveBeenLastCalledWith(firstId);
    act(() => { firstHook.result.current.downloadModels(); });
    const secondId: unknown = mocks.download.mock.calls[1]?.[0];
    expect(secondId).toEqual(expect.any(String));
    expect(secondId).not.toBe("");
    firstHook.unmount();

    const secondHook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    act(() => { secondHook.result.current.downloadModels(); });
    const thirdId: unknown = mocks.download.mock.calls[2]?.[0];
    expect(thirdId).toEqual(expect.any(String));
    expect(thirdId).not.toBe("");
    expect(new Set([firstId, secondId, thirdId]).size).toBe(3);
    secondHook.unmount();
    await act(async () => { pending.resolve(); });
  });

  it.each(["downloading", "progress", "completed", "cancelled", "error"])(
    "retires publication ownership immediately when cancellation starts, before a late %s event",
    async (status) => {
      const transfer = deferred<void>();
      const cancelAck = deferred<void>();
      mocks.download.mockReturnValueOnce(transfer.promise);
      mocks.cancel.mockReturnValueOnce(cancelAck.promise);
      const hook = renderHook(() => useModelStatus());
      await flushMicrotasks();
      act(() => { hook.result.current.downloadModels(); });
      const requestId: string | undefined = mocks.download.mock.calls[0]?.[0];
      let cancellation!: Promise<void>;
      act(() => { cancellation = Promise.resolve(hook.result.current.cancelDownload()); });
      const beforeLateEvent = visibleDownload(hook.result.current);

      await emitDownload({ requestId, status, progress: 65, message: "retired transfer", error: "retired error" });

      expect.soft(visibleDownload(hook.result.current)).toEqual(beforeLateEvent);
      expect.soft(mocks.restart).not.toHaveBeenCalled();
      await act(async () => { cancelAck.resolve(); await cancellation; transfer.resolve(); });
      expect(hook.result.current).toMatchObject({ stage: "need_download", isDownloading: false, error: null });
      hook.unmount();
    },
  );

  describe.each(["retired", "unversioned"] as const)("%s events during a replacement download", (eventOwner) => {
    it.each(["downloading", "progress", "completed", "cancelled", "error"])(
      "ignores old %s without clearing the current busy state or Promise owner",
      async (status) => {
        const oldTransfer = deferred<void>();
        const currentTransfer = deferred<void>();
        mocks.download.mockReturnValueOnce(oldTransfer.promise).mockReturnValueOnce(currentTransfer.promise);
        const hook = renderHook(() => useModelStatus());
        await flushMicrotasks();
        act(() => { hook.result.current.downloadModels(); });
        const oldId: string | undefined = mocks.download.mock.calls[0]?.[0];
        await act(async () => { await hook.result.current.cancelDownload(); });
        act(() => { hook.result.current.downloadModels(); });
        const currentId: string | undefined = mocks.download.mock.calls[1]?.[0];
        await emitDownload({ requestId: currentId, status: "progress", progress: 30, message: "current transfer" });
        expect(hook.result.current).toMatchObject({ stage: "downloading", isDownloading: true, downloadProgress: 30, downloadMessage: "current transfer" });
        const beforeOldEvent = visibleDownload(hook.result.current);

        await emitDownload({
          ...(eventOwner === "retired" ? { requestId: oldId } : {}),
          status, progress: 95, message: "obsolete transfer", error: "obsolete failure",
        });

        expect.soft(visibleDownload(hook.result.current)).toEqual(beforeOldEvent);
        expect.soft(mocks.restart).not.toHaveBeenCalled();
        act(() => { hook.result.current.downloadModels(); });
        expect.soft(mocks.download).toHaveBeenCalledTimes(2);
        await act(async () => { currentTransfer.reject(new Error("current transfer failed")); });
        expect.soft(hook.result.current).toMatchObject({ stage: "error", isDownloading: false, error: "current transfer failed" });
        hook.unmount();
        await act(async () => { oldTransfer.resolve(); });
      },
    );
  });

  it.each(["resolve", "reject"] as const)("keeps replacement ownership when the retired download Promise later %ss", async (outcome) => {
    const oldTransfer = deferred<void>();
    const currentTransfer = deferred<void>();
    mocks.download.mockReturnValueOnce(oldTransfer.promise).mockReturnValueOnce(currentTransfer.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    act(() => { hook.result.current.downloadModels(); });
    await act(async () => { await hook.result.current.cancelDownload(); });
    act(() => { hook.result.current.downloadModels(); });
    const currentId: string | undefined = mocks.download.mock.calls[1]?.[0];
    await emitDownload({ requestId: currentId, status: "progress", progress: 45, message: "replacement progress" });
    const beforeCompletion = visibleDownload(hook.result.current);
    const checksBeforeCompletion = mocks.check.mock.calls.length;

    await act(async () => {
      if (outcome === "resolve") oldTransfer.resolve();
      else oldTransfer.reject(new Error("retired transfer failed"));
    });

    expect(visibleDownload(hook.result.current)).toEqual(beforeCompletion);
    expect(mocks.check).toHaveBeenCalledTimes(checksBeforeCompletion);
    act(() => { hook.result.current.downloadModels(); });
    expect(mocks.download).toHaveBeenCalledTimes(2);
    await emitDownload({ requestId: currentId, status: "completed", progress: 100, message: "replacement complete" });
    expect(hook.result.current).toMatchObject({ stage: "loading", isDownloading: false, downloadProgress: 100, error: null });
    expect(mocks.restart).toHaveBeenCalledTimes(1);
    hook.unmount();
    await act(async () => { currentTransfer.resolve(); });
  });
});
