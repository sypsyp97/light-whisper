import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useExclusivePicker } from "@/hooks/useExclusivePicker";

function PickerHarness() {
  const picker = useExclusivePicker<"engine">();
  return (
    <div ref={picker.setRef("engine")}>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={picker.isExpanded("engine")}
        aria-label="Engine"
        onClick={() => picker.toggle("engine")}
      >
        Choose
      </button>
      {picker.isOpen("engine") && (
        <div className={picker.popoverClass("engine")}>
          <div className="picker-list" role="listbox">
            <button className="picker-option" data-active="false">Alpha</button>
            <button className="picker-option" data-active="true">Beta</button>
            <button className="picker-option" data-active="false">Gamma</button>
          </div>
        </div>
      )}
    </div>
  );
}

describe("useExclusivePicker accessibility", () => {
  it("adds option semantics and supports arrow, Home, End, typeahead and Escape", async () => {
    const user = userEvent.setup();
    render(<PickerHarness />);

    const trigger = screen.getByRole("button", { name: "Engine" });
    await user.click(trigger);

    const listbox = await screen.findByRole("listbox", { name: "Engine" });
    const options = await screen.findAllByRole("option");
    expect(trigger).toHaveAttribute("aria-controls", listbox.id);
    expect(options[1]).toHaveAttribute("aria-selected", "true");
    await waitFor(() => expect(options[1]).toHaveFocus());

    await user.keyboard("{ArrowDown}");
    expect(options[2]).toHaveFocus();
    await user.keyboard("{Home}");
    expect(options[0]).toHaveFocus();
    await user.keyboard("g");
    expect(options[2]).toHaveFocus();
    await user.keyboard("{End}{Escape}");

    await waitFor(() => expect(trigger).toHaveFocus());
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
