import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RecordingStage } from "@/components/main/RecordingStage";

function baseProps(overrides: Partial<React.ComponentProps<typeof RecordingStage>> = {}) {
  return {
    isRecording: false,
    isProcessing: false,
    isReady: true,
    hotkeyDisplay: "F2",
    recordingMode: "toggle" as const,
    error: null as string | null,
    onToggle: vi.fn(),
    ...overrides,
  };
}

describe("RecordingStage", () => {
  it("renders the stage root", () => {
    render(<RecordingStage {...baseProps()} />);
    expect(screen.getByTestId("main-record-stage")).toBeInTheDocument();
  });

  it("renders the record button", () => {
    render(<RecordingStage {...baseProps()} />);
    expect(screen.getByTestId("main-record-btn")).toBeInTheDocument();
  });

  it("fires onToggle when the record button is clicked", async () => {
    const onToggle = vi.fn();
    render(<RecordingStage {...baseProps({ onToggle })} />);
    await userEvent.click(screen.getByTestId("main-record-btn"));
    expect(onToggle).toHaveBeenCalled();
  });

  it("disables the record button when not ready", () => {
    render(<RecordingStage {...baseProps({ isReady: false })} />);
    expect(screen.getByTestId("main-record-btn")).toBeDisabled();
  });

  it("disables the record button while processing", () => {
    render(<RecordingStage {...baseProps({ isProcessing: true })} />);
    expect(screen.getByTestId("main-record-btn")).toBeDisabled();
  });
});
