import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import TranslationSettingsSection from "@/components/settings/TranslationSettingsSection";

function renderSection(onSelectTarget = vi.fn().mockResolvedValue(undefined)) {
  render(
    <TranslationSettingsSection
      target="English"
      hotkeyDisplay="Ctrl+Alt+T"
      hotkeyCapture={{
        capturing: false,
        saving: false,
        startCapture: vi.fn(),
        cancelCapture: vi.fn(),
      }}
      onClearHotkey={vi.fn()}
      onSelectTarget={onSelectTarget}
    />,
  );
  return onSelectTarget;
}

describe("TranslationSettingsSection language picker", () => {
  it("uses the shared popup and can disable translation", () => {
    const onSelectTarget = renderSection();

    fireEvent.click(screen.getByRole("button", {
      name: "settings.translationTargetLanguage",
    }));
    fireEvent.click(screen.getByRole("option", { name: "settings.off" }));

    expect(onSelectTarget).toHaveBeenCalledWith(null);
  });

  it("accepts a custom language from the popup", () => {
    const onSelectTarget = renderSection();

    fireEvent.click(screen.getByRole("button", {
      name: "settings.translationTargetLanguage",
    }));
    fireEvent.click(screen.getByText("settings.customLang"));
    const input = screen.getByRole("textbox", { name: "settings.customLangLabel" });
    fireEvent.change(input, { target: { value: "Italiano" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onSelectTarget).toHaveBeenCalledWith("Italiano");
  });
});
