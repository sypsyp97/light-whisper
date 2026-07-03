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
    },
    assistant_hotkey: null,
    assistant_system_prompt: null,
    assistant_screen_context_enabled: false,
    web_search: { enabled: false, provider: "model_native", max_results: 5 },
  })),
  setAssistantHotkey: vi.fn(async () => undefined),
  setAssistantSystemPrompt: vi.fn(async () => undefined),
  setAssistantScreenContextEnabled: vi.fn(async () => undefined),
  setAssistantApiKey: vi.fn(async () => undefined),
  getAssistantApiKey: vi.fn(async () => ""),
  setLlmProviderConfig: vi.fn(async () => undefined),
  setWebSearchConfig: vi.fn(async () => undefined),
  setWebSearchApiKey: vi.fn(async () => undefined),
  getWebSearchApiKey: vi.fn(async () => ""),
  listAiModels: vi.fn(async () => ({ models: [], sourceUrl: "x" })),
  getLlmReasoningSupport: vi.fn(async () => ({ supported: false, summary: "" })),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { AssistantSection } from "@/components/settings/AssistantSection";

describe("AssistantSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders the section wrapper", () => {
    render(<AssistantSection />);
    expect(screen.getByTestId("settings-section-assistant")).toBeInTheDocument();
  });

  it("renders the enable toggle", () => {
    render(<AssistantSection />);
    expect(screen.getByTestId("assistant-enable-toggle")).toBeInTheDocument();
  });

  it("renders the screen context toggle", async () => {
    render(<AssistantSection />);
    expect(await screen.findByTestId("assistant-screen-context-toggle")).toBeInTheDocument();
  });

  it("renders the same-provider toggle", async () => {
    render(<AssistantSection />);
    expect(await screen.findByTestId("assistant-same-provider-toggle")).toBeInTheDocument();
  });

  it("renders the websearch enable toggle", async () => {
    render(<AssistantSection />);
    expect(await screen.findByTestId("websearch-enable-toggle")).toBeInTheDocument();
  });

  it("toggling screen context calls setAssistantScreenContextEnabled", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AssistantSection />);
    await user.click(await screen.findByTestId("assistant-screen-context-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.setAssistantScreenContextEnabled)).toHaveBeenCalled();
    });
  });

  it("toggling websearch calls setWebSearchConfig", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AssistantSection />);
    await user.click(await screen.findByTestId("websearch-enable-toggle"));
    await waitFor(() => {
      expect(vi.mocked(api.setWebSearchConfig)).toHaveBeenCalled();
    });
  });

  it("normalizes the legacy custom provider id and saves typed assistant model names", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    vi.mocked(api.getUserProfile).mockResolvedValueOnce({
      hot_words: [],
      correction_patterns: [],
      vocab_frequency: {},
      total_transcriptions: 0,
      last_updated: 0,
      llm_provider: {
        active: "custom_compat",
        assistant_use_separate_model: true,
        assistant_provider: "custom_compat",
        custom_providers: [],
      },
      assistant_hotkey: null,
      assistant_system_prompt: null,
      assistant_screen_context_enabled: false,
      web_search: { enabled: false, provider: "model_native", max_results: 5 },
    });

    render(<AssistantSection />);
    await user.click(await screen.findByTestId("assistant-model-picker"));
    await user.type(screen.getByTestId("assistant-model-picker-search"), "assistant-model-x");
    await user.click(screen.getByTestId("assistant-model-picker-option-custom-value"));

    await waitFor(() => {
      const saved = vi.mocked(api.setLlmProviderConfig).mock.calls.some((call) => (
        call[0] === "custom"
        && call[6] === "assistant-model-x"
        && call[7] === "custom"
      ));
      expect(saved).toBe(true);
    });
  });
});
