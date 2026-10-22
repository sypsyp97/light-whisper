import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FunASRStatus } from "@/types";
import { ownershipTrace } from "@/test/asyncOwnershipTrace";

type StatusPayload = { status: string; device?: string; gpu_name?: string | null; message?: string };
const mocks = vi.hoisted(() => ({
  check: vi.fn(), start: vi.fn(), restart: vi.fn(), download: vi.fn(), cancel: vi.fn(),
  status: undefined as undefined | ((event: { payload: StatusPayload }) => void),
}));
vi.mock("@/api/tauri", () => ({
  checkFunASRStatus: mocks.check, startFunASR: mocks.start, restartFunASR: mocks.restart,
  downloadModels: mocks.download, cancelModelDownload: mocks.cancel,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: (event: { payload: StatusPayload }) => void) => {
    if (name === "funasr-status") mocks.status = callback;
    else if (name !== "model-download-status") throw new Error(`Unexpected listener: ${name}`);
    return vi.fn();
  }),
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));
import { useModelStatus } from "../useModelStatus";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function status(id: number, kind: string): FunASRStatus {
  return {
    running: kind !== "stopped" && kind !== "missing", ready: kind === "ready",
    model_loaded: kind === "ready", models_present: kind !== "missing",
    model_state: kind === "missing" ? "missing" : "ready", message: "",
    device: kind === "ready" ? "cuda" : "cpu", gpu_name: kind === "ready" ? `query GPU ${id}` : undefined,
  };
}
async function flush() { await act(async () => { await Promise.resolve(); await Promise.resolve(); }); }

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.status = undefined;
  mocks.restart.mockResolvedValue(undefined);
  mocks.cancel.mockResolvedValue(undefined);
  // Keep native download work pending; status ownership never cancels issued work.
  mocks.download.mockImplementation(() => new Promise(() => {}));
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

describe("TLC engine status ownership replay", () => {
  it.each(["engine-status-event", "engine-status-query", "engine-status-start", "engine-status-progress"])(
    "replays %s through the real hook", async (name) => {
      const events = ownershipTrace(name);
      const requests: ReturnType<typeof deferred<FunASRStatus>>[] = [];
      const starts = new Map<number, ReturnType<typeof deferred<void>>>();
      const epochs = new Map<number, number>();
      const startEpochs = new Map<number, number>();
      const settled = new Set<number>();
      const settledStarts = new Set<number>();
      let epoch = 0;
      let startEpoch = 0;
      let completing = 0;
      let currentFailures = 0;
      let hook: ReturnType<typeof renderHook<ReturnType<typeof useModelStatus>, unknown>> | undefined;
      mocks.check.mockImplementation(() => {
        const request = deferred<FunASRStatus>();
        requests.push(request);
        epochs.set(requests.length, ++epoch);
        return request.promise;
      });
      mocks.start.mockImplementation(() => {
        if (!completing || starts.has(completing)) throw new Error("Start without a unique accepted status response");
        const start = deferred<void>();
        startEpochs.set(completing, ++startEpoch);
        starts.set(completing, start);
        return start.promise;
      });
      const snapshot = () => {
        if (!hook) throw new Error("Snapshot before mount");
        const value = hook.result.current;
        return {
          stage: value.stage, ready: value.isReady, device: value.device, gpu: value.gpuName,
          error: value.error, downloading: value.isDownloading, timers: vi.getTimerCount(),
          starts: mocks.start.mock.calls.length, restarts: mocks.restart.mock.calls.length,
          downloads: mocks.download.mock.calls.length,
        };
      };
      const query = async (id: number) => {
        expect(id).toBe(requests.length + 1);
        if (!hook) hook = renderHook(() => useModelStatus());
        else {
          expect(vi.getTimerCount()).toBe(1);
          await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
        }
        await flush();
        expect(mocks.check).toHaveBeenCalledTimes(id);
      };
      const emit = async (kind: string, crashId?: number, nativeProgress = false) => {
        if (!mocks.status) throw new Error("Status event without a registered listener");
        epoch += 1;
        if (kind !== "loading") startEpoch += 1;
        const payload: StatusPayload = kind === "crashed" ? { status: "crashed" }
          : nativeProgress ? { status: "loading" }
          : kind === "failure" ? { status: "error", message: "event failed" }
          : { status: kind, device: kind === "ready" ? "cuda" : "cpu",
            gpu_name: kind === "ready" ? "event GPU" : null };
        await act(async () => { mocks.status!({ payload }); });
        await flush();
        expect(hook!.result.current.stage).toBe(kind === "failure" ? "error" : kind === "crashed" ? "loading" : kind);
        if (kind === "ready") {
          expect(hook!.result.current).toMatchObject({ device: "cuda", gpuName: "event GPU", error: null });
          expect(vi.getTimerCount()).toBe(0);
        } else if (kind === "crashed") {
          currentFailures = 0;
          expect(mocks.check).toHaveBeenCalledTimes(crashId!);
          expect(vi.getTimerCount()).toBe(1);
        }
      };
      const complete = async (id: number, kind: string) => {
        const request = requests[id - 1];
        if (!request || settled.has(id)) throw new Error(`Completion without pending status ${id}`);
        settled.add(id);
        const before = snapshot();
        const current = epochs.get(id) === epoch;
        if (current && ["ready", "failure", "missing"].includes(kind)) startEpoch += 1;
        if (current && kind === "failure") epoch += 1;
        completing = id;
        await act(async () => {
          if (kind === "failure") request.reject(new Error(`query ${id} failed`));
          else request.resolve(status(id, kind));
          await request.promise.catch(() => undefined);
        });
        completing = 0;
        await flush();
        if (!current) expect(snapshot()).toEqual(before);
        else if (kind === "failure") {
          expect(hook!.result.current).toMatchObject({ stage: "error", error: `query ${id} failed` });
          expect(vi.getTimerCount()).toBe(0);
        } else if (kind === "ready") {
          currentFailures = 0;
          expect(hook!.result.current).toMatchObject({ stage: "ready", isReady: true,
            device: "cuda", gpuName: `query GPU ${id}`, error: null });
          expect(vi.getTimerCount()).toBe(0);
        } else if (kind === "loading" || kind === "stopped") {
          expect(hook!.result.current).toMatchObject({ stage: "loading", device: "cpu", gpuName: null });
          expect(mocks.start).toHaveBeenCalledTimes(before.starts + (kind === "stopped" ? 1 : 0));
          expect(vi.getTimerCount()).toBe(before.timers);
        } else if (kind === "missing") {
          await act(async () => { await vi.advanceTimersByTimeAsync(0); });
          expect(hook!.result.current).toMatchObject({ stage: "downloading", isDownloading: true, error: null });
          expect(mocks.download).toHaveBeenCalledTimes(before.downloads + 1);
        } else throw new Error(`Unhandled status kind: ${kind}`);
        expect(mocks.restart).not.toHaveBeenCalled();
      };
      const finishStart = async (id: number, success: boolean) => {
        const start = starts.get(id);
        if (!start || settledStarts.has(id)) throw new Error(`Completion without pending start ${id}`);
        settledStarts.add(id);
        const before = snapshot();
        const current = startEpochs.get(id) === startEpoch;
        await act(async () => {
          if (success) start.resolve(undefined);
          else start.reject(new Error("start failed"));
          await start.promise.catch(() => undefined);
        });
        await flush();
        if (!current) expect(snapshot()).toEqual(before);
        else {
          currentFailures = success ? 0 : currentFailures + 1;
          if (currentFailures < 3) expect(snapshot()).toEqual(before);
          else {
            epoch += 1;
            startEpoch += 1;
            expect(hook!.result.current).toMatchObject({ stage: "error", error: "start failed" });
            expect(vi.getTimerCount()).toBe(0);
          }
        }
      };
      const handled: string[] = [];
      for (const event of events) {
        let match: RegExpExecArray | null;
        if ((match = /^engine-status:query:([1-6])$/.exec(event))) await query(Number(match[1]));
        else if ((match = /^engine-status:event:(ready|loading|failure)$/.exec(event))) await emit(match[1]);
        else if ((match = /^engine-status:crashed:([1-6])$/.exec(event))) await emit("crashed", Number(match[1]));
        else if ((match = /^engine-status:progress:([1-6])$/.exec(event))) {
          if (!starts.has(Number(match[1])) || settledStarts.has(Number(match[1]))) {
            throw new Error(`Native progress without a pending issued start: ${event}`);
          }
          await emit("loading", undefined, true);
        } else if ((match = /^engine-status:complete:([1-6]):(ready|loading|stopped|missing|failure)$/.exec(event))) {
          await complete(Number(match[1]), match[2]);
        } else if ((match = /^engine-status:start:([1-6]):(success|failure)$/.exec(event))) {
          await finishStart(Number(match[1]), match[2] === "success");
        } else throw new Error(`Unhandled TLC engine status event: ${event}`);
        handled.push(event);
      }
      expect(handled).toEqual(events);
      if (name === "engine-status-progress") {
        expect(currentFailures).toBe(3);
        expect(mocks.start).toHaveBeenCalledTimes(3);
        expect(mocks.check).toHaveBeenCalledTimes(6);
        expect(hook!.result.current).toMatchObject({ stage: "error", error: "start failed" });
        expect(vi.getTimerCount()).toBe(0);
        // The third terminal start failure supersedes the already pending poll.
        await complete(6, "loading");
      }
      // Demonstrate current recovery through the actual crash listener, API and interval.
      await emit("crashed", requests.length + 1);
      const recovery = requests.length;
      await complete(recovery, "ready");
      const recovered = snapshot();
      await act(async () => { await vi.advanceTimersByTimeAsync(12000); });
      expect(snapshot()).toEqual(recovered);
      expect(mocks.check).toHaveBeenCalledTimes(recovery);
      hook!.unmount();
    },
  );
});
