import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PersistentHistoryPage, PersistentHistoryRecord } from "@/types";
import { ownershipTrace } from "@/test/asyncOwnershipTrace";

const api = vi.hoisted(() => ({
  copyToClipboard: vi.fn(), deleteTranscriptionHistory: vi.fn(),
  exportTranscriptionHistory: vi.fn(), getTranscriptionHistoryStats: vi.fn(),
  hideMainWindow: vi.fn(), listTranscriptionHistory: vi.fn(),
  reprocessTranscriptionHistory: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ minimize: vi.fn() }) }));
vi.mock("@/components/TitleBar", () => ({ default: () => <header /> }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({
  i18n: { language: "en" },
  t: (key: string, options?: Record<string, unknown>) => options?.error ? `${key}:${options.error}` : key,
}) }));
import HistoryPage from "@/pages/HistoryPage";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function record(id: number, text: string): PersistentHistoryRecord {
  return {
    id, sessionId: id, createdAt: Date.UTC(2026, 9, 21), updatedAt: Date.UTC(2026, 9, 21),
    mode: "dictation", workflow: "dictation", status: "success", text, originalText: text,
    durationSec: 1, language: "en", engine: "sensevoice", appProcess: "Code.exe",
    appWindowTitle: "editor", audioAvailable: false,
  };
}
const page = (items: PersistentHistoryRecord[], hasMore = true): PersistentHistoryPage => ({
  items, total: 3, hasMore,
});

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  api.getTranscriptionHistoryStats.mockResolvedValue({ total: 0 });
});
afterEach(() => { vi.useRealTimers(); });

describe("TLC history filter ownership replay", () => {
  it.each([
    "history-filter-late-retirement", "history-filter-pagination", "history-filter-failed-pagination",
  ])("replays %s through the actual page", async (name) => {
    const events = ownershipTrace(name);
    const old = deferred<PersistentHistoryPage>();
    const reset = deferred<PersistentHistoryPage>();
    const retry = deferred<PersistentHistoryPage>();
    const seed = record(1, "seed old filter row");
    const obsolete = record(2, "obsolete old page row");
    const replacement = record(3, "replacement current filter row");
    const replacementSecond = record(4, "second current filter row");
    const appended = record(5, "next current filter page row");
    api.listTranscriptionHistory.mockReset().mockResolvedValueOnce(page([seed]));
    api.listTranscriptionHistory.mockImplementation(({ query }: { query: string }) => query ? reset.promise : old.promise);
    const view = render(<HistoryPage onNavigate={vi.fn()} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText(seed.text)).toBeVisible();
    let dispatched = false;
    let failed = false;
    let oldStarted = false;
    const handled: string[] = [];
    for (const event of events) {
      switch (event) {
        case "history:old:begin":
          fireEvent.click(screen.getByRole("button", { name: /historyPage.loadMore/ }));
          expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(expect.objectContaining({ query: "", offset: 1 }));
          oldStarted = true;
          break;
        case "history:filter:change": {
          const calls = api.listTranscriptionHistory.mock.calls.length;
          fireEvent.change(screen.getByRole("textbox"), { target: { value: "current" } });
          expect(screen.queryByRole("button", { name: /historyPage.loadMore/ })).not.toBeInTheDocument();
          await act(async () => { await vi.advanceTimersByTimeAsync(179); });
          expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(calls);
          break;
        }
        case "history:reset:dispatch":
          await act(async () => { await vi.advanceTimersByTimeAsync(1); });
          dispatched = true;
          expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(expect.objectContaining({ query: "current", offset: 0 }));
          break;
        case "history:old:success":
        case "history:old:failure":
          if (!oldStarted) throw new Error(`Completion without request: ${event}`);
          await act(async () => {
            if (event.endsWith("success")) old.resolve(page([obsolete]));
            else old.reject(new Error("obsolete request failed"));
            await old.promise.catch(() => undefined);
          });
          expect(screen.queryByText(obsolete.text)).not.toBeInTheDocument();
          expect(screen.queryByText(/obsolete request failed/)).not.toBeInTheDocument();
          expect(screen.queryByRole("button", { name: /historyPage.loadMore/ })).not.toBeInTheDocument();
          break;
        case "history:reset:failure":
          if (!dispatched) throw new Error(`Completion without request: ${event}`);
          await act(async () => { reset.reject(new Error("current reset failed")); await reset.promise.catch(() => undefined); });
          failed = true;
          expect(screen.getByText(/current reset failed/)).toBeVisible();
          expect(screen.queryByRole("button", { name: /historyPage.loadMore/ })).not.toBeInTheDocument();
          break;
        default:
          throw new Error(`Unhandled TLC history event: ${event}`);
      }
      handled.push(event);
    }
    expect(handled).toEqual(events);
    // Drain the corrected reset after the counterexample schedule, including
    // error -> offset-zero retry -> replacement publication when it failed.
    // Then exercise the model's reachable normal pagination at current offset 2.
    if (!dispatched) {
      await act(async () => { await vi.advanceTimersByTimeAsync(1); });
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(expect.objectContaining({ query: "current", offset: 0 }));
    }
    if (failed) {
      api.listTranscriptionHistory.mockReturnValueOnce(retry.promise);
      fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(expect.objectContaining({ query: "current", offset: 0 }));
      await act(async () => { retry.resolve(page([replacement, replacementSecond])); await retry.promise; });
    } else {
      await act(async () => { reset.resolve(page([replacement, replacementSecond])); await reset.promise; });
    }
    expect(screen.getByText(replacement.text)).toBeVisible();
    expect(screen.getByText(replacementSecond.text)).toBeVisible();
    expect(screen.queryByText(seed.text)).not.toBeInTheDocument();
    expect(screen.queryByText(obsolete.text)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "common.retry" })).not.toBeInTheDocument();
    api.listTranscriptionHistory.mockResolvedValueOnce(page([appended], false));
    fireEvent.click(screen.getByRole("button", { name: /historyPage.loadMore/ }));
    expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(expect.objectContaining({ query: "current", offset: 2 }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText(replacement.text)).toBeVisible();
    expect(screen.getByText(replacementSecond.text)).toBeVisible();
    expect(screen.getByText(appended.text)).toBeVisible();
    expect(screen.queryByRole("button", { name: /historyPage.loadMore/ })).not.toBeInTheDocument();
    view.unmount();
  });
});
