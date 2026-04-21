import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Button } from "@/components/ui/Button";

describe("Button", () => {
  it("renders children inside a button element", () => {
    render(<Button>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toBeInTheDocument();
  });

  it("uses provided data-testid when passed", () => {
    render(<Button data-testid="engine-save-btn">Save</Button>);
    expect(screen.getByTestId("engine-save-btn")).toBeInTheDocument();
  });

  it("falls back to ui-button testid when consumer omits one", () => {
    render(<Button>Save</Button>);
    expect(screen.getByTestId("ui-button")).toBeInTheDocument();
  });

  it("calls onClick when clicked", async () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Save</Button>);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("disables the button when loading is true", () => {
    render(<Button loading>Save</Button>);
    expect(screen.getByRole("button", { name: /Save/ })).toBeDisabled();
  });

  it("does not fire onClick when disabled", async () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Save
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(onClick).not.toHaveBeenCalled();
  });

  it("renders with type button by default", () => {
    render(<Button>Save</Button>);
    expect(screen.getByRole("button", { name: "Save" })).toHaveAttribute("type", "button");
  });
});
