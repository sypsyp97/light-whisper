import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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
      "historyPage.allModes": "全部",
      "historyPage.allStatuses": "全部状态",
      "historyPage.appRule": `规则：${options?.name ?? ""}`,
      "historyPage.asrLatency": "ASR",
      "historyPage.assistant": "助手",
      "historyPage.delete": "删除记录",
      "historyPage.deleteConfirm": "确认删除",
      "historyPage.dictation": "听写",
      "historyPage.export": "导出",
      "historyPage.exportJson": "JSON",
      "historyPage.exportMarkdown": "Markdown",
      "historyPage.failed": "失败",
      "historyPage.finalText": "最终文本",
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
