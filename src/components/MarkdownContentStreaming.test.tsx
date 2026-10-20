import type { ComponentProps } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ReactMarkdown from "react-markdown";
import MarkdownContent from "@/components/MarkdownContent";
import { SelectionResult } from "@/features/selection-assistant/SelectionResult";

vi.mock("react-markdown", async (importOriginal) => {
  const { default: OriginalMarkdown } = await importOriginal<typeof import("react-markdown")>();
  return {
    default: vi.fn((props: ComponentProps<typeof OriginalMarkdown>) => <OriginalMarkdown {...props} />),
  };
});

const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
const requestFrame = vi.fn((callback: FrameRequestCallback) => {
  const id = ++nextFrame;
  frames.set(id, callback);
  return id;
});
const cancelFrame = vi.fn((id: number) => { frames.delete(id); });

function flushFrame() {
  act(() => {
    const pending = [...frames.values()];
    frames.clear();
    pending.forEach((callback) => callback(16));
  });
}

beforeEach(() => {
  frames.clear();
  nextFrame = 0;
  vi.clearAllMocks();
  vi.stubGlobal("requestAnimationFrame", requestFrame);
  vi.stubGlobal("cancelAnimationFrame", cancelFrame);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe.each([
  ["MarkdownContent", MarkdownContent],
  ["SelectionResult", SelectionResult],
] as const)("%s streaming", (_name, Component) => {
  it("shows initial content immediately and parses a burst only once at the next frame", () => {
    const { rerender } = render(<Component content="First chunk" streaming />);
    expect(screen.getByText("First chunk")).toBeVisible();
    expect(ReactMarkdown).toHaveBeenCalledTimes(1);

    let content = "First chunk";
    for (let chunk = 1; chunk <= 20; chunk += 1) {
      content += ` ${chunk}`;
      rerender(<Component content={content} streaming />);
    }

    expect.soft(screen.queryByText("First chunk")).toBeVisible();
    expect.soft(ReactMarkdown).toHaveBeenCalledTimes(1);
    expect.soft(frames.size).toBe(1);
    expect.soft(requestFrame).toHaveBeenCalledTimes(1);
    flushFrame();
    expect(screen.getByText(content)).toBeVisible();
    expect(ReactMarkdown).toHaveBeenCalledTimes(2);
    expect(frames.size).toBe(0);
    content += " 21";
    rerender(<Component content={content} streaming />);
    expect(ReactMarkdown).toHaveBeenCalledTimes(2);
    expect(frames.size).toBe(1);
    flushFrame();
    expect(screen.getByText(content)).toBeVisible();
    expect(ReactMarkdown).toHaveBeenCalledTimes(3);
  });
});

describe("MarkdownContent streaming boundaries", () => {
  it("renders every non-streaming content change immediately by default", () => {
    const { container, rerender } = render(<MarkdownContent content="**First**" />);
    expect(screen.getByText("First").tagName).toBe("STRONG");
    rerender(<MarkdownContent content="Second $x^2$" />);
    expect(container.querySelector(".katex")).not.toBeNull();
    expect(screen.queryByText("First")).toBeNull();
    expect(requestFrame).not.toHaveBeenCalled();
  });

  it("shows the first nonempty chunk immediately after an empty stream", () => {
    const { rerender } = render(<MarkdownContent content="" streaming />);
    rerender(<MarkdownContent content="First nonempty chunk" streaming />);
    expect(screen.getByText("First nonempty chunk")).toBeVisible();
    expect(frames.size).toBe(0);
  });

  it("finishing a stream flushes the final content and invalidates the pending frame", () => {
    const { rerender } = render(<MarkdownContent content="Partial" streaming />);
    rerender(<MarkdownContent content="Partial queued" streaming />);
    expect.soft(frames.size).toBe(1);
    const staleCallbacks = [...frames.values()];
    rerender(<MarkdownContent content="**Authoritative final**" streaming={false} />);

    expect(screen.getByText("Authoritative final").tagName).toBe("STRONG");
    expect(frames.size).toBe(0);
    expect(cancelFrame).toHaveBeenCalledTimes(1);
    act(() => staleCallbacks.forEach((callback) => callback(16)));
    expect(screen.getByText("Authoritative final")).toBeVisible();
    expect(screen.queryByText("Partial queued")).toBeNull();
  });

  it("clears immediately on reset and never revives queued content in a replacement stream", () => {
    const { container, rerender } = render(<MarkdownContent content="Old stream" streaming />);
    rerender(<MarkdownContent content="Old stream queued" streaming />);
    expect.soft(frames.size).toBe(1);
    const staleCallbacks = [...frames.values()];
    rerender(<MarkdownContent content="" streaming />);
    expect(container.textContent).toBe("");
    expect(frames.size).toBe(0);

    rerender(<MarkdownContent content="Replacement first chunk" streaming />);
    expect(screen.getByText("Replacement first chunk")).toBeVisible();
    act(() => staleCallbacks.forEach((callback) => callback(16)));
    expect(screen.queryByText(/Old stream/)).toBeNull();
    expect(screen.getByText("Replacement first chunk")).toBeVisible();
  });

  it("flushes a non-prefix replacement immediately and invalidates queued content from the previous stream", () => {
    const { rerender } = render(<MarkdownContent content="Previous request" streaming />);
    rerender(<MarkdownContent content="Previous request queued" streaming />);
    expect.soft(frames.size).toBe(1);
    const staleCallbacks = [...frames.values()];
    rerender(<MarkdownContent content="Replacement request" streaming />);

    expect(screen.getByText("Replacement request")).toBeVisible();
    expect(screen.queryByText(/Previous request/)).toBeNull();
    expect(frames.size).toBe(0);
    expect(cancelFrame).toHaveBeenCalledTimes(1);
    act(() => staleCallbacks.forEach((callback) => callback(16)));
    expect(screen.getByText("Replacement request")).toBeVisible();
    rerender(<MarkdownContent content="Replacement request continued" streaming />);
    expect(screen.getByText("Replacement request")).toBeVisible();
    expect(frames.size).toBe(1);
    flushFrame();
    expect(screen.getByText("Replacement request continued")).toBeVisible();
  });

  it("cancels pending work on unmount", () => {
    const { rerender, unmount } = render(<MarkdownContent content="Partial" streaming />);
    rerender(<MarkdownContent content="Partial queued" streaming />);
    expect.soft(frames.size).toBe(1);
    unmount();
    expect(cancelFrame).toHaveBeenCalledTimes(1);
    expect(frames.size).toBe(0);
  });

  it("preserves math, raw-HTML blocking, and the current link callback after a frame", () => {
    const firstOpenLink = vi.fn();
    const latestOpenLink = vi.fn();
    const { container, rerender } = render(
      <MarkdownContent content="Formula $x" streaming onOpenLink={firstOpenLink} />,
    );
    rerender(
      <MarkdownContent
        content={'Formula $x^2$. [Source](https://example.com/source)\n\n<img src=x onerror="alert(1)">'}
        streaming
        onOpenLink={latestOpenLink}
      />,
    );
    flushFrame();
    expect(container.querySelectorAll(".katex")).toHaveLength(1);
    expect(container.querySelector("img, script")).toBeNull();
    fireEvent.click(screen.getByRole("link", { name: "Source" }));
    expect(latestOpenLink).toHaveBeenCalledWith("https://example.com/source");
    expect(firstOpenLink).not.toHaveBeenCalled();
  });
});
