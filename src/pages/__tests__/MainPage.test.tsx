import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  copyToClipboard: vi.fn(async () => "ok"),
  submitUserCorrection: vi.fn(async () => undefined),
  hideMainWindow: vi.fn(async () => "ok"),
  openPermissionSettings: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    hide: vi.fn(),
  }),
}));

vi.mock("@/contexts/RecordingContext", async () => {
  const helper = await import("@/test/renderWithContext");
  return {
    useRecordingContext: helper.useRecordingContext,
    RecordingProvider: helper.RecordingProvider,
  };
});

import { renderWithRecordingContext } from "@/test/renderWithContext";
import MainPage from "@/pages/MainPage";

const onNavigate = vi.fn();

describe("MainPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("renders the main-page root", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />);
    expect(screen.getByTestId("main-page")).toBeInTheDocument();
  });

  it("renders the record button", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />);
    expect(screen.getByTestId("main-record-btn")).toBeInTheDocument();
  });

  it("renders the status indicator", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />);
    expect(screen.getByTestId("main-status")).toBeInTheDocument();
  });

  it("clicking the settings left action navigates to settings", async () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />);
    await userEvent.click(screen.getByTestId("titlebar-left-action"));
    expect(onNavigate).toHaveBeenCalledWith("settings");
  });

  it("shows an error banner when recordingError is set", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, {
      recordingError: "Boom",
    });
    expect(screen.getByTestId("main-error-banner")).toBeInTheDocument();
  });

  it("shows the onboarding hint when no history and not dismissed", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, { history: [] });
    expect(screen.getByTestId("main-onboarding")).toBeInTheDocument();
  });

  it("hides onboarding when localStorage flag is set", () => {
    localStorage.setItem("light-whisper-onboarding-dismissed", "true");
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, { history: [] });
    expect(screen.queryByTestId("main-onboarding")).not.toBeInTheDocument();
  });

  it("shows transcription result when transcriptionResult is non-null", () => {
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, {
      transcriptionResult: "hello",
      durationSec: 3,
      charCount: 5,
    });
    expect(screen.getByTestId("main-result")).toBeInTheDocument();
  });

  it("shows an Open Settings button when the recording error is a PermissionDenied", async () => {
    // The whole point of the structured-error refactor: a permission denial
    // must surface a one-click deeplink, not a multi-line opaque toast.
    const apiMod = await import("@/api/tauri");
    const openSpy = vi.mocked(
      (apiMod as unknown as { openPermissionSettings: ReturnType<typeof vi.fn> })
        .openPermissionSettings,
    );
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, {
      recordingError: "需要「麦克风」权限",
      recordingErrorPermission: {
        kind: "microphone",
        settingsUrl:
          "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
      },
    });
    const btn = screen.getByTestId("main-perm-open-settings-btn");
    await userEvent.click(btn);
    expect(openSpy).toHaveBeenCalledWith("microphone");
  });

  it("falls back to the retry button when the error is a model error (not permission)", () => {
    // Permission and model errors must NOT both grab the action slot — the
    // permission deeplink only takes priority when there's actually a
    // structured permission denial.
    renderWithRecordingContext(<MainPage onNavigate={onNavigate} animClass="" />, {
      modelError: "model boom",
    });
    expect(screen.getByTestId("main-retry-btn")).toBeInTheDocument();
    expect(
      screen.queryByTestId("main-perm-open-settings-btn"),
    ).not.toBeInTheDocument();
  });
});
