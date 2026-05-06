import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  checkPermission: vi.fn(async (kind: string) => ({
    granted: kind === "microphone",
    canRequest: kind !== "microphone",
  })),
  requestPermission: vi.fn(async () => ({ granted: true, canRequest: false })),
  openPermissionSettings: vi.fn(async () => undefined),
  pasteText: vi.fn(async () => "ok"),
}));

import * as apiModule from "@/api/tauri";
import { PermissionsSection } from "@/components/settings/PermissionsSection";

// The impl agent will add `checkPermission`/`requestPermission` to tauri.ts per contract §3.20.
// Cast so the tests compile before that edit lands.
const api = apiModule as unknown as {
  checkPermission: ReturnType<typeof vi.fn>;
  requestPermission: ReturnType<typeof vi.fn>;
  openPermissionSettings: ReturnType<typeof vi.fn>;
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

  it("clicking 'Open Settings' on a denied row invokes openPermissionSettings(kind)", async () => {
    // The user complaint was that they can't unblock themselves from a
    // permission error. The fix is a one-click deeplink straight into the
    // matching System Settings pane — every denied row exposes one.
    render(<PermissionsSection />);
    const btn = await screen.findByTestId("perm-open-settings-screen");
    await userEvent.click(btn);
    await waitFor(() => {
      expect(vi.mocked(api.openPermissionSettings)).toHaveBeenCalledWith("screen");
    });
  });

  it("granted rows expose a re-check button (not a Request button)", async () => {
    // Once a permission is granted, the prominent CTA must NOT keep saying
    // "Request"/"Settings" — that's confusing and tempts users to revoke
    // themselves. Instead it becomes a quiet "Re-check" affordance.
    render(<PermissionsSection />);
    expect(
      await screen.findByTestId("perm-recheck-microphone"),
    ).toBeInTheDocument();
    // And there is NO "Open Settings" button on a granted row — opening
    // settings would only let the user revoke; we don't surface that path.
    expect(
      screen.queryByTestId("perm-open-settings-microphone"),
    ).not.toBeInTheDocument();
  });

  it("re-check button refreshes the row's status via checkPermission", async () => {
    render(<PermissionsSection />);
    const btn = await screen.findByTestId("perm-recheck-microphone");
    vi.mocked(api.checkPermission).mockClear();
    await userEvent.click(btn);
    await waitFor(() => {
      expect(vi.mocked(api.checkPermission)).toHaveBeenCalledWith("microphone");
    });
  });

  it("section-level 'Re-check all' refreshes every permission", async () => {
    render(<PermissionsSection />);
    const btn = await screen.findByTestId("perm-recheck-all-btn");
    vi.mocked(api.checkPermission).mockClear();
    await userEvent.click(btn);
    await waitFor(() => {
      const calls = vi
        .mocked(api.checkPermission)
        .mock.calls.map((c) => c[0]);
      expect(calls).toEqual(
        expect.arrayContaining([
          "microphone",
          "accessibility",
          "screen",
          "automation",
        ]),
      );
    });
  });
});
