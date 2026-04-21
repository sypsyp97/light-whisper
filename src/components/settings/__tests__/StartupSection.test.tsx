import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  isAutostartEnabled: vi.fn(async () => false),
  enableAutostart: vi.fn(async () => undefined),
  disableAutostart: vi.fn(async () => undefined),
}));

import * as api from "@/api/tauri";
import { StartupSection } from "@/components/settings/StartupSection";

describe("StartupSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the section wrapper", () => {
    render(<StartupSection />);
    expect(screen.getByTestId("settings-section-startup")).toBeInTheDocument();
  });

  it("renders the autostart toggle", async () => {
    render(<StartupSection />);
    expect(await screen.findByTestId("autostart-toggle")).toBeInTheDocument();
  });

  it("toggling calls enableAutostart when currently disabled", async () => {
    render(<StartupSection />);
    const toggle = await screen.findByTestId("autostart-toggle");
    await userEvent.click(toggle);
    await waitFor(() => {
      expect(vi.mocked(api.enableAutostart)).toHaveBeenCalled();
    });
  });
});
