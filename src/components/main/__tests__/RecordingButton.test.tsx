import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RecordingButton } from "@/components/main/RecordingButton";

describe("RecordingButton", () => {
  it("renders an enabled button in idle state", () => {
    render(<RecordingButton state="idle" onClick={vi.fn()} data-testid="main-record-btn" />);
    expect(screen.getByTestId("main-record-btn")).toBeEnabled();
  });

  it("calls onClick when clicked in idle state", async () => {
    const onClick = vi.fn();
    render(<RecordingButton state="idle" onClick={onClick} data-testid="main-record-btn" />);
    await userEvent.click(screen.getByTestId("main-record-btn"));
    expect(onClick).toHaveBeenCalled();
  });

  it("is disabled when state is disabled", () => {
    render(<RecordingButton state="disabled" onClick={vi.fn()} data-testid="main-record-btn" />);
    expect(screen.getByTestId("main-record-btn")).toBeDisabled();
  });

  it("is disabled when state is processing", () => {
    render(<RecordingButton state="processing" onClick={vi.fn()} data-testid="main-record-btn" />);
    expect(screen.getByTestId("main-record-btn")).toBeDisabled();
  });

  it("has an aria-label that reflects the recording state", () => {
    render(<RecordingButton state="recording" onClick={vi.fn()} data-testid="main-record-btn" />);
    expect(screen.getByTestId("main-record-btn")).toHaveAttribute("aria-label");
  });
});
