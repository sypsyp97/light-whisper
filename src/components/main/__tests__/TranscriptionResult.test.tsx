import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TranscriptionResult } from "@/components/main/TranscriptionResult";

function baseProps(overrides: Partial<React.ComponentProps<typeof TranscriptionResult>> = {}) {
  return {
    text: "hello world",
    originalText: "hello world",
    mode: "dictation" as const,
    durationSec: 6,
    charCount: 11,
    detectedLanguage: "en",
    onChange: vi.fn(),
    onSubmitCorrection: vi.fn(),
    onCopy: vi.fn(),
    ...overrides,
  };
}

describe("TranscriptionResult", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the result root", () => {
    render(<TranscriptionResult {...baseProps()} />);
    expect(screen.getByTestId("main-result")).toBeInTheDocument();
  });

  it("renders the stats line", () => {
    render(<TranscriptionResult {...baseProps()} />);
    expect(screen.getByTestId("main-result-stats")).toBeInTheDocument();
  });

  it("fires onCopy when the copy button is clicked", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const onCopy = vi.fn();
    render(<TranscriptionResult {...baseProps({ onCopy })} />);
    await user.click(screen.getByTestId("main-result-copy"));
    expect(onCopy).toHaveBeenCalled();
  });

  it("renders the editable text region", () => {
    render(<TranscriptionResult {...baseProps()} />);
    expect(screen.getByTestId("main-result-text")).toBeInTheDocument();
  });
});
