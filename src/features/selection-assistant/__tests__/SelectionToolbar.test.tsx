import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import {
  SelectionToolbar,
  type SelectionToolbarAction,
} from "../SelectionToolbar";

function renderToolbar(overrides: Partial<Parameters<typeof SelectionToolbar>[0]> = {}) {
  const onAction = vi.fn<(action: SelectionToolbarAction) => void>();
  const onScreenshot = vi.fn();
  const onCancelScreenshot = vi.fn();
  const onStartDrag = vi.fn();
  const props = {
    selectionText: "selected text",
    screenshotStatus: "idle" as const,
    onAction,
    onScreenshot,
    onCancelScreenshot,
    onStartDrag,
    ...overrides,
  };

  return { ...render(<SelectionToolbar {...props} />), ...props };
}

describe("SelectionToolbar", () => {
  it("exposes an accessible toolbar", () => {
    renderToolbar();

    expect(screen.getByRole("toolbar", { name: "划词助手" })).toBeInTheDocument();
  });

  it.each(["翻译", "解释", "优化", "复制", "搜索", "截图辅助"])(
    "renders a visible %s action label",
    (name) => {
      renderToolbar();

      const button = screen.getByRole("button", { name });
      expect(button).toHaveTextContent(name);
    },
  );

  it("lets the explicit close button dismiss the toolbar", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderToolbar({ onClose });

    await user.click(screen.getByRole("button", { name: "关闭" }));

    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["翻译", "translate"],
    ["解释", "explain"],
    ["优化", "optimize"],
    ["复制", "copy"],
    ["搜索", "search"],
  ] as const)("dispatches %s as %s", async (label, action) => {
    const user = userEvent.setup();
    const { onAction } = renderToolbar();

    await user.click(screen.getByRole("button", { name: label }));

    expect(onAction).toHaveBeenCalledWith(action);
  });

  it("keeps actions out of the keyboard focus order", () => {
    renderToolbar();
    const button = screen.getByRole("button", { name: "翻译" });

    expect(button).toHaveAttribute("tabindex", "-1");
  });

  it("isolates its own pointer events from the outside-click dismiss layer", () => {
    const outsidePointerDown = vi.fn();
    const { container } = render(
      <div onPointerDown={outsidePointerDown}>
        <SelectionToolbar
          selectionText="selected text"
          screenshotStatus="idle"
          onAction={vi.fn()}
          onScreenshot={vi.fn()}
          onCancelScreenshot={vi.fn()}
          onStartDrag={vi.fn()}
        />
      </div>,
    );

    fireEvent.pointerDown(screen.getByRole("button", { name: "复制" }));

    expect(container).toBeTruthy();
    expect(outsidePointerDown).not.toHaveBeenCalled();
  });

  it("switches screenshot assistance to an explicit cancel action while capture is active", async () => {
    const user = userEvent.setup();
    const { onCancelScreenshot, onScreenshot } = renderToolbar({
      screenshotStatus: "capturing",
    });

    expect(screen.queryByRole("button", { name: "截图辅助" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "取消截图" }));

    expect(onCancelScreenshot).toHaveBeenCalledTimes(1);
    expect(onScreenshot).not.toHaveBeenCalled();
  });

  it("disables model actions for a normalized empty selection but keeps screenshot available", () => {
    renderToolbar({ selectionText: "  \r\n " });

    for (const name of ["翻译", "解释", "复制", "搜索"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
    }
    expect(screen.getByRole("button", { name: "截图辅助" })).toBeEnabled();
  });
});
