import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  cancelSelectionAction: vi.fn(),
  copySelection: vi.fn(),
  getSelectionOverlayState: vi.fn(),
  hideSelectionAssistant: vi.fn(),
  replaceSelection: vi.fn(),
  resizeSelectionWindow: vi.fn(),
  runSelectionAction: vi.fn(),
  searchSelection: vi.fn(),
  startSelectionWindowDrag: vi.fn(),
}));
const currentWindow = vi.hoisted(() => ({
  hide: vi.fn(),
  startDragging: vi.fn(),
  startResizeDragging: vi.fn(),
}));

vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => currentWindow,
}));
vi.mock("@/hooks/useTheme", () => ({ useTheme: vi.fn() }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => ({
      "common.cancel": "取消",
      "common.close": "关闭",
      "selection.toolbarLabel": "划词助手",
      "selection.translate": "翻译",
      "selection.explain": "解释",
      "selection.optimize": "优化",
      "selection.copy": "复制",
      "selection.search": "搜索",
      "selection.result": "划词助手结果",
      "selection.copyResult": "复制结果",
      "selection.copied": "已复制",
      "selection.replaceResult": "一键替换",
      "selection.replaceWorking": "正在替换",
      "selection.workingHint": "处理中",
      "selection.translateWorking": "翻译中",
      "selection.explainWorking": "解释中",
      "selection.optimizeWorking": "优化中",
      "selection.error": "错误",
      "selection.resize": "调整结果窗口大小",
    })[key] ?? key,
  }),
}));

import SelectionOverlay from "@/pages/SelectionOverlay";

async function renderReadyOverlay(text = "Selected source text") {
  api.getSelectionOverlayState.mockResolvedValue({
    version: 1,
    text,
  });
  const rendered = render(<SelectionOverlay />);

  await waitFor(() => {
    expect(screen.getByRole("button", { name: "翻译" })).toBeEnabled();
  });
  return rendered;
}

beforeEach(() => {
  api.cancelSelectionAction.mockReset().mockResolvedValue(undefined);
  api.copySelection.mockReset().mockResolvedValue(undefined);
  api.getSelectionOverlayState.mockReset();
  api.hideSelectionAssistant.mockReset().mockResolvedValue(undefined);
  api.replaceSelection.mockReset().mockResolvedValue(undefined);
  api.resizeSelectionWindow.mockReset().mockResolvedValue(undefined);
  api.runSelectionAction.mockReset();
  api.searchSelection.mockReset().mockResolvedValue(undefined);
  api.startSelectionWindowDrag.mockReset().mockResolvedValue(undefined);
  currentWindow.hide.mockReset().mockResolvedValue(undefined);
  currentWindow.startDragging.mockReset().mockResolvedValue(undefined);
  currentWindow.startResizeDragging.mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SelectionOverlay dismissal", () => {
  it("closes from the explicit close button", async () => {
    const user = userEvent.setup();
    await renderReadyOverlay();

    await user.click(screen.getByRole("button", { name: "关闭" }));

    expect(api.cancelSelectionAction).toHaveBeenCalledTimes(1);
    expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
  });

  it.each(["pointerDown", "click"] as const)(
    "closes when the close button receives an independent %s event",
    async (eventName) => {
      await renderReadyOverlay();
      const closeButton = screen.getByRole("button", { name: "关闭" });

      if (eventName === "pointerDown") {
        fireEvent.pointerDown(closeButton, { button: 0 });
      } else {
        fireEvent.click(closeButton);
      }

      await waitFor(() => {
        expect(api.cancelSelectionAction).toHaveBeenCalledTimes(1);
        expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
        expect(currentWindow.hide).toHaveBeenCalledTimes(1);
      });
    },
  );

  it("continues hiding when cancellation rejects", async () => {
    api.cancelSelectionAction.mockRejectedValueOnce(new Error("cancel IPC failed"));
    await renderReadyOverlay();

    fireEvent.pointerDown(screen.getByRole("button", { name: "关闭" }), {
      button: 0,
    });

    await waitFor(() => {
      expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
      expect(currentWindow.hide).toHaveBeenCalledTimes(1);
    });
  });

  it("closes when the user presses the blank area outside the toolbar", async () => {
    const { container } = await renderReadyOverlay();
    const blankArea = container.querySelector(".selection-overlay");
    expect(blankArea).not.toBeNull();

    fireEvent.pointerDown(blankArea!);

    await waitFor(() => {
      expect(api.cancelSelectionAction).toHaveBeenCalledTimes(1);
      expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
      expect(currentWindow.hide).toHaveBeenCalledTimes(1);
    });
  });

  it("still closes from outside after a result has expanded the window", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Translated result");
    const { container } = await renderReadyOverlay();

    await user.click(screen.getByRole("button", { name: "翻译" }));
    expect(await screen.findByText("Translated result")).toBeInTheDocument();
    api.cancelSelectionAction.mockClear();
    api.hideSelectionAssistant.mockClear();

    fireEvent.pointerDown(container.querySelector(".selection-overlay")!);

    await waitFor(() => {
      expect(api.cancelSelectionAction).toHaveBeenCalledTimes(1);
      expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
      expect(currentWindow.hide).toHaveBeenCalledTimes(1);
    });
  });
});

describe("SelectionOverlay window dragging", () => {
  it("starts Tauri's native drag operation from the selected-text preview", async () => {
    await renderReadyOverlay("Drag this selection preview");
    const preview = screen
      .getByText("Drag this selection preview")
      .closest(".selection-preview-row");
    expect(preview).not.toBeNull();

    fireEvent.pointerDown(preview!, { button: 0 });

    expect(currentWindow.startDragging).toHaveBeenCalledTimes(1);
    expect(api.startSelectionWindowDrag).not.toHaveBeenCalled();
  });

  it("keeps the expanded result header draggable", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Translated result");
    const { container } = await renderReadyOverlay();
    await user.click(screen.getByRole("button", { name: "翻译" }));
    await screen.findByText("Translated result");
    const resultHeader = container.querySelector(".selection-result-header");
    expect(resultHeader).not.toBeNull();

    fireEvent.pointerDown(resultHeader!, { button: 0 });

    expect(currentWindow.startDragging).toHaveBeenCalledTimes(1);
  });

  it("falls back to the dedicated drag command when Tauri dragging rejects", async () => {
    currentWindow.startDragging.mockRejectedValueOnce(new Error("drag unavailable"));
    await renderReadyOverlay("Fallback drag");
    const preview = screen
      .getByText("Fallback drag")
      .closest(".selection-preview-row");

    fireEvent.pointerDown(preview!, { button: 0 });

    await waitFor(() => {
      expect(api.startSelectionWindowDrag).toHaveBeenCalledTimes(1);
    });
  });
});

describe("SelectionOverlay window resizing", () => {
  it("starts native southeast resizing from the result handle", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Resizable result");
    await renderReadyOverlay();
    await user.click(screen.getByRole("button", { name: "翻译" }));

    fireEvent.pointerDown(
      await screen.findByRole("button", { name: "调整结果窗口大小" }),
      { button: 0 },
    );

    expect(currentWindow.startResizeDragging).toHaveBeenCalledWith("SouthEast");
  });
});

describe("SelectionOverlay content and result actions", () => {
  it("shows a readable preview of the selected source text", async () => {
    await renderReadyOverlay("A source sentence the user selected");

    expect(screen.getByText("A source sentence the user selected")).toBeVisible();
  });

  it.each([
    ["翻译", "Translated result"],
    ["解释", "Explained result"],
    ["优化", "Optimized result"],
  ] as const)("copies the generated %s result", async (actionLabel, result) => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue(result);
    await renderReadyOverlay();

    await user.click(screen.getByRole("button", { name: actionLabel }));
    expect(await screen.findByText(result)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "复制结果" }));

    expect(api.copySelection).toHaveBeenCalledWith(result);
  });

  it("replaces the exact source selection with an optimized result", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Optimized result");
    await renderReadyOverlay("Original selection");

    await user.click(screen.getByRole("button", { name: "优化" }));
    await screen.findByText("Optimized result");
    const copyButton = screen.getByRole("button", { name: "复制结果" });
    const replaceButton = screen.getByRole("button", { name: "一键替换" });
    expect(
      Array.from(copyButton.parentElement!.querySelectorAll("button")).map(
        (button) => button.textContent,
      ),
    ).toEqual(["复制结果", "一键替换"]);

    await user.click(replaceButton);

    expect(api.replaceSelection).toHaveBeenCalledWith({
      replacementText: "Optimized result",
      sourceText: "Original selection",
      version: 1,
    });
    expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
    expect(currentWindow.hide).toHaveBeenCalledTimes(1);
  });

  it.each(["翻译", "解释"])(
    "does not offer source replacement for a %s result",
    async (actionLabel) => {
      const user = userEvent.setup();
      api.runSelectionAction.mockResolvedValue("Read-only result");
      await renderReadyOverlay();

      await user.click(screen.getByRole("button", { name: actionLabel }));
      await screen.findByText("Read-only result");

      expect(screen.queryByRole("button", { name: "一键替换" })).toBeNull();
    },
  );

  it("disables replacement while the paste transaction is in flight", async () => {
    const user = userEvent.setup();
    let finishReplacement: (() => void) | undefined;
    api.runSelectionAction.mockResolvedValue("Optimized result");
    api.replaceSelection.mockImplementation(
      () => new Promise<void>((resolve) => { finishReplacement = resolve; }),
    );
    await renderReadyOverlay("Original selection");

    await user.click(screen.getByRole("button", { name: "优化" }));
    await user.click(await screen.findByRole("button", { name: "一键替换" }));

    const pendingButton = screen.getByRole("button", { name: "正在替换" });
    expect(pendingButton).toBeDisabled();
    fireEvent.click(pendingButton);
    expect(api.replaceSelection).toHaveBeenCalledTimes(1);

    finishReplacement?.();
    await waitFor(() => {
      expect(api.hideSelectionAssistant).toHaveBeenCalledTimes(1);
    });
  });

  it("keeps the optimized result open when guarded replacement fails", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Optimized result");
    api.replaceSelection.mockRejectedValue(
      new Error("原选区或目标窗口已变化，请重新划词后再试"),
    );
    await renderReadyOverlay("Original selection");

    await user.click(screen.getByRole("button", { name: "优化" }));
    await user.click(await screen.findByRole("button", { name: "一键替换" }));

    expect(
      await screen.findByText("原选区或目标窗口已变化，请重新划词后再试"),
    ).toBeVisible();
    expect(api.hideSelectionAssistant).not.toHaveBeenCalled();
    expect(currentWindow.hide).not.toHaveBeenCalled();
  });

  it("does not treat clicks inside the result card as outside dismissals", async () => {
    const user = userEvent.setup();
    api.runSelectionAction.mockResolvedValue("Explained result");
    const { container } = await renderReadyOverlay();

    await user.click(screen.getByRole("button", { name: "解释" }));
    await screen.findByText("Explained result");
    api.cancelSelectionAction.mockClear();
    api.hideSelectionAssistant.mockClear();

    fireEvent.pointerDown(container.querySelector(".selection-result")!);
    await act(async () => Promise.resolve());

    expect(api.hideSelectionAssistant).not.toHaveBeenCalled();
  });
});
