import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  exportUserProfile: vi.fn(async () => JSON.stringify({ hello: "world" })),
  importUserProfile: vi.fn(async () => undefined),
}));

import * as api from "@/api/tauri";
import { DataSection } from "@/components/settings/DataSection";

describe("DataSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the section wrapper", () => {
    render(<DataSection />);
    expect(screen.getByTestId("settings-section-data")).toBeInTheDocument();
  });

  it("renders export and import buttons", () => {
    render(<DataSection />);
    expect(screen.getByTestId("data-export-btn")).toBeInTheDocument();
    expect(screen.getByTestId("data-import-btn")).toBeInTheDocument();
  });

  it("clicking export calls exportUserProfile", async () => {
    render(<DataSection />);
    // Stub URL.createObjectURL for jsdom
    const createObjectURL = vi.fn(() => "blob:mock");
    const revokeObjectURL = vi.fn();
    (window.URL as unknown as { createObjectURL: typeof createObjectURL }).createObjectURL = createObjectURL;
    (window.URL as unknown as { revokeObjectURL: typeof revokeObjectURL }).revokeObjectURL = revokeObjectURL;
    await userEvent.click(screen.getByTestId("data-export-btn"));
    await waitFor(() => {
      expect(vi.mocked(api.exportUserProfile)).toHaveBeenCalled();
    });
  });

  it("renders the hidden import input", () => {
    render(<DataSection />);
    expect(screen.getByTestId("data-import-input")).toBeInTheDocument();
  });
});
