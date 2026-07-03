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

  it("shows ASR, AI polish, and total latency when timing is available", () => {
    render(
      <TranscriptionResult
        {...baseProps({
          timing: { asrMs: 42, polishMs: 900, totalMs: 948, rawFirst: { status: "replaced" } },
        })}
      />,
    );

    expect(screen.getByText(/result.latency.asr/)).toBeInTheDocument();
    expect(screen.getByText(/result.latency.ai/)).toBeInTheDocument();
    expect(screen.getByText(/result.latency.total/)).toBeInTheDocument();
    expect(screen.getByText(/result.rawFirst.replaced/)).toBeInTheDocument();
  });

  it("shows polish complete after a polished preview-only result arrives", () => {
    render(
      <TranscriptionResult
        {...baseProps({
          text: "你好。",
          originalText: "ni hao",
          durationSec: null,
          charCount: null,
          timing: { asrMs: 42, polishMs: 900, totalMs: 948, rawFirst: { status: "preview_only" } },
          resultStage: "polished",
        })}
      />,
    );

    expect(screen.getByText("result.rawFirst.polished_preview")).toBeInTheDocument();
    expect(screen.queryByText("result.rawFirst.preview_only")).not.toBeInTheDocument();
  });

  it("keeps raw preview read-only and allows editing after polished result", () => {
    const { rerender } = render(
      <TranscriptionResult
        {...baseProps({
          text: "ni hao",
          originalText: "ni hao",
          durationSec: null,
          charCount: null,
          resultStage: "raw",
        })}
      />,
    );

    expect(screen.getByLabelText("result.editableTranscription")).toHaveAttribute("readonly");

    rerender(
      <TranscriptionResult
        {...baseProps({
          text: "你好。",
          originalText: "ni hao",
          durationSec: null,
          charCount: null,
          resultStage: "polished",
        })}
      />,
    );

    expect(screen.getByLabelText("result.editableTranscription")).not.toHaveAttribute("readonly");
  });
});
