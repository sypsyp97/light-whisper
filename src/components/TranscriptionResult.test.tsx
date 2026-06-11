import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import TranscriptionResult from "@/components/TranscriptionResult";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, vars?: Record<string, unknown>) => {
      if (key === "result.title") return "Result";
      if (key === "result.stats") {
        return `${vars?.chars} chars · ${vars?.duration}s · ${vars?.cpm} chars/min`;
      }
      if (key === "result.latency.asr") return `ASR ${vars?.ms}ms`;
      if (key === "result.latency.ai") return `AI ${vars?.ms}ms`;
      if (key === "result.latency.total") return `total ${vars?.ms}ms`;
      if (key === "result.rawFirst.polished_preview") return "polish complete";
      if (key === "result.rawFirst.replaced") return "raw-first replaced";
      return key;
    },
  }),
}));

describe("TranscriptionResult", () => {
  it("shows ASR, AI polish, and total latency when timing is available", () => {
    render(
      <TranscriptionResult
        text="hello"
        originalText="hello"
        isProcessing={false}
        copiedId={null}
        onCopy={vi.fn()}
        durationSec={1.2}
        charCount={5}
        timing={{ asrMs: 42, polishMs: 900, totalMs: 948, rawFirst: { status: "replaced" } }}
      />,
    );

    expect(screen.getByText(/ASR 42ms/)).toBeInTheDocument();
    expect(screen.getByText(/AI 900ms/)).toBeInTheDocument();
    expect(screen.getByText(/total 948ms/)).toBeInTheDocument();
    expect(screen.getByText(/raw-first replaced/)).toBeInTheDocument();
  });

  it("shows polish complete after a polished preview-only result arrives", () => {
    render(
      <TranscriptionResult
        text="你好。"
        originalText="ni hao"
        isProcessing={false}
        copiedId={null}
        onCopy={vi.fn()}
        durationSec={null}
        charCount={null}
        timing={{ asrMs: 42, polishMs: 900, totalMs: 948, rawFirst: { status: "preview_only" } }}
        resultStage="polished"
      />,
    );

    expect(screen.getByText("polish complete")).toBeInTheDocument();
    expect(screen.queryByText("result.rawFirst.preview_only")).not.toBeInTheDocument();
  });

  it("keeps raw preview read-only and allows editing after polished result", () => {
    const { rerender } = render(
      <TranscriptionResult
        text="ni hao"
        originalText="ni hao"
        isProcessing={false}
        copiedId={null}
        onCopy={vi.fn()}
        durationSec={null}
        charCount={null}
        resultStage="raw"
      />,
    );

    expect(screen.getByLabelText("result.editableTranscription")).toHaveAttribute("readonly");

    rerender(
      <TranscriptionResult
        text="你好。"
        originalText="ni hao"
        isProcessing={false}
        copiedId={null}
        onCopy={vi.fn()}
        durationSec={null}
        charCount={null}
        resultStage="polished"
      />,
    );

    expect(screen.getByLabelText("result.editableTranscription")).not.toHaveAttribute("readonly");
  });
});
