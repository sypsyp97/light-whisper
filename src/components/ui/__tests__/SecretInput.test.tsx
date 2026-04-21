import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SecretInput } from "@/components/ui/SecretInput";

describe("SecretInput", () => {
  it("renders masked by default", () => {
    render(<SecretInput value="sk-secret" onChange={vi.fn()} data-testid="engine-api-key" />);
    expect(screen.getByTestId("engine-api-key")).toHaveAttribute("type", "password");
  });

  it("toggles visibility when the reveal button is clicked", async () => {
    render(<SecretInput value="sk-secret" onChange={vi.fn()} data-testid="engine-api-key" />);
    await userEvent.click(screen.getByTestId("engine-api-key-reveal"));
    expect(screen.getByTestId("engine-api-key")).toHaveAttribute("type", "text");
  });

  it("fires onChange with the raw string when the user types", async () => {
    const onChange = vi.fn();
    render(<SecretInput value="" onChange={onChange} data-testid="engine-api-key" />);
    await userEvent.type(screen.getByTestId("engine-api-key"), "ab");
    expect(onChange).toHaveBeenCalled();
    const lastArg = onChange.mock.calls[onChange.mock.calls.length - 1][0];
    expect(typeof lastArg).toBe("string");
  });

  it("honors placeholder prop", () => {
    render(
      <SecretInput
        value=""
        onChange={vi.fn()}
        placeholder="Enter key"
        data-testid="engine-api-key"
      />,
    );
    expect(screen.getByTestId("engine-api-key")).toHaveAttribute("placeholder", "Enter key");
  });
});
