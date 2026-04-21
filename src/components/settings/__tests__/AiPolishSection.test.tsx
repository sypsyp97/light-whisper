import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  getUserProfile: vi.fn(async () => ({
    hot_words: [],
    correction_patterns: [],
    vocab_frequency: {},
    total_transcriptions: 0,
    last_updated: 0,
    llm_provider: {
      active: "openai",
      custom_providers: [],
      openai_auth_mode: "api_key",
    },
  })),
  getAiPolishApiKey: vi.fn(async () => ""),
  setAiPolishConfig: vi.fn(async () => undefined),
  setAiPolishScreenContextEnabled: vi.fn(async () => undefined),
  setLlmProviderConfig: vi.fn(async () => undefined),
  listAiModels: vi.fn(async () => ({ models: [{ id: "gpt-4o" }], sourceUrl: "x" })),
  getLlmReasoningSupport: vi.fn(async () => ({ supported: false, summary: "" })),
  addCustomProvider: vi.fn(async () => "provider-id"),
  setCustomPrompt: vi.fn(async () => undefined),
  setOpenaiFastMode: vi.fn(async () => undefined),
  getOpenaiCodexOauthStatus: vi.fn(async () => ({ loggedIn: false })),
  loginOpenaiCodexOauth: vi.fn(async () => ({ loggedIn: true })),
  logoutOpenaiCodexOauth: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { AiPolishSection } from "@/components/settings/AiPolishSection";

describe("AiPolishSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    localStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the section wrapper", () => {
    render(<AiPolishSection />);
    expect(screen.getByTestId("settings-section-ai-polish")).toBeInTheDocument();
  });

  it("renders the master toggle and provider picker", async () => {
    render(<AiPolishSection />);
    expect(screen.getByTestId("polish-enable-toggle")).toBeInTheDocument();
    expect(await screen.findByTestId("polish-provider-picker")).toBeInTheDocument();
  });

  it("toggling enable calls setAiPolishConfig", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AiPolishSection />);
    await user.click(screen.getByTestId("polish-enable-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.setAiPolishConfig)).toHaveBeenCalled();
    });
  });

  it("toggling screen context calls setAiPolishScreenContextEnabled", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AiPolishSection />);
    await user.click(screen.getByTestId("polish-screen-context-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.setAiPolishScreenContextEnabled)).toHaveBeenCalled();
    });
  });

  it("renders the API key secret input", async () => {
    render(<AiPolishSection />);
    expect(await screen.findByTestId("polish-api-key")).toBeInTheDocument();
  });

  it("renders custom prompt textarea", async () => {
    render(<AiPolishSection />);
    expect(await screen.findByTestId("polish-custom-prompt")).toBeInTheDocument();
  });
});
