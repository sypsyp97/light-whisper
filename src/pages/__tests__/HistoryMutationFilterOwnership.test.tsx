import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PersistentHistoryRecord } from "@/types";

const api = vi.hoisted(() => ({
  copyToClipboard: vi.fn(), deleteTranscriptionHistory: vi.fn(),
  exportTranscriptionHistory: vi.fn(), getTranscriptionHistoryStats: vi.fn(),
  hideMainWindow: vi.fn(), listTranscriptionHistory: vi.fn(),
  reprocessTranscriptionHistory: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => vi.fn()) }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ minimize: vi.fn() }) }));
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("@/components/TitleBar", () => ({ default: () => <header /> }));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ i18n: { language: "en" }, t }) };
});

import HistoryPage from "@/pages/HistoryPage";

const record: PersistentHistoryRecord = {
  id: 7, sessionId: 9, createdAt: Date.UTC(2026, 6, 13), updatedAt: Date.UTC(2026, 6, 13),
  mode: "dictation", workflow: "dictation", status: "success", text: "Dictation A",
  originalText: "Raw A", durationSec: 2.4, language: "en", engine: "sensevoice",
  audioAvailable: true,
};
const assistantRecord: PersistentHistoryRecord = {
  ...record, id: 8, mode: "assistant", workflow: "assistant", text: "Assistant B",
};
const currentFilter = { query: "", mode: "assistant", status: "", limit: 50, offset: 0 };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  vi.spyOn(window, "confirm").mockReturnValue(true);
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
  });
  api.getTranscriptionHistoryStats.mockResolvedValue({
    total: 2, totalCharacters: 22,
    asr: { p50Ms: 100, p95Ms: 200 }, polish: { p50Ms: 100, p95Ms: 200 },
    totalLatency: { p50Ms: 200, p95Ms: 400 },
  });
  api.listTranscriptionHistory.mockImplementation(({ mode }: { mode: string }) => Promise.resolve({
    items: [mode === "assistant" ? assistantRecord : record], total: 1, hasMore: false,
  }));
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.clearAllTimers(); vi.useRealTimers(); });

describe("history mutation filter ownership", () => {
  it.each(["polish", "asr", "delete-removed", "delete-missing"] as const)(
    "refreshes the visible filter when a pending %s mutation completes",
    async (kind) => {
      const reprocess = deferred<PersistentHistoryRecord>();
      const remove = deferred<boolean>();
      api.reprocessTranscriptionHistory.mockReturnValueOnce(reprocess.promise);
      api.deleteTranscriptionHistory.mockReturnValueOnce(remove.promise);
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      fireEvent.click(screen.getByRole("button", { name: "historyPage.dictation" }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({ ...currentFilter, mode: "dictation" });

      const deleting = kind.startsWith("delete-");
      fireEvent.click(screen.getByRole("button", {
        name: deleting ? "historyPage.delete" : kind === "asr" ? "historyPage.reprocessAsr" : "historyPage.reprocessPolish",
      }));
      if (deleting) expect(api.deleteTranscriptionHistory).toHaveBeenCalledWith(record.id);
      else expect(api.reprocessTranscriptionHistory).toHaveBeenCalledWith(record.id, kind);

      fireEvent.click(screen.getByRole("button", { name: "historyPage.assistant" }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByText("Assistant B")).toBeVisible();
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(currentFilter);
      const callsBeforeCompletion = api.listTranscriptionHistory.mock.calls.length;

      await act(async () => {
        if (deleting) remove.resolve(kind === "delete-removed");
        else reprocess.resolve(record);
      });

      expect.soft(api.listTranscriptionHistory).toHaveBeenCalledTimes(callsBeforeCompletion + 1);
      expect.soft(api.listTranscriptionHistory).toHaveBeenLastCalledWith(currentFilter);
      expect.soft(screen.queryByText("Assistant B")).toBeVisible();
      expect.soft(screen.queryByText("Dictation A")).not.toBeInTheDocument();
    },
  );
});
