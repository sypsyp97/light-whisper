import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  getEngine: vi.fn(async () => "glm-asr"),
  setEngine: vi.fn(async () => "ok"),
  getOnlineAsrApiKey: vi.fn(async () => ""),
  setOnlineAsrApiKey: vi.fn(async () => undefined),
  getOnlineAsrEndpoint: vi.fn(async () => ({ region: "international", url: "https://api" })),
  setOnlineAsrEndpoint: vi.fn(async () => ({ region: "international", url: "https://api" })),
  getAlibabaAsrConfig: vi.fn(async () => ({
    region: "international",
    url: "https://api",
    model: "qwen-asr",
    models: ["qwen-asr"],
  })),
  setAlibabaAsrModel: vi.fn(async () => ({ model: "qwen-asr" })),
  listAlibabaAsrModels: vi.fn(async () => ({ models: ["qwen-asr", "qwen-asr-2"], source: "live" })),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { EngineSection } from "@/components/settings/EngineSection";

describe("EngineSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the section wrapper", () => {
    render(<EngineSection />);
    expect(screen.getByTestId("settings-section-engine")).toBeInTheDocument();
  });

  it("renders the engine picker", () => {
    render(<EngineSection />);
    expect(screen.getByTestId("engine-picker")).toBeInTheDocument();
  });

  it("switching engine calls setEngine", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<EngineSection />);
    await waitFor(() => expect(api.getEngine).toHaveBeenCalled());
    await user.click(screen.getByTestId("engine-picker"));
    await user.click(screen.getByTestId("engine-picker-option-alibaba-asr"));
    await waitFor(() => {
      expect(vi.mocked(api.setEngine)).toHaveBeenCalledWith("alibaba-asr");
    });
  });

  it("typing in the API key calls setOnlineAsrApiKey after 900ms", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<EngineSection />);
    const input = await screen.findByTestId("engine-api-key");
    await user.type(input, "sk-test");
    vi.advanceTimersByTime(900);
    await waitFor(() => {
      expect(vi.mocked(api.setOnlineAsrApiKey)).toHaveBeenCalled();
    });
  });
});
