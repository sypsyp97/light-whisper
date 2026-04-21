import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  checkAppUpdate: vi.fn(async () => ({
    available: false,
    currentVersion: "1.3.3",
    latestVersion: "1.3.3",
  })),
  openAppReleasePage: vi.fn(async () => "ok"),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "1.3.3"),
}));

import * as api from "@/api/tauri";
import { UpdateSection } from "@/components/settings/UpdateSection";

describe("UpdateSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the section wrapper", () => {
    render(<UpdateSection />);
    expect(screen.getByTestId("settings-section-update")).toBeInTheDocument();
  });

  it("renders the current version badge", async () => {
    render(<UpdateSection />);
    expect(await screen.findByTestId("update-current-version")).toBeInTheDocument();
  });

  it("clicking check calls checkAppUpdate", async () => {
    render(<UpdateSection />);
    await userEvent.click(screen.getByTestId("update-check-btn"));
    await waitFor(() => {
      expect(vi.mocked(api.checkAppUpdate)).toHaveBeenCalled();
    });
  });
});
