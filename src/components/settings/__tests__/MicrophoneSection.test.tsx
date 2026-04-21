import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  listInputDevices: vi.fn(async () => ({
    devices: [
      { name: "MacBook Pro Microphone", isDefault: true },
      { name: "External USB", isDefault: false },
    ],
    selectedDeviceName: null,
  })),
  setInputDevice: vi.fn(async () => undefined),
  testMicrophone: vi.fn(async () => "ok"),
  startMicrophoneLevelMonitor: vi.fn(async () => "ok"),
  stopMicrophoneLevelMonitor: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { MicrophoneSection } from "@/components/settings/MicrophoneSection";

describe("MicrophoneSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("renders the section wrapper", () => {
    render(<MicrophoneSection />);
    expect(screen.getByTestId("settings-section-microphone")).toBeInTheDocument();
  });

  it("renders device picker, refresh, test and level monitor controls", async () => {
    render(<MicrophoneSection />);
    expect(await screen.findByTestId("mic-device-picker")).toBeInTheDocument();
    expect(screen.getByTestId("mic-refresh")).toBeInTheDocument();
    expect(screen.getByTestId("mic-test")).toBeInTheDocument();
    expect(screen.getByTestId("mic-level-monitor-toggle")).toBeInTheDocument();
  });

  it("calls testMicrophone when the test button is clicked", async () => {
    render(<MicrophoneSection />);
    await userEvent.click(screen.getByTestId("mic-test"));
    await waitFor(() => expect(vi.mocked(api.testMicrophone)).toHaveBeenCalled());
  });

  it("selecting a device calls setInputDevice with the device name", async () => {
    render(<MicrophoneSection />);
    const picker = await screen.findByTestId("mic-device-picker");
    await userEvent.click(picker);
    await userEvent.click(await screen.findByTestId("mic-device-picker-option-External USB"));
    await waitFor(() => {
      expect(vi.mocked(api.setInputDevice)).toHaveBeenCalledWith("External USB");
    });
  });

  it("toggling level monitor starts the backend monitor", async () => {
    render(<MicrophoneSection />);
    await userEvent.click(screen.getByTestId("mic-level-monitor-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.startMicrophoneLevelMonitor)).toHaveBeenCalled();
    });
  });
});
