import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { OnboardingHint } from "@/components/main/OnboardingHint";

describe("OnboardingHint", () => {
  it("renders at the expected testid", () => {
    render(<OnboardingHint hotkeyDisplay="F2" mode="toggle" onDismiss={vi.fn()} />);
    expect(screen.getByTestId("main-onboarding")).toBeInTheDocument();
  });

  it("shows the hotkey display string", () => {
    render(<OnboardingHint hotkeyDisplay="Ctrl+Alt+Space" mode="hold" onDismiss={vi.fn()} />);
    expect(screen.getByTestId("main-onboarding")).toHaveTextContent(/Ctrl|Alt|Space/);
  });

  it("calls onDismiss when the dismiss button is clicked", async () => {
    const onDismiss = vi.fn();
    render(<OnboardingHint hotkeyDisplay="F2" mode="toggle" onDismiss={onDismiss} />);
    await userEvent.click(screen.getByTestId("main-onboarding-dismiss"));
    expect(onDismiss).toHaveBeenCalled();
  });
});
