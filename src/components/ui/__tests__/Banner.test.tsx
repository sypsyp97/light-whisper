import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Banner } from "@/components/ui/Banner";

describe("Banner", () => {
  it("renders the message", () => {
    render(<Banner tone="error" message="Something broke" data-testid="main-error-banner" />);
    expect(screen.getByTestId("main-error-banner")).toHaveTextContent("Something broke");
  });

  it("renders the action button when action is provided", async () => {
    const onClick = vi.fn();
    render(
      <Banner
        tone="error"
        message="Broke"
        action={{ label: "Retry", onClick, testId: "main-retry-btn" }}
      />,
    );
    await userEvent.click(screen.getByTestId("main-retry-btn"));
    expect(onClick).toHaveBeenCalled();
  });

  it("shows a dismiss button only when onDismiss is provided", () => {
    const { rerender } = render(<Banner tone="info" message="Hi" />);
    expect(screen.queryByRole("button", { name: /close/i })).not.toBeInTheDocument();
    rerender(<Banner tone="info" message="Hi" onDismiss={vi.fn()} />);
    expect(screen.getByRole("button", { name: /close/i })).toBeInTheDocument();
  });

  it("fires onDismiss when X is clicked", async () => {
    const onDismiss = vi.fn();
    render(<Banner tone="info" message="Hi" onDismiss={onDismiss} />);
    await userEvent.click(screen.getByRole("button", { name: /close/i }));
    expect(onDismiss).toHaveBeenCalled();
  });
});
