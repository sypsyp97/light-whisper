import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  setInputMethodCommand: vi.fn(async () => undefined),
  setSoundEnabled: vi.fn(async () => undefined),
}));

import * as api from "@/api/tauri";
import { InputMethodSection } from "@/components/settings/InputMethodSection";

describe("InputMethodSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("renders the section wrapper", () => {
    render(<InputMethodSection />);
    expect(screen.getByTestId("settings-section-input")).toBeInTheDocument();
  });

  it("renders the input method segmented and sound toggle", () => {
    render(<InputMethodSection />);
    expect(screen.getByTestId("input-method-segmented")).toBeInTheDocument();
    expect(screen.getByTestId("input-sound-toggle")).toBeInTheDocument();
  });

  it("switching method to clipboard calls setInputMethodCommand", async () => {
    render(<InputMethodSection />);
    await userEvent.click(screen.getByTestId("input-method-segmented-seg-clipboard"));
    await waitFor(() => {
      expect(vi.mocked(api.setInputMethodCommand)).toHaveBeenCalledWith("clipboard");
    });
    expect(localStorage.getItem("light-whisper-input-method")).toBe("clipboard");
  });

  it("toggling sound calls setSoundEnabled", async () => {
    render(<InputMethodSection />);
    await userEvent.click(screen.getByTestId("input-sound-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.setSoundEnabled)).toHaveBeenCalled();
    });
  });
});
