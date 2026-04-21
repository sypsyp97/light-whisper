import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Picker } from "@/components/ui/Picker";

type Val = "a" | "b" | "c";
const options = [
  { value: "a" as Val, label: "Alpha" },
  { value: "b" as Val, label: "Beta" },
  { value: "c" as Val, label: "Gamma" },
];

describe("Picker", () => {
  it("renders the trigger with the current selection label", () => {
    render(<Picker<Val> value="a" options={options} onChange={vi.fn()} data-testid="engine-picker" />);
    expect(screen.getByTestId("engine-picker")).toHaveTextContent("Alpha");
  });

  it("opens the popover on trigger click", async () => {
    render(<Picker<Val> value="a" options={options} onChange={vi.fn()} data-testid="engine-picker" />);
    await userEvent.click(screen.getByTestId("engine-picker"));
    expect(screen.getByTestId("engine-picker-popover")).toBeInTheDocument();
  });

  it("calls onChange when an option is selected", async () => {
    const onChange = vi.fn();
    render(<Picker<Val> value="a" options={options} onChange={onChange} data-testid="engine-picker" />);
    await userEvent.click(screen.getByTestId("engine-picker"));
    await userEvent.click(screen.getByTestId("engine-picker-option-b"));
    expect(onChange).toHaveBeenCalledWith("b");
  });

  it("closes on Escape", async () => {
    render(<Picker<Val> value="a" options={options} onChange={vi.fn()} data-testid="engine-picker" />);
    await userEvent.click(screen.getByTestId("engine-picker"));
    expect(screen.getByTestId("engine-picker-popover")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByTestId("engine-picker-popover")).not.toBeInTheDocument();
  });

  it("shows placeholder when value is missing from options", () => {
    render(
      <Picker<Val>
        value={"z" as Val}
        options={options}
        onChange={vi.fn()}
        placeholder="Pick one"
        data-testid="engine-picker"
      />,
    );
    expect(screen.getByTestId("engine-picker")).toHaveTextContent("Pick one");
  });

  it("renders a footer inside the popover", async () => {
    render(
      <Picker<Val>
        value="a"
        options={options}
        onChange={vi.fn()}
        data-testid="engine-picker"
        footer={<button data-testid="engine-picker-add">Add</button>}
      />,
    );
    await userEvent.click(screen.getByTestId("engine-picker"));
    expect(screen.getByTestId("engine-picker-add")).toBeInTheDocument();
  });
});
