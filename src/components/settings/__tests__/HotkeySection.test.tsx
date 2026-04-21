import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  registerCustomHotkey: vi.fn(async () => "ok"),
  unregisterAllHotkeys: vi.fn(async () => "ok"),
  setRecordingMode: vi.fn(async () => undefined),
  getHotkeyDiagnostic: vi.fn(async () => ({
    shortcut: "F2",
    registered: true,
    backend: "RegisterHotKey",
    isPressed: false,
  })),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

vi.mock("@/contexts/RecordingContext", async () => {
  const helper = await import("@/test/renderWithContext");
  return {
    useRecordingContext: helper.useRecordingContext,
    RecordingProvider: helper.RecordingProvider,
  };
});

import { HotkeySection } from "@/components/settings/HotkeySection";
import { renderWithRecordingContext } from "@/test/renderWithContext";
import * as api from "@/api/tauri";

describe("HotkeySection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("renders the section wrapper", () => {
    renderWithRecordingContext(<HotkeySection />);
    expect(screen.getByTestId("settings-section-hotkey")).toBeInTheDocument();
  });

  it("renders the capture button", () => {
    renderWithRecordingContext(<HotkeySection />);
    expect(screen.getByTestId("hotkey-capture-btn")).toBeInTheDocument();
  });

  it("renders the reset button", () => {
    renderWithRecordingContext(<HotkeySection />);
    expect(screen.getByTestId("hotkey-reset-btn")).toBeInTheDocument();
  });

  it("renders the recording mode segmented control", () => {
    renderWithRecordingContext(<HotkeySection />);
    expect(screen.getByTestId("recording-mode-segmented")).toBeInTheDocument();
  });

  it("changing recording mode persists to localStorage and calls setRecordingMode", async () => {
    renderWithRecordingContext(<HotkeySection />);
    await userEvent.click(screen.getByTestId("recording-mode-segmented-seg-toggle"));
    expect(localStorage.getItem("light-whisper-recording-mode")).toBe("toggle");
    expect(vi.mocked(api.setRecordingMode)).toHaveBeenCalledWith(true);
  });

  it("renders a diagnostic banner when hotkey diagnostic has a warning", () => {
    renderWithRecordingContext(<HotkeySection />, {
      hotkeyDiagnostic: {
        shortcut: "F2",
        registered: true,
        backend: "RegisterHotKey",
        isPressed: false,
        warning: "Conflict detected",
      },
    });
    expect(screen.getByTestId("hotkey-diagnostic")).toBeInTheDocument();
  });

  it("renders an error banner when hotkeyError is set", () => {
    renderWithRecordingContext(<HotkeySection />, { hotkeyError: "Registration failed" });
    expect(screen.getByTestId("hotkey-error-banner")).toBeInTheDocument();
  });
});
