import { describe, it, expect, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { AppearanceSection } from "@/components/settings/AppearanceSection";

describe("AppearanceSection", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("renders the section wrapper", () => {
    render(<AppearanceSection />);
    expect(screen.getByTestId("settings-section-appearance")).toBeInTheDocument();
  });

  it("renders the theme segmented control", () => {
    render(<AppearanceSection />);
    expect(screen.getByTestId("appearance-theme")).toBeInTheDocument();
  });

  it("renders the language picker", () => {
    render(<AppearanceSection />);
    expect(screen.getByTestId("appearance-language")).toBeInTheDocument();
  });

  it("switches theme segment when clicked", async () => {
    render(<AppearanceSection />);
    await userEvent.click(screen.getByTestId("appearance-theme-seg-dark"));
    // After click, localStorage should have the theme key updated by useTheme.
    expect(localStorage.getItem("light-whisper-theme")).toBe("dark");
  });
});
