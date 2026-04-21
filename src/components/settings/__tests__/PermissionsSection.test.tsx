import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  checkPermission: vi.fn(async (kind: string) => ({
    granted: kind === "microphone",
    canRequest: kind !== "microphone",
  })),
  requestPermission: vi.fn(async () => ({ granted: true, canRequest: false })),
  pasteText: vi.fn(async () => "ok"),
}));

import * as apiModule from "@/api/tauri";
import { PermissionsSection } from "@/components/settings/PermissionsSection";

// The impl agent will add `checkPermission`/`requestPermission` to tauri.ts per contract §3.20.
// Cast so the tests compile before that edit lands.
const api = apiModule as unknown as {
  checkPermission: ReturnType<typeof vi.fn>;
  requestPermission: ReturnType<typeof vi.fn>;
  pasteText: ReturnType<typeof vi.fn>;
};

describe("PermissionsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the section wrapper", () => {
    render(<PermissionsSection />);
    expect(screen.getByTestId("settings-section-permissions")).toBeInTheDocument();
  });

  it("renders a row for each of the four permissions", async () => {
    render(<PermissionsSection />);
    expect(await screen.findByTestId("perm-row-microphone")).toBeInTheDocument();
    expect(await screen.findByTestId("perm-row-accessibility")).toBeInTheDocument();
    expect(await screen.findByTestId("perm-row-screen")).toBeInTheDocument();
    expect(await screen.findByTestId("perm-row-automation")).toBeInTheDocument();
  });

  it("clicking request on accessibility calls requestPermission with accessibility", async () => {
    render(<PermissionsSection />);
    const btn = await screen.findByTestId("perm-request-accessibility");
    await userEvent.click(btn);
    await waitFor(() => {
      expect(vi.mocked(api.requestPermission)).toHaveBeenCalledWith("accessibility");
    });
  });

  it("renders the paste-test button and calls pasteText when clicked", async () => {
    render(<PermissionsSection />);
    const btn = screen.getByTestId("perm-paste-test-btn");
    await userEvent.click(btn);
    await waitFor(() => {
      expect(vi.mocked(api.pasteText)).toHaveBeenCalledWith("ok", "clipboard");
    });
  });

  it("shows granted badge for microphone row", async () => {
    render(<PermissionsSection />);
    expect(await screen.findByTestId("perm-status-microphone")).toBeInTheDocument();
  });
});
