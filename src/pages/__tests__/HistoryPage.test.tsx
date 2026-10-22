import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { PersistentHistoryPage } from "@/types";

const api = vi.hoisted(() => ({
  copyToClipboard: vi.fn(),
  deleteTranscriptionHistory: vi.fn(),
  exportTranscriptionHistory: vi.fn(),
  getTranscriptionHistoryStats: vi.fn(),
  hideMainWindow: vi.fn(),
  listTranscriptionHistory: vi.fn(),
  reprocessTranscriptionHistory: vi.fn(),
}));

const event = vi.hoisted(() => ({ listen: vi.fn() }));
const toast = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn() }));

vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => event);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ minimize: vi.fn() }),
}));
vi.mock("sonner", () => ({ toast }));
vi.mock("@/components/TitleBar", () => ({
  default: ({ title, leftAction, rightActions }: {
    title: string;
    leftAction: React.ReactNode;
    rightActions: React.ReactNode;
  }) => <header><h1>{title}</h1>{leftAction}{rightActions}</header>,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    i18n: { language: "zh-CN" },
    t: (key: string, options?: Record<string, unknown>) => ({
      "common.back": "返回",
      "common.close": "关闭",
      "common.copy": "复制",
      "common.loading": "加载中",
      "common.minimize": "最小化",
      "common.retry": "重试",
      "historyPage.allModes": "全部",
      "historyPage.allStatuses": "全部状态",
      "historyPage.appRule": `规则：${options?.name ?? ""}`,
      "historyPage.asrLatency": "ASR",
      "historyPage.assistant": "助手",
      "historyPage.delete": "删除记录",
      "historyPage.deleteConfirm": "确认删除",
      "historyPage.dictation": "听写",
      "historyPage.export": "导出",
      "historyPage.exportFormat": "导出格式",
      "historyPage.exportJson": "JSON",
      "historyPage.exportMarkdown": "Markdown",
      "historyPage.failed": "失败",
      "historyPage.finalText": "最终文本",
      "historyPage.loadFailed": `加载失败：${options?.error ?? ""}`,
      "historyPage.loadMore": "加载更多",
      "historyPage.modeFilter": "按模式筛选",
      "historyPage.polishLatency": "AI",
      "historyPage.rawText": "原始 ASR",
      "historyPage.sourceText": "编辑原文",
      "historyPage.reprocessAsr": "使用当前引擎重新识别",
      "historyPage.reprocessDone": "已生成新的历史记录",
      "historyPage.reprocessPolish": "使用当前模型重新润色",
      "historyPage.searchPlaceholder": "搜索文本、应用或窗口…",
      "historyPage.showDetails": "查看详情",
      "historyPage.hideDetails": "收起详情",
      "historyPage.success": "成功",
      "historyPage.statusFilter": "按状态筛选",
      "historyPage.title": "历史记录",
      "historyPage.totalLatency": "总计",
    })[key] ?? key,
  }),
}));

import HistoryPage from "@/pages/HistoryPage";

const record = {
  id: 7,
  sessionId: 9,
  createdAt: Date.UTC(2026, 6, 13, 8, 30),
  updatedAt: Date.UTC(2026, 6, 13, 8, 31),
  mode: "dictation" as const,
  workflow: "dictation" as const,
  status: "success" as const,
  text: "润色后的最终文本",
  originalText: "原始识别文本",
  durationSec: 2.4,
  language: "zh",
  engine: "sensevoice",
  appProcess: "Code.exe",
  appWindowTitle: "light-whisper — Visual Studio Code",
  appRuleName: "代码编辑器",
  audioAvailable: true,
  asrMs: 240,
  polishMs: 580,
  totalMs: 860,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.clearAllMocks();
  event.listen.mockResolvedValue(vi.fn());
  api.listTranscriptionHistory.mockResolvedValue({ items: [record], total: 1, hasMore: false });
  api.getTranscriptionHistoryStats.mockResolvedValue({
    total: 1,
    totalCharacters: 8,
    asr: { p50Ms: 240, p95Ms: 240 },
    polish: { p50Ms: 580, p95Ms: 580 },
    totalLatency: { p50Ms: 860, p95Ms: 860 },
  });
  api.reprocessTranscriptionHistory.mockResolvedValue(record);
  api.deleteTranscriptionHistory.mockResolvedValue(true);
});

describe("HistoryPage", () => {
  describe("filter changes during search debounce", () => {
    afterEach(() => vi.useRealTimers());

    it.each([
      { filter: "query", outcome: "success" },
      { filter: "query", outcome: "error" },
      { filter: "mode", outcome: "success" },
      { filter: "mode", outcome: "error" },
      { filter: "status", outcome: "success" },
      { filter: "status", outcome: "error" },
    ])("ignores an obsolete $outcome after the $filter changes before replacement dispatch", async ({ filter, outcome }) => {
      vi.useFakeTimers();
      const obsolete = deferred<PersistentHistoryPage>();
      const latest = deferred<PersistentHistoryPage>();
      api.listTranscriptionHistory
        .mockResolvedValueOnce({ items: [record], total: 2, hasMore: true })
        .mockReturnValueOnce(obsolete.promise)
        .mockReturnValueOnce(latest.promise);
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByText(record.text)).toBeInTheDocument();

      fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
        target: { value: "older" },
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(180); });
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({
        query: "older", mode: "", status: "", limit: 50, offset: 0,
      });

      const expectedFilter = { query: "older", mode: "", status: "" };
      if (filter === "query") {
        fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
          target: { value: "latest" },
        });
        expectedFilter.query = "latest";
      } else if (filter === "mode") {
        fireEvent.click(screen.getByRole("button", { name: "助手" }));
        expectedFilter.mode = "assistant";
      } else {
        fireEvent.change(screen.getByDisplayValue("全部状态"), { target: { value: "failed" } });
        expectedFilter.status = "failed";
      }

      await act(async () => {
        await vi.advanceTimersByTimeAsync(179);
        if (outcome === "success") {
          obsolete.resolve({ items: [{ ...record, id: 8, text: "Obsolete result" }], total: 2, hasMore: true });
        } else {
          obsolete.reject(new Error("Obsolete request failed"));
        }
      });

      expect.soft(api.listTranscriptionHistory).toHaveBeenCalledTimes(2);
      expect.soft(screen.queryByText("Obsolete result")).not.toBeInTheDocument();
      expect.soft(screen.queryByText(record.text)).toBeInTheDocument();
      expect.soft(screen.queryByText("加载失败：Obsolete request failed")).not.toBeInTheDocument();
      expect.soft(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
      const loadMore = screen.queryByRole("button", { name: /加载更多/ });
      expect.soft(loadMore === null || loadMore.hasAttribute("disabled"), "pagination stays unavailable while the filter reset is pending").toBe(true);

      await act(async () => { await vi.advanceTimersByTimeAsync(1); });
      expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(3);
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({ ...expectedFilter, limit: 50, offset: 0 });
      await act(async () => {
        latest.resolve({ items: [{ ...record, id: 9, text: "Latest result" }], total: 1, hasMore: false });
      });
      expect(screen.getByText("Latest result")).toBeInTheDocument();
      expect(screen.queryByText(record.text)).not.toBeInTheDocument();
      expect(screen.queryByText("Obsolete result")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
    });

    it("blocks load-more at the previous filter's offset while a search reset waits for debounce", async () => {
      vi.useFakeTimers();
      const latest = deferred<PersistentHistoryPage>();
      api.listTranscriptionHistory.mockImplementation(({ query }) => query
        ? latest.promise
        : Promise.resolve({ items: [record], total: 2, hasMore: true }));
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByRole("button", { name: /加载更多/ })).toBeEnabled();

      fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
        target: { value: "latest" },
      });
      const loadMore = screen.queryByRole("button", { name: /加载更多/ });
      expect.soft(loadMore === null || loadMore.hasAttribute("disabled"), "pagination is unavailable immediately after changing the filter").toBe(true);
      if (loadMore) fireEvent.click(loadMore);
      expect.soft(api.listTranscriptionHistory).toHaveBeenCalledTimes(1);
      expect.soft(api.listTranscriptionHistory).not.toHaveBeenCalledWith({
        query: "latest", mode: "", status: "", limit: 50, offset: 1,
      });
      expect(screen.getByText(record.text)).toBeInTheDocument();

      await act(async () => { await vi.advanceTimersByTimeAsync(180); });
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({
        query: "latest", mode: "", status: "", limit: 50, offset: 0,
      });
      await act(async () => {
        latest.resolve({ items: [{ ...record, id: 9, text: "Latest result" }], total: 1, hasMore: false });
      });
      expect(screen.getByText("Latest result")).toBeInTheDocument();
      expect(screen.queryByText(record.text)).not.toBeInTheDocument();
    });

    it("shows a current request failure and retries the current filters from offset zero", async () => {
      vi.useFakeTimers();
      const current = deferred<PersistentHistoryPage>();
      const retry = deferred<PersistentHistoryPage>();
      api.listTranscriptionHistory
        .mockResolvedValueOnce({ items: [record], total: 2, hasMore: true })
        .mockReturnValueOnce(current.promise)
        .mockReturnValueOnce(retry.promise);
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByText(record.text)).toBeInTheDocument();

      fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
        target: { value: "current" },
      });
      fireEvent.click(screen.getByRole("button", { name: "助手" }));
      fireEvent.change(screen.getByDisplayValue("全部状态"), { target: { value: "success" } });
      await act(async () => { await vi.advanceTimersByTimeAsync(180); });
      const currentFilter = { query: "current", mode: "assistant", status: "success", limit: 50, offset: 0 };
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(currentFilter);
      await act(async () => { current.reject(new Error("Current request failed")); });

      expect(screen.getByText("加载失败：Current request failed")).toBeVisible();
      fireEvent.click(screen.getByRole("button", { name: "重试" }));
      expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(3);
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith(currentFilter);
      await act(async () => {
        retry.resolve({
          items: [{ ...record, id: 9, mode: "assistant", workflow: "assistant", text: "Retried current result" }],
          total: 1,
          hasMore: false,
        });
      });
      expect(screen.getByText("Retried current result")).toBeVisible();
      expect(screen.queryByText("加载失败：Current request failed")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
      expect(screen.queryByText(record.text)).not.toBeInTheDocument();
    });
  });

  describe("recovery and control semantics", () => {
    afterEach(() => vi.useRealTimers());

    it("keeps pagination unavailable after a failed filter reset until retry replaces the previous page", async () => {
      vi.useFakeTimers();
      const reset = deferred<PersistentHistoryPage>();
      const retry = deferred<PersistentHistoryPage>();
      const recoveredItems = [
        { ...record, id: 8, mode: "assistant" as const, workflow: "assistant" as const, text: "Recovered first result" },
        { ...record, id: 9, mode: "assistant" as const, workflow: "assistant" as const, text: "Recovered second result" },
      ];
      const appendedItem = { ...recoveredItems[0], id: 10, text: "Recovered next page" };
      api.listTranscriptionHistory
        .mockResolvedValueOnce({ items: [record], total: 4, hasMore: true })
        .mockReturnValueOnce(reset.promise)
        .mockReturnValueOnce(retry.promise)
        .mockResolvedValueOnce({ items: [appendedItem], total: 3, hasMore: false });
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByRole("button", { name: /加载更多/ })).toBeEnabled();

      fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
        target: { value: "recovered" },
      });
      fireEvent.click(screen.getByRole("button", { name: "助手" }));
      fireEvent.change(screen.getByDisplayValue("全部状态"), { target: { value: "success" } });
      await act(async () => { await vi.advanceTimersByTimeAsync(180); });
      const currentFilter = { query: "recovered", mode: "assistant", status: "success", limit: 50 };
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({ ...currentFilter, offset: 0 });
      await act(async () => { reset.reject(new Error("Filter reset failed")); });

      expect(screen.getByText("加载失败：Filter reset failed")).toBeVisible();
      expect(screen.getByText(record.text)).toBeVisible();
      const loadMoreAfterFailure = screen.queryByRole("button", { name: /加载更多/ });
      expect.soft(loadMoreAfterFailure === null || loadMoreAfterFailure.hasAttribute("disabled"), "pagination stays unavailable after the current filter reset fails").toBe(true);

      fireEvent.click(screen.getByRole("button", { name: "重试" }));
      expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(3);
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({ ...currentFilter, offset: 0 });
      const loadMoreDuringRetry = screen.queryByRole("button", { name: /加载更多/ });
      expect.soft(loadMoreDuringRetry === null || loadMoreDuringRetry.hasAttribute("disabled"), "pagination stays unavailable until retry succeeds").toBe(true);
      expect(screen.getByText(record.text)).toBeVisible();

      await act(async () => { retry.resolve({ items: recoveredItems, total: 3, hasMore: true }); });
      expect(screen.queryByText(record.text)).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
      for (const item of recoveredItems) expect(screen.getByText(item.text)).toBeVisible();
      expect(screen.getByRole("button", { name: /加载更多/ })).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: /加载更多/ }));
      expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(4);
      expect(api.listTranscriptionHistory).toHaveBeenLastCalledWith({ ...currentFilter, offset: recoveredItems.length });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(screen.getByText(appendedItem.text)).toBeVisible();
      for (const item of recoveredItems) expect(screen.getByText(item.text)).toBeVisible();
      expect(screen.queryByRole("button", { name: /加载更多/ })).not.toBeInTheDocument();
    });

    it("names native comboboxes by their translated purpose when values change", async () => {
      vi.useFakeTimers();
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      const exportFormat = screen.getByDisplayValue("Markdown");
      const statusFilter = screen.getByDisplayValue("全部状态");
      expect(screen.getAllByRole("combobox")).toEqual([exportFormat, statusFilter]);
      expect.soft(exportFormat).toHaveAccessibleName("导出格式");
      expect.soft(statusFilter).toHaveAccessibleName("按状态筛选");

      fireEvent.change(exportFormat, { target: { value: "json" } });
      fireEvent.change(statusFilter, { target: { value: "failed" } });
      expect(exportFormat).toHaveValue("json");
      expect(statusFilter).toHaveValue("failed");
      expect.soft(exportFormat).toHaveAccessibleName("导出格式");
      expect.soft(statusFilter).toHaveAccessibleName("按状态筛选");
    });

    it("exposes exactly one pressed mode and updates it when switching modes", async () => {
      vi.useFakeTimers();
      render(<HistoryPage onNavigate={vi.fn()} />);
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      const modeFilter = screen.getByRole("group");
      expect.soft(modeFilter).toHaveAccessibleName("按模式筛选");
      const modeButtons = ["全部", "听写", "助手"].map((name) => within(modeFilter).getByRole("button", { name }));
      expect.soft(modeButtons.map((button) => button.getAttribute("aria-pressed")), "all modes is selected initially").toEqual(["true", "false", "false"]);

      for (const selected of [1, 2, 0]) {
        fireEvent.click(modeButtons[selected]);
        expect.soft(modeButtons.map((button) => button.getAttribute("aria-pressed")), `selected mode: ${modeButtons[selected].textContent}`).toEqual(modeButtons.map((_, index) => String(index === selected)));
      }
    });
  });

  it("finishes the delete transition before applying a backend history notification", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const { container } = render(<HistoryPage onNavigate={vi.fn()} />);
    await screen.findByText("润色后的最终文本");
    api.listTranscriptionHistory.mockResolvedValue({ items: [], total: 0, hasMore: false });
    fireEvent.click(screen.getByRole("button", { name: "删除记录" }));
    await waitFor(() => expect(container.querySelector(".history-record-motion")).toHaveAttribute("data-removing", "true"));
    const calls = api.listTranscriptionHistory.mock.calls.length;
    const handler = event.listen.mock.calls.find(([name]) => name === "history-updated")?.[1];
    act(() => handler?.({ payload: null }));
    expect(api.listTranscriptionHistory).toHaveBeenCalledTimes(calls);
    expect(screen.getByText("润色后的最终文本")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("润色后的最终文本")).not.toBeInTheDocument());
    expect(api.listTranscriptionHistory.mock.calls.length).toBeGreaterThan(calls);
  });
  it("loads persisted records and forwards search filters", async () => {
    render(<HistoryPage onNavigate={vi.fn()} />);

    expect(await screen.findByText("润色后的最终文本")).toBeInTheDocument();
    expect(screen.getByText("Code.exe")).toBeInTheDocument();
    expect(screen.getByText("规则：代码编辑器")).toBeInTheDocument();
    expect(api.listTranscriptionHistory).toHaveBeenCalledWith({
      query: "",
      mode: "",
      status: "",
      limit: 50,
      offset: 0,
    });

    fireEvent.change(screen.getByLabelText("搜索文本、应用或窗口…"), {
      target: { value: "final" },
    });

    await waitFor(() => {
      expect(api.listTranscriptionHistory).toHaveBeenCalledWith(expect.objectContaining({
        query: "final",
        offset: 0,
      }));
    });
  });

  it("reprocesses and deletes a saved record", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<HistoryPage onNavigate={vi.fn()} />);
    await screen.findByText("润色后的最终文本");

    fireEvent.click(screen.getByRole("button", { name: "使用当前模型重新润色" }));
    await waitFor(() => {
      expect(api.reprocessTranscriptionHistory).toHaveBeenCalledWith(7, "polish");
    });

    fireEvent.click(screen.getByRole("button", { name: "删除记录" }));
    await waitFor(() => {
      expect(api.deleteTranscriptionHistory).toHaveBeenCalledWith(7);
    });
  });

  it("keeps the local record when the backend reports a no-op delete", async () => {
    vi.spyOn(window, "confirm").mockReturnValue(true);
    api.deleteTranscriptionHistory.mockResolvedValue(false);
    render(<HistoryPage onNavigate={vi.fn()} />);
    await screen.findByText("润色后的最终文本");

    fireEvent.click(screen.getByRole("button", { name: "删除记录" }));

    await waitFor(() => expect(api.deleteTranscriptionHistory).toHaveBeenCalledWith(7));
    expect(screen.getByText("润色后的最终文本")).toBeInTheDocument();
  });

  it("shows both the edit instruction and selected source text", async () => {
    api.listTranscriptionHistory.mockResolvedValue({
      items: [{
        ...record,
        workflow: "edit",
        originalText: "把它写得更礼貌",
        sourceText: "这个方案不行。",
        text: "这个方案目前还不够理想。",
      }],
      total: 1,
      hasMore: false,
    });
    render(<HistoryPage onNavigate={vi.fn()} />);
    await screen.findByText("这个方案目前还不够理想。");

    fireEvent.click(screen.getByRole("button", { name: "查看详情" }));

    expect(screen.getByText("把它写得更礼貌")).toBeInTheDocument();
    expect(screen.getByText("编辑原文")).toBeInTheDocument();
    expect(screen.getByText("这个方案不行。")).toBeInTheDocument();
  });

  it("does not offer dictation reprocessing for assistant or edit workflows", async () => {
    api.listTranscriptionHistory.mockResolvedValue({
      items: [
        { ...record, id: 8, workflow: "assistant", mode: "assistant" },
        { ...record, id: 9, workflow: "edit" },
      ],
      total: 2,
      hasMore: false,
    });
    render(<HistoryPage onNavigate={vi.fn()} />);

    await waitFor(() => expect(api.listTranscriptionHistory).toHaveBeenCalled());
    expect(screen.queryByRole("button", { name: "使用当前模型重新润色" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "使用当前引擎重新识别" })).not.toBeInTheDocument();
  });

  it("disposes a listener that resolves after unmount", async () => {
    const dispose = vi.fn();
    let resolveListen: ((unlisten: () => void) => void) | undefined;
    event.listen.mockReturnValue(new Promise((resolve) => { resolveListen = resolve; }));
    const view = render(<HistoryPage onNavigate={vi.fn()} />);
    await waitFor(() => expect(event.listen).toHaveBeenCalledTimes(1));

    view.unmount();
    await act(async () => {
      resolveListen?.(dispose);
      await Promise.resolve();
    });

    expect(dispose).toHaveBeenCalledTimes(1);
  });
});
