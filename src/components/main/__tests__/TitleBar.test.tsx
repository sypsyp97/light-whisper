import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Settings } from "lucide-react";
import { TitleBar } from "@/components/main/TitleBar";

describe("TitleBar", () => {
  it("renders with titlebar testid", () => {
    render(<TitleBar />);
    expect(screen.getByTestId("titlebar")).toBeInTheDocument();
  });

  it("fires leftAction.onClick when the left button is clicked", async () => {
    const onClick = vi.fn();
    render(
      <TitleBar
        leftAction={{ icon: <Settings size={14} />, label: "Settings", onClick }}
      />,
    );
    await userEvent.click(screen.getByTestId("titlebar-left-action"));
    expect(onClick).toHaveBeenCalled();
  });

  it("calls onMinimize when the minimize button is clicked", async () => {
    const onMinimize = vi.fn();
    render(<TitleBar onMinimize={onMinimize} />);
    await userEvent.click(screen.getByTestId("titlebar-minimize"));
    expect(onMinimize).toHaveBeenCalled();
  });

  it("calls onClose when the close button is clicked", async () => {
    const onClose = vi.fn();
    render(<TitleBar onClose={onClose} />);
    await userEvent.click(screen.getByTestId("titlebar-close"));
    expect(onClose).toHaveBeenCalled();
  });

  it("renders custom title when provided", () => {
    render(<TitleBar title="Custom" />);
    expect(screen.getByText("Custom")).toBeInTheDocument();
  });
});
