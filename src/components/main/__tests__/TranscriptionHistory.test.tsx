import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TranscriptionHistory } from "@/components/main/TranscriptionHistory";
import type { HistoryItem } from "@/types";

const items: HistoryItem[] = [
  { id: "1", text: "first transcription", originalText: "first transcription", timestamp: 1000, timeDisplay: "10:00" },
  { id: "2", text: "second transcription", originalText: "second transcription", timestamp: 2000, timeDisplay: "10:01" },
];

describe("TranscriptionHistory", () => {
  it("renders the list root", () => {
    render(<TranscriptionHistory items={items} onCopy={vi.fn()} />);
    expect(screen.getByTestId("main-history")).toBeInTheDocument();
  });

  it("renders an item per history entry with its testid", () => {
    render(<TranscriptionHistory items={items} onCopy={vi.fn()} />);
    expect(screen.getByTestId("main-history-item-1")).toBeInTheDocument();
    expect(screen.getByTestId("main-history-item-2")).toBeInTheDocument();
  });

  it("calls onCopy with the item when its copy button is clicked", async () => {
    const onCopy = vi.fn();
    render(<TranscriptionHistory items={items} onCopy={onCopy} />);
    await userEvent.click(screen.getByTestId("main-history-copy-1"));
    expect(onCopy).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }));
  });

  it("renders nothing when items is empty", () => {
    const { container } = render(<TranscriptionHistory items={[]} onCopy={vi.fn()} />);
    // Root may still render; just assert no item rows.
    expect(container.querySelectorAll("[data-testid^='main-history-item-']").length).toBe(0);
  });
});
