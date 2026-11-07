import { StrictMode } from "react";
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type DownloadStatusEvent = {
  payload: {
    status: string;
    message?: string;
    progress?: number;
    error?: string;
  };
};

type DownloadStatusHandler = (event: DownloadStatusEvent) => void;

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
    mocks.listen.mockImplementation(() => Promise.resolve(vi.fn()));
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
});
