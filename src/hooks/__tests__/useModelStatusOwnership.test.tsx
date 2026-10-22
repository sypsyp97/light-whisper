import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FunASRStatus } from "@/types";

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

type StatusPayload = {
  status: string;
  message?: string;
  device?: string | null;
  gpu_name?: string | null;
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function status(overrides: Partial<FunASRStatus> = {}): FunASRStatus {
  return {
    running: true,
    ready: false,
    model_loaded: false,
    models_present: true,
    model_state: "ready",
    message: "",
    device: "cpu",
    gpu_name: "poll GPU",
    ...overrides,
  };
}

async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function advance(milliseconds: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(milliseconds);
  });
}

describe("useModelStatus status ownership", () => {
  let handler: ((event: { payload: StatusPayload }) => void) | undefined;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    handler = undefined;
    mocks.listen.mockImplementation((event: string, callback: typeof handler) => {
      if (event === "funasr-status") handler = callback;
      return Promise.resolve(vi.fn());
    });
    mocks.check.mockResolvedValue(status());
    mocks.start.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
    mocks.download.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  function emit(payload: StatusPayload) {
    expect(handler).toBeDefined();
    act(() => handler?.({ payload }));
  }

  it("keeps a ready event after older loading and rejected queries settle, with polling stopped", async () => {
    const olderLoading = deferred<FunASRStatus>();
    const olderFailure = deferred<FunASRStatus>();
    mocks.check.mockReturnValueOnce(olderLoading.promise).mockReturnValueOnce(olderFailure.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(2);

    emit({ status: "ready", device: "cuda", gpu_name: "event GPU" });
    expect(hook.result.current).toMatchObject({ stage: "ready", isReady: true, error: null });
    await act(async () => { olderLoading.resolve(status()); });
    expect(hook.result.current).toMatchObject({
      stage: "ready", device: "cuda", gpuName: "event GPU", error: null,
    });

    await act(async () => { olderFailure.reject(new Error("obsolete status failure")); });
    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "event GPU", error: null,
    });
    await advance(18_000);
    expect(mocks.check).toHaveBeenCalledTimes(2);
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();
  });

  it.each(["need_api_key", "error"])(
    "keeps a %s event and its device/error when an older ready query completes",
    async (eventStatus) => {
      const olderReady = deferred<FunASRStatus>();
      mocks.check.mockReturnValueOnce(olderReady.promise);
      const hook = renderHook(() => useModelStatus());
      await flushMicrotasks();

      emit({ status: eventStatus, message: "current backend failure", device: "cloud", gpu_name: null });
      expect(hook.result.current.stage).toBe("error");
      await act(async () => { olderReady.resolve(status({ ready: true })); });

      expect(hook.result.current).toMatchObject({
        stage: "error", isReady: false, error: "current backend failure", device: "cloud", gpuName: null,
      });
      await advance(18_000);
      expect(mocks.check).toHaveBeenCalledOnce();
      expect(mocks.start).not.toHaveBeenCalled();
    },
  );

  it("lets the latest overlapping query win without an old ready result or rejection stopping recovery polling", async () => {
    const olderReady = deferred<FunASRStatus>();
    const olderFailure = deferred<FunASRStatus>();
    const currentLoading = deferred<FunASRStatus>();
    mocks.check
      .mockReturnValueOnce(olderReady.promise)
      .mockReturnValueOnce(olderFailure.promise)
      .mockReturnValueOnce(currentLoading.promise)
      .mockResolvedValue(status({ ready: true, device: "cuda", gpu_name: "current GPU" }));
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(12_000);
    expect(mocks.check).toHaveBeenCalledTimes(3);

    await act(async () => {
      currentLoading.resolve(status({ device: "cuda", gpu_name: "current GPU" }));
    });
    await act(async () => { olderReady.resolve(status({ ready: true })); });
    expect(hook.result.current).toMatchObject({
      stage: "loading", device: "cuda", gpuName: "current GPU", error: null,
    });
    await act(async () => { olderFailure.reject(new Error("obsolete poll failure")); });
    expect(hook.result.current).toMatchObject({ stage: "loading", error: null });

    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "current GPU", error: null,
    });
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
  });

  it("does not start the backend or auto-download from stopped/missing queries superseded by loading", async () => {
    const olderStopped = deferred<FunASRStatus>();
    const olderMissing = deferred<FunASRStatus>();
    mocks.check.mockReturnValueOnce(olderStopped.promise).mockReturnValueOnce(olderMissing.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(6_000);

    emit({ status: "loading", device: "cuda", gpu_name: "event GPU" });
    await act(async () => { olderStopped.resolve(status({ running: false })); });
    expect(mocks.start).not.toHaveBeenCalled();
    await act(async () => {
      olderMissing.resolve(status({ running: false, models_present: false, model_state: "missing" }));
    });
    await advance(0);

    expect(hook.result.current).toMatchObject({
      stage: "loading", device: "cuda", gpuName: "event GPU", error: null, isDownloading: false,
    });
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();
  });

  it("ignores an obsolete third start rejection after a ready event", async () => {
    const olderStart = deferred<void>();
    mocks.check.mockResolvedValue(status({ running: false }));
    mocks.start
      .mockRejectedValueOnce(new Error("first start failed"))
      .mockRejectedValueOnce(new Error("second start failed"))
      .mockReturnValueOnce(olderStart.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(12_000);
    expect(mocks.start).toHaveBeenCalledTimes(3);
    expect(hook.result.current.stage).toBe("loading");

    emit({ status: "ready", device: "cuda", gpu_name: "recovered GPU" });
    await act(async () => { olderStart.reject(new Error("obsolete start failed")); });

    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "recovered GPU", error: null,
    });
    await advance(18_000);
    expect(mocks.check).toHaveBeenCalledTimes(3);
  });

  it("still reports three current start failures, permits retry, and recovers through polling after a crash", async () => {
    mocks.check.mockResolvedValue(status({ running: false }));
    mocks.start.mockRejectedValue(new Error("current start failed"));
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(6_000);
    expect(mocks.start).toHaveBeenCalledTimes(2);
    expect(hook.result.current).toMatchObject({ stage: "loading", error: null });
    await advance(6_000);
    expect(mocks.start).toHaveBeenCalledTimes(3);
    expect(hook.result.current).toMatchObject({ stage: "error", error: "current start failed" });
    await advance(18_000);
    expect(mocks.check).toHaveBeenCalledTimes(3);

    mocks.check.mockResolvedValueOnce(status({ ready: true }));
    act(() => hook.result.current.retry());
    await flushMicrotasks();
    expect(hook.result.current).toMatchObject({ stage: "ready", error: null });
    mocks.start.mockResolvedValue(undefined);
    emit({ status: "crashed", message: "engine disconnected" });
    await flushMicrotasks();
    expect(mocks.start).toHaveBeenCalledTimes(4);
    expect(hook.result.current).toMatchObject({ stage: "loading", error: null });

    mocks.check.mockResolvedValueOnce(status({ ready: true, device: "cuda", gpu_name: "recovered GPU" }));
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(6);
    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "recovered GPU", error: null,
    });
  });

  it("counts only current loading queries toward the ten-check restart threshold and restarts once", async () => {
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(42_000);
    expect(mocks.check).toHaveBeenCalledTimes(8);
    expect(mocks.restart).not.toHaveBeenCalled();

    const olderLoading = deferred<FunASRStatus>();
    mocks.check.mockReturnValueOnce(olderLoading.promise);
    await advance(12_000);
    expect(mocks.check).toHaveBeenCalledTimes(10);
    expect(mocks.restart).not.toHaveBeenCalled();
    await act(async () => { olderLoading.resolve(status()); });
    expect(mocks.restart).not.toHaveBeenCalled();

    await advance(6_000);
    expect(mocks.restart).toHaveBeenCalledOnce();
    expect(hook.result.current.stage).toBe("loading");
    await advance(12_000);
    expect(mocks.restart).toHaveBeenCalledOnce();
  });

  it("retires older queries as soon as a newer query starts, while its result is still pending", async () => {
    const olderStopped = deferred<FunASRStatus>();
    const olderFailure = deferred<FunASRStatus>();
    const currentLoading = deferred<FunASRStatus>();
    mocks.check
      .mockReturnValueOnce(olderStopped.promise)
      .mockReturnValueOnce(olderFailure.promise)
      .mockReturnValueOnce(currentLoading.promise)
      .mockResolvedValue(status({ ready: true, device: "cuda", gpu_name: "current GPU" }));
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(12_000);
    expect(mocks.check).toHaveBeenCalledTimes(3);
    const pendingSnapshot = { stage: "checking", device: null, gpuName: null, error: null };
    expect(hook.result.current).toMatchObject(pendingSnapshot);

    await act(async () => { olderStopped.resolve(status({ running: false })); });
    expect(hook.result.current).toMatchObject(pendingSnapshot);
    expect(mocks.start).not.toHaveBeenCalled();
    await act(async () => { olderFailure.reject(new Error("obsolete pending-window failure")); });
    expect(hook.result.current).toMatchObject(pendingSnapshot);
    expect(vi.getTimerCount()).toBe(1);
    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();

    await act(async () => {
      currentLoading.resolve(status({ device: "cuda", gpu_name: "current GPU" }));
    });
    expect(hook.result.current).toMatchObject({
      stage: "loading", device: "cuda", gpuName: "current GPU", error: null,
    });
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "current GPU", error: null,
    });
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
  });

  it("counts current start failures across backend loading events and intervening loading polls", async () => {
    const attempts = [deferred<void>(), deferred<void>(), deferred<void>()];
    const finalLoadingPoll = deferred<FunASRStatus>();
    mocks.check.mockResolvedValueOnce(status({ running: false }));
    for (const attempt of attempts) {
      mocks.start.mockImplementationOnce(() => {
        handler?.({ payload: { status: "loading", message: "finding engine runtime" } });
        return attempt.promise;
      });
    }
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();

    for (let index = 0; index < attempts.length; index += 1) {
      expect(mocks.start).toHaveBeenCalledTimes(index + 1);
      expect(hook.result.current).toMatchObject({ stage: "loading", error: null });
      if (index === attempts.length - 1) mocks.check.mockReturnValueOnce(finalLoadingPoll.promise);
      await advance(6_000);
      expect(mocks.check).toHaveBeenCalledTimes(index * 2 + 2);
      expect(mocks.start).toHaveBeenCalledTimes(index + 1);
      await act(async () => { attempts[index].reject(new Error(`start attempt ${index + 1} failed`)); });
      if (index < attempts.length - 1) {
        expect(hook.result.current).toMatchObject({ stage: "loading", error: null });
        mocks.check.mockResolvedValueOnce(status({ running: false }));
        await advance(6_000);
      }
    }

    expect(hook.result.current).toMatchObject({ stage: "error", error: "start attempt 3 failed" });
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => {
      finalLoadingPoll.resolve(status({ device: "obsolete device", gpu_name: "obsolete GPU" }));
    });
    expect(hook.result.current).toMatchObject({
      stage: "error", error: "start attempt 3 failed", device: "cpu", gpuName: "poll GPU",
    });
    expect(vi.getTimerCount()).toBe(0);
    expect(mocks.start).toHaveBeenCalledTimes(3);
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();
    await advance(18_000);
    expect(mocks.check).toHaveBeenCalledTimes(6);
  });

  it("keeps an accepted ready query after the pending third start rejects", async () => {
    const olderStart = deferred<void>();
    mocks.check.mockResolvedValue(status({ running: false }));
    mocks.start
      .mockRejectedValueOnce(new Error("first start failed"))
      .mockRejectedValueOnce(new Error("second start failed"))
      .mockReturnValueOnce(olderStart.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await advance(12_000);
    expect(mocks.start).toHaveBeenCalledTimes(3);
    expect(hook.result.current).toMatchObject({ stage: "loading", error: null });

    mocks.check.mockResolvedValueOnce(status({ ready: true, device: "cuda", gpu_name: "ready GPU" }));
    await advance(6_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
    expect(hook.result.current).toMatchObject({ stage: "ready", isReady: true, error: null });
    await act(async () => { olderStart.reject(new Error("obsolete third start failed")); });

    expect(hook.result.current).toMatchObject({
      stage: "ready", isReady: true, device: "cuda", gpuName: "ready GPU", error: null,
    });
    await advance(18_000);
    expect(mocks.check).toHaveBeenCalledTimes(4);
    expect(mocks.start).toHaveBeenCalledTimes(3);
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();
  });
});
