import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Toggle } from "@/components/ui/Toggle";

describe("Toggle", () => {
  it("renders a switch role element", () => {
    render(<Toggle checked={false} onChange={vi.fn()} label="Enable" />);
    expect(screen.getByRole("switch", { name: "Enable" })).toBeInTheDocument();
  });

  it("reflects checked state via aria-checked", () => {
    render(<Toggle checked onChange={vi.fn()} label="Enable" />);
    expect(screen.getByRole("switch", { name: "Enable" })).toHaveAttribute("aria-checked", "true");
  });

  it("calls onChange with inverted value on click", async () => {
    const onChange = vi.fn();
    render(<Toggle checked={false} onChange={onChange} label="Enable" />);
    await userEvent.click(screen.getByRole("switch", { name: "Enable" }));
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("toggles via Space key", async () => {
    const onChange = vi.fn();
    render(<Toggle checked onChange={onChange} label="Enable" />);
    const el = screen.getByRole("switch", { name: "Enable" });
    el.focus();
    await userEvent.keyboard(" ");
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("does not fire onChange when disabled", async () => {
    const onChange = vi.fn();
    render(<Toggle checked={false} onChange={onChange} label="Enable" disabled />);
    await userEvent.click(screen.getByRole("switch", { name: "Enable" }));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("forwards data-testid", () => {
    render(
      <Toggle checked={false} onChange={vi.fn()} label="Enable" data-testid="mic-level-monitor-toggle" />,
    );
    expect(screen.getByTestId("mic-level-monitor-toggle")).toBeInTheDocument();
  });
});
