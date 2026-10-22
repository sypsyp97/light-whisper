import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PersistentHistoryStats } from "@/types";
import { ownershipTrace } from "@/test/asyncOwnershipTrace";

const mocks = vi.hoisted(() => ({
  api: {
    copyToClipboard: vi.fn(), deleteTranscriptionHistory: vi.fn(),
    exportTranscriptionHistory: vi.fn(), getTranscriptionHistoryStats: vi.fn(),
    hideMainWindow: vi.fn(), listTranscriptionHistory: vi.fn(),
    reprocessTranscriptionHistory: vi.fn(),
  },
  listener: undefined as undefined | (() => void),
  unlisten: vi.fn(),
}));
vi.mock("@/api/tauri", () => mocks.api);
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: () => void) => {
    if (name !== "history-updated") throw new Error(`Unexpected listener: ${name}`);
    mocks.listener = callback;
    return mocks.unlisten;
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ minimize: vi.fn() }) }));
vi.mock("@/components/TitleBar", () => ({ default: () => <header /> }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({
  i18n: { language: "en" },
  t: (key: string, options?: Record<string, unknown>) => options?.count === undefined
    ? key : `${key}:${options.count}`,
}) }));
import HistoryPage from "@/pages/HistoryPage";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

// Every rendered statistic identifies the request that supplied the snapshot.
function stats(id: number): PersistentHistoryStats {
  return {
    total: id * 101, success: id * 100, failed: id,
    totalCharacters: id * 1001,
    asr: { p50Ms: id * 11, p95Ms: id * 17 },
    polish: { p50Ms: id * 23, p95Ms: id * 31 },
    totalLatency: { p50Ms: id * 37, p95Ms: id * 41 },
  };
}

function expectSnapshot(id: number) {
  if (id === 0) {
    expect(document.querySelector(".history-metrics")).not.toBeInTheDocument();
    return;
  }
  const value = stats(id);
  const metrics = within(screen.getByRole("region", { name: `historyPage.total:${value.total}` }));
  expect(metrics.getByText(value.total.toLocaleString())).toBeVisible();
  expect(metrics.getByText(`historyPage.characters:${value.totalCharacters.toLocaleString()}`)).toBeVisible();
  for (const latency of [value.asr, value.polish, value.totalLatency]) {
    expect(metrics.getByText(`${latency.p50Ms}ms`)).toBeVisible();
    expect(metrics.getByText(`P95 ${latency.p95Ms}ms`)).toBeVisible();
  }
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  mocks.listener = undefined;
  mocks.api.listTranscriptionHistory.mockResolvedValue({ items: [], total: 0, hasMore: false });
});
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

describe("TLC history statistics ownership replay", () => {
  it.each([
    "history-stats-publication", "history-stats-error", "history-stats-cleanup",
  ])("replays %s through the actual page", async (name) => {
    const events = ownershipTrace(name);
    const requests: ReturnType<typeof deferred<PersistentHistoryStats>>[] = [];
    const settled = new Set<number>();
    const expectedErrors: Error[] = [];
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    mocks.api.getTranscriptionHistoryStats.mockReset().mockImplementation(() => {
      const request = deferred<PersistentHistoryStats>();
      requests.push(request);
      return request.promise;
    });
    let view: ReturnType<typeof render> | undefined;
    let mounted = false;
    let latest = 0;
    let accepted = 0;

    const check = () => {
      expect(errorLog.mock.calls).toEqual(expectedErrors.map((error) => ["Failed to load history stats:", error]));
      if (mounted) expectSnapshot(accepted);
      else expect(document.querySelector(".history-metrics")).not.toBeInTheDocument();
    };
    const begin = async (id: number) => {
      expect(id).toBe(requests.length + 1);
      latest = id;
      if (!view) {
        mounted = true;
        view = render(<HistoryPage onNavigate={vi.fn()} />);
      } else {
        if (!mounted || !mocks.listener) throw new Error("Refresh without a mounted history listener");
        await act(async () => { mocks.listener!(); });
      }
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(mocks.api.getTranscriptionHistoryStats).toHaveBeenCalledTimes(id);
      check();
    };
    const complete = async (id: number, success: boolean) => {
      const request = requests[id - 1];
      if (!request || settled.has(id)) throw new Error(`Completion without pending request ${id}`);
      settled.add(id);
      const error = new Error(`statistics request ${id} failed`);
      if (mounted && latest === id) {
        if (success) accepted = id;
        else expectedErrors.push(error);
      }
      await act(async () => {
        if (success) request.resolve(stats(id));
        else request.reject(error);
        await request.promise.catch(() => undefined);
      });
      check();
    };
    const dispose = () => {
      if (!view || !mounted) throw new Error("Disposal without a mounted page");
      view.unmount();
      mounted = false;
      expect(mocks.unlisten).toHaveBeenCalledTimes(1);
      check();
    };

    const handled: string[] = [];
    for (const event of events) {
      const requestEvent = /^history-stats:(begin|success|failure):([1-3])$/.exec(event);
      if (requestEvent) {
        const id = Number(requestEvent[2]);
        if (requestEvent[1] === "begin") await begin(id);
        else await complete(id, requestEvent[1] === "success");
      } else if (event === "history-stats:dispose") {
        dispose();
      } else {
        throw new Error(`Unhandled TLC history statistics event: ${event}`);
      }
      handled.push(event);
    }
    expect(handled).toEqual(events);

    if (mounted) {
      // Complete the newest request if the negative stopped at an older error.
      if (!settled.has(latest)) await complete(latest, false);
      const recovery = requests.length + 1;
      await begin(recovery);
      await complete(recovery, true);
      expectSnapshot(recovery);
      // Native failure after disposal is observable even though React 19 drops
      // unmounted state updates without warning.
      const pendingAtDisposal = requests.length + 1;
      await begin(pendingAtDisposal);
      dispose();
      await complete(pendingAtDisposal, false);
    }
    for (let id = 1; id <= requests.length; id += 1) {
      if (!settled.has(id)) await complete(id, false);
    }
    expect(settled.size).toBe(requests.length);
  });
});
