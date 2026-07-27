import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

import PolishStructureControl from "@/components/settings/PolishStructureControl";

describe("PolishStructureControl", () => {
  it("shows the saved level and its behavior boundary", () => {
    render(<PolishStructureControl level="balanced" onChange={vi.fn()} />);

    expect(screen.getByRole("slider", { name: "settings.structureLevel" }))
      .toHaveAttribute("aria-valuetext", "settings.structureLevelBalanced");
    expect(screen.getByText("settings.structureLevelBalancedDesc")).toBeInTheDocument();
  });

  it("maps the strongest slider position to strong structuring", () => {
    const onChange = vi.fn();
    render(<PolishStructureControl level="off" onChange={onChange} />);

    fireEvent.change(screen.getByRole("slider", { name: "settings.structureLevel" }), {
      target: { value: "3" },
    });

    expect(onChange).toHaveBeenCalledWith("strong");
  });
});
