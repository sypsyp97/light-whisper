import { act, cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PersistentHistoryStats } from "@/types";

const api = vi.hoisted(() => ({
  copyToClipboard: vi.fn(),
  deleteTranscriptionHistory: vi.fn(),
  exportTranscriptionHistory: vi.fn(),
  getTranscriptionHistoryStats: vi.fn(),
  hideMainWindow: vi.fn(),
  listTranscriptionHistory: vi.fn(),
  reprocessTranscriptionHistory: vi.fn(),
}));
const event = vi.hoisted(() => ({ listen: vi.fn(), unlisten: vi.fn() }));

vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen: event.listen }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ minimize: vi.fn() }),
}));
vi.mock("@/components/TitleBar", () => ({ default: () => <header /> }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "en" },
    t: (key: string, options?: Record<string, unknown>) => (
      options?.count == null ? key : `${key}:${options.count}`
    ),
  }),
}));

import HistoryPage from "@/pages/HistoryPage";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function snapshot(total: number, totalCharacters: number, latency: number): PersistentHistoryStats {
  return {
    total,
    success: total - 1,
    failed: 1,
    totalCharacters,
    asr: { p50Ms: latency, p95Ms: latency + 1 },
    polish: { p50Ms: latency + 2, p95Ms: latency + 3 },
    totalLatency: { p50Ms: latency + 4, p95Ms: latency + 5 },
  };
}

const accepted = snapshot(11, 110, 100);
const obsolete = snapshot(22, 220, 200);
const current = snapshot(33, 330, 300);

function expectMetrics(stats: PersistentHistoryStats) {
  const metrics = screen.getByRole("region", { name: /^historyPage.total:/ });
  expect(metrics).toHaveAccessibleName(`historyPage.total:${stats.total}`);
  expect(within(metrics).getByText(stats.total.toLocaleString())).toBeVisible();
  expect(within(metrics).getByText(`historyPage.characters:${stats.totalCharacters.toLocaleString()}`)).toBeVisible();
  for (const latency of [stats.asr, stats.polish, stats.totalLatency]) {
    expect(within(metrics).getByText(`${latency.p50Ms}ms`)).toBeVisible();
    expect(within(metrics).getByText(`P95 ${latency.p95Ms}ms`)).toBeVisible();
  }
}

let historyUpdated: (() => void) | undefined;

function updateHistory() {
  if (!historyUpdated) throw new Error("history-updated listener was not registered");
  act(historyUpdated);
}

async function renderPage() {
  const view = render(<HistoryPage onNavigate={vi.fn()} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  expect(event.listen).toHaveBeenCalledWith("history-updated", expect.any(Function));
  return view;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  historyUpdated = undefined;
  event.listen.mockImplementation((name: string, callback: () => void) => {
    if (name === "history-updated") historyUpdated = callback;
    return Promise.resolve(event.unlisten);
  });
  api.listTranscriptionHistory.mockResolvedValue({ items: [], total: 0, hasMore: false });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("HistoryPage stats refresh ownership", () => {
  it("keeps every metric from the newer refresh when the older success arrives last", async () => {
    const older = deferred<PersistentHistoryStats>();
    const newer = deferred<PersistentHistoryStats>();
    api.getTranscriptionHistoryStats
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise);
    await renderPage();

    updateHistory();
    expect(api.getTranscriptionHistoryStats).toHaveBeenCalledTimes(2);
    await act(async () => { newer.resolve(current); await newer.promise; });
    expectMetrics(current);

    await act(async () => { older.resolve(obsolete); await older.promise; });
    expectMetrics(current);
  });

  it("retires an older success as soon as a newer refresh starts, before it settles", async () => {
    const older = deferred<PersistentHistoryStats>();
    const newer = deferred<PersistentHistoryStats>();
    api.getTranscriptionHistoryStats
      .mockResolvedValueOnce(accepted)
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise);
    await renderPage();
    expectMetrics(accepted);

    updateHistory();
    updateHistory();
    expect(api.getTranscriptionHistoryStats).toHaveBeenCalledTimes(3);
    await act(async () => { older.resolve(obsolete); await older.promise; });
    expectMetrics(accepted);

    await act(async () => { newer.resolve(current); await newer.promise; });
    expectMetrics(current);
  });

  it("keeps the last accepted snapshot after the current failure and a later obsolete success", async () => {
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    const older = deferred<PersistentHistoryStats>();
    const newer = deferred<PersistentHistoryStats>();
    const failure = new Error("current statistics request failed");
    api.getTranscriptionHistoryStats
      .mockResolvedValueOnce(accepted)
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(newer.promise)
      .mockResolvedValueOnce(current);
    await renderPage();
    expectMetrics(accepted);

    updateHistory();
    updateHistory();
    await act(async () => { newer.reject(failure); await newer.promise.catch(() => undefined); });
    expect(errorLog).toHaveBeenCalledTimes(1);
    expect(errorLog).toHaveBeenCalledWith(expect.any(String), failure);
    expectMetrics(accepted);

    await act(async () => { older.resolve(obsolete); await older.promise; });
    expectMetrics(accepted);
    expect(errorLog).toHaveBeenCalledTimes(1);

    updateHistory();
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expectMetrics(current);
    expect(errorLog).toHaveBeenCalledTimes(1);
  });

  it("suppresses obsolete failures both while the current refresh is pending and after its success", async () => {
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    const older = deferred<PersistentHistoryStats>();
    const middle = deferred<PersistentHistoryStats>();
    const newer = deferred<PersistentHistoryStats>();
    api.getTranscriptionHistoryStats
      .mockResolvedValueOnce(accepted)
      .mockReturnValueOnce(older.promise)
      .mockReturnValueOnce(middle.promise)
      .mockReturnValueOnce(newer.promise);
    await renderPage();
    expectMetrics(accepted);

    updateHistory();
    updateHistory();
    updateHistory();
    expect(api.getTranscriptionHistoryStats).toHaveBeenCalledTimes(4);
    await act(async () => { older.reject(new Error("superseded while pending")); await older.promise.catch(() => undefined); });
    expect(errorLog).not.toHaveBeenCalled();
    expectMetrics(accepted);

    await act(async () => { newer.resolve(current); await newer.promise; });
    expectMetrics(current);
    await act(async () => { middle.reject(new Error("superseded after success")); await middle.promise.catch(() => undefined); });
    expect(errorLog).not.toHaveBeenCalled();
    expectMetrics(current);
  });

  it.each(["success", "failure"] as const)("retires a pending %s after unmount without affecting the next page", async (outcome) => {
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    const retired = deferred<PersistentHistoryStats>();
    api.getTranscriptionHistoryStats
      .mockReturnValueOnce(retired.promise)
      .mockResolvedValueOnce(current);
    const first = await renderPage();
    first.unmount();
    expect(event.unlisten).toHaveBeenCalledTimes(1);

    const next = await renderPage();
    expectMetrics(current);
    await act(async () => {
      if (outcome === "success") retired.resolve(obsolete);
      else retired.reject(new Error("statistics failed after unmount"));
      await retired.promise.catch(() => undefined);
    });
    expectMetrics(current);
    next.unmount();
    expect(event.unlisten).toHaveBeenCalledTimes(2);
    expect(errorLog).not.toHaveBeenCalled();
  });
});
