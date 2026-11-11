import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function flushMicrotasks() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("useModelStatus funasr-status listener ownership", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.check.mockResolvedValue({
      running: true,
      ready: true,
      device: "cpu",
      gpu_name: null,
    });
    mocks.start.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
    mocks.download.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
  });

  it("unlistens exactly once when funasr registration resolves after unmount", async () => {
    const registration = deferred<() => void>();
    const lateUnlisten = vi.fn();

    mocks.listen.mockImplementation((event: string) => {
      if (event === "funasr-status") return registration.promise;
      return Promise.resolve(vi.fn());
    });

    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    hook.unmount();

    await act(async () => {
      registration.resolve(lateUnlisten);
      await Promise.resolve();
    });

    expect(lateUnlisten).toHaveBeenCalledOnce();
  });

  it("ignores a late funasr-status event after the hook is disposed", async () => {
    const registration = deferred<() => void>();
    let handler:
      | ((event: { payload: { status: string; message?: string } }) => void)
      | undefined;

    mocks.listen.mockImplementation((event: string, callback: typeof handler) => {
      if (event === "funasr-status") {
        handler = callback;
        return registration.promise;
      }
      return Promise.resolve(vi.fn());
    });

    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    const stageBeforeUnmount = hook.result.current.stage;
    const checksBeforeEvent = mocks.check.mock.calls.length;
    hook.unmount();

    await act(async () => {
      registration.resolve(vi.fn());
      await Promise.resolve();
    });
    expect(handler).toBeDefined();

    await act(async () => {
      handler?.({ payload: { status: "crashed", message: "late event" } });
    });

    expect(mocks.check).toHaveBeenCalledTimes(checksBeforeEvent);
    expect(mocks.start).not.toHaveBeenCalled();
    expect(hook.result.current.stage).toBe(stageBeforeUnmount);
  });

  it("restarts a disconnected engine after it was already ready", async () => {
    let handler:
      | ((event: { payload: { status: string; message?: string } }) => void)
      | undefined;
    mocks.listen.mockImplementation((event: string, callback: typeof handler) => {
      if (event === "funasr-status") handler = callback;
      return Promise.resolve(vi.fn());
    });
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    expect(hook.result.current.stage).toBe("ready");
    expect(mocks.start).not.toHaveBeenCalled();

    mocks.check.mockResolvedValue({ running: false, ready: false, models_present: true });
    await act(async () => {
      handler?.({ payload: { status: "crashed", message: "engine pipe disconnected" } });
    });
    await flushMicrotasks();
    expect(mocks.start).toHaveBeenCalledOnce();
    expect(hook.result.current.stage).toBe("loading");

    await act(async () => {
      handler?.({ payload: { status: "ready" } });
    });
    expect(hook.result.current.stage).toBe("ready");
    hook.unmount();
  });
});

describe("useModelStatus model integrity repair", () => {
  type StatusEvent = {
    payload: {
      status: string;
      message?: string;
      progress?: number;
      model_state?: "ready" | "missing" | "corrupt";
      models_present?: boolean;
      corrupt_models?: string[];
      requestId?: string;
    };
  };
  const handlers = new Map<string, (event: StatusEvent) => void>();
  const corruptStatus = {
    running: false,
    ready: false,
    models_present: true,
    model_state: "corrupt",
    corrupt_models: ["model.bin"],
    message: "Model integrity check failed",
  };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.resetAllMocks();
    handlers.clear();
    mocks.listen.mockImplementation((event: string, handler: (event: StatusEvent) => void) => {
      handlers.set(event, handler);
      return Promise.resolve(() => handlers.delete(event));
    });
    mocks.check.mockResolvedValue(corruptStatus);
    mocks.start.mockResolvedValue(undefined);
    mocks.restart.mockResolvedValue(undefined);
    mocks.download.mockResolvedValue(undefined);
    mocks.cancel.mockResolvedValue(undefined);
  });

  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
  });

  function emit(event: string, payload: StatusEvent["payload"]) {
    act(() => handlers.get(event)?.({ payload }));
  }

  it.each([true, false])(
    "offers repair for structured corruption when models_present is %s",
    async (modelsPresent) => {
      mocks.check.mockResolvedValue({ ...corruptStatus, models_present: modelsPresent });
      const hook = renderHook(() => useModelStatus());
      await flushMicrotasks();

      expect(hook.result.current.stage).toBe("need_repair");
      expect(hook.result.current.isReady).toBe(false);
      expect(hook.result.current.isDownloading).toBe(false);
      expect(mocks.start).not.toHaveBeenCalled();
      expect(mocks.download).not.toHaveBeenCalled();
      hook.unmount();
    },
  );

  it("does not repeatedly start the engine or download corrupt models without a user action", async () => {
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    await act(async () => { await vi.advanceTimersByTimeAsync(18_000); });

    expect(mocks.start).not.toHaveBeenCalled();
    expect(mocks.restart).not.toHaveBeenCalled();
    expect(mocks.download).not.toHaveBeenCalled();
    expect(hook.result.current.stage).toBe("need_repair");
    hook.unmount();
  });

  it.each(["error", "crashed"])(
    "uses structured corruption from a %s event without depending on message language",
    async (status) => {
      mocks.check.mockResolvedValueOnce({ running: true, ready: true, model_state: "ready" });
      const hook = renderHook(() => useModelStatus());
      await flushMicrotasks();
      expect(hook.result.current.stage).toBe("ready");

      emit("funasr-status", {
        status,
        model_state: "corrupt",
        models_present: true,
        corrupt_models: ["model.bin"],
        message: "Checksum mismatch",
      });
      await flushMicrotasks();

      expect(hook.result.current.stage).toBe("need_repair");
      await act(async () => { await vi.advanceTimersByTimeAsync(18_000); });
      expect(mocks.start).not.toHaveBeenCalled();
      expect(mocks.restart).not.toHaveBeenCalled();
      expect(mocks.download).not.toHaveBeenCalled();
      hook.unmount();
    },
  );

  it("downloads only after the repair action and becomes ready after repair completes", async () => {
    const pendingRepair = deferred<void>();
    mocks.download.mockReturnValue(pendingRepair.promise);
    const hook = renderHook(() => useModelStatus());
    await flushMicrotasks();
    expect(hook.result.current.stage).toBe("need_repair");
    expect(mocks.download).not.toHaveBeenCalled();

    await act(async () => { hook.result.current.downloadModels(); });
    expect(mocks.download).toHaveBeenCalledOnce();
    expect(hook.result.current.stage).toBe("downloading");
    expect(hook.result.current.isDownloading).toBe(true);

    emit("model-download-status", { status: "completed", progress: 100, requestId: mocks.download.mock.calls[0]?.[0] });
    expect(hook.result.current.stage).toBe("loading");
    mocks.check.mockResolvedValue({ running: true, ready: true, model_state: "ready" });
    emit("funasr-status", { status: "ready", model_state: "ready" });
    await act(async () => { pendingRepair.resolve(undefined); });

    expect(hook.result.current.stage).toBe("ready");
    expect(hook.result.current.isReady).toBe(true);
    expect(hook.result.current.isDownloading).toBe(false);
    expect(mocks.download).toHaveBeenCalledOnce();
    hook.unmount();
  });

  it.each([
    ["legacy", "snapshot"],
    ["legacy", "event"],
    ["structured", "snapshot"],
    ["structured", "event"],
  ])(
    "waits for the download action after a %s missing-model %s",
    async (format, source) => {
      const pendingDownload = deferred<void>();
      mocks.download.mockReturnValue(pendingDownload.promise);
      const modelState = format === "structured" ? { model_state: "missing" as const } : {};
      mocks.check.mockResolvedValue({ running: false, ready: false, models_present: false, ...modelState });
      if (source === "event") mocks.check.mockResolvedValueOnce({ running: true, ready: true });
      const hook = renderHook(() => useModelStatus());
      await flushMicrotasks();
      if (source === "event") {
        expect(hook.result.current.stage).toBe("ready");
        emit("funasr-status", {
          status: "error",
          message: format === "legacy" ? "模型文件未下载" : "Missing weights",
          ...modelState,
        });
      }

      await act(async () => { await vi.advanceTimersByTimeAsync(18_000); });
      expect(mocks.download).not.toHaveBeenCalled();
      expect(hook.result.current).toMatchObject({ stage: "need_download", isDownloading: false });
      expect(mocks.start).not.toHaveBeenCalled();

      act(() => { hook.result.current.downloadModels(); });
      expect(mocks.download).toHaveBeenCalledOnce();
      expect(hook.result.current).toMatchObject({ stage: "downloading", isDownloading: true });
      hook.unmount();
      await act(async () => { pendingDownload.resolve(undefined); });
    },
  );
});
