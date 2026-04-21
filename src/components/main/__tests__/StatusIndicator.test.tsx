import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StatusIndicator } from "@/components/main/StatusIndicator";

function baseProps(overrides: Partial<React.ComponentProps<typeof StatusIndicator>> = {}) {
  return {
    stage: "ready" as const,
    isReady: true,
    engineLabel: "GLM-ASR",
    device: "MacBook Pro Microphone",
    gpuName: null as string | null,
    error: null as string | null,
    ...overrides,
  };
}

describe("StatusIndicator", () => {
  it("renders at the expected testid", () => {
    render(<StatusIndicator {...baseProps()} />);
    expect(screen.getByTestId("main-status")).toBeInTheDocument();
  });

  it("shows the engine label when ready", () => {
    render(<StatusIndicator {...baseProps()} />);
    expect(screen.getByTestId("main-status")).toHaveTextContent("GLM-ASR");
  });

  it("shows a retry button when in error stage", async () => {
    const onRetry = vi.fn();
    render(
      <StatusIndicator
        {...baseProps({ stage: "error", isReady: false, error: "Boom", onRetry })}
      />,
    );
    const retry = screen.getByTestId("main-retry-btn");
    expect(retry).toBeInTheDocument();
    await userEvent.click(retry);
    expect(onRetry).toHaveBeenCalled();
  });

  it("shows the error text when in error stage", () => {
    render(
      <StatusIndicator
        {...baseProps({ stage: "error", isReady: false, error: "Model failed" })}
      />,
    );
    expect(screen.getByTestId("main-status")).toHaveTextContent("Model failed");
  });
});
