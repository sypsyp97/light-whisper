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
import type { UserProfile } from "@/types";

const baseProfile = {
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
} satisfies UserProfile;

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

  it("loads the active custom provider base URL and model", async () => {
    vi.mocked(api.getUserProfile).mockResolvedValueOnce({
      ...baseProfile,
      llm_provider: {
        ...baseProfile.llm_provider,
        active: "provider-id",
        custom_providers: [{
          id: "provider-id",
          name: "OpenRouter",
          base_url: "https://openrouter.ai/api/v1",
          model: "openai/gpt-4o-mini",
          api_format: "openai_compat",
        }],
      },
    });

    render(<AiPolishSection />);

    expect(await screen.findByTestId("polish-base-url")).toHaveValue("https://openrouter.ai/api/v1");
    await waitFor(() => {
      expect(screen.getByTestId("polish-model-picker")).toHaveTextContent("openai/gpt-4o-mini");
    });
  });

  it("selects and saves a newly added custom provider", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AiPolishSection />);

    await user.click(await screen.findByTestId("polish-provider-picker"));
    await user.click(screen.getByRole("button", { name: /settings\.addCustomProvider|添加自定义服务商|Add Custom Provider/ }));
    await user.type(screen.getByTestId("custom-provider-name"), "OpenRouter");
    await user.type(screen.getByTestId("custom-provider-base-url"), "https://openrouter.ai/api/v1");
    await user.type(screen.getByTestId("custom-provider-model"), "openai/gpt-4o-mini");
    await user.click(screen.getByTestId("custom-provider-submit"));

    await waitFor(() => {
      expect(vi.mocked(api.addCustomProvider)).toHaveBeenCalledWith(
        "OpenRouter",
        "https://openrouter.ai/api/v1",
        "openai/gpt-4o-mini",
        "openai_compat",
      );
    });
    await waitFor(() => {
      const saved = vi.mocked(api.setLlmProviderConfig).mock.calls.some((call) => (
        call[0] === "provider-id"
        && call[1] === "https://openrouter.ai/api/v1"
        && call[2] === "openai/gpt-4o-mini"
      ));
      expect(saved).toBe(true);
    });
    expect(screen.getByTestId("polish-provider-picker")).toHaveTextContent("OpenRouter");
  });

  it("saves a manually typed model name from the picker", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AiPolishSection />);

    await user.click(await screen.findByTestId("polish-model-picker"));
    await user.type(screen.getByTestId("polish-model-picker-search"), "o3-mini");
    await user.click(screen.getByTestId("polish-model-picker-option-custom-value"));

    await waitFor(() => {
      const saved = vi.mocked(api.setLlmProviderConfig).mock.calls.some((call) => call[2] === "o3-mini");
      expect(saved).toBe(true);
    });
    expect(screen.getByTestId("polish-model-picker")).toHaveTextContent("o3-mini");
  });

  it("flushes pending API key saves to the provider that was active while typing", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<AiPolishSection />);

    await user.type(await screen.findByTestId("polish-api-key"), "sk-openai");
    await user.click(screen.getByTestId("polish-provider-picker"));
    await user.click(await screen.findByTestId("polish-provider-picker-option-deepseek"));

    await waitFor(() => {
      expect(vi.mocked(api.setAiPolishConfig)).toHaveBeenCalledWith(false, "sk-openai", "openai");
    });
  });
});
