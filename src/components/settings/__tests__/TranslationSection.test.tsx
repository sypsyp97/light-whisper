import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  getUserProfile: vi.fn(async () => ({
    hot_words: [],
    correction_patterns: [],
    vocab_frequency: {},
    total_transcriptions: 0,
    last_updated: 0,
    llm_provider: { active: "openai", custom_providers: [] },
    translation_target: null,
    translation_hotkey: null,
  })),
  setTranslationTarget: vi.fn(async () => false),
  setTranslationHotkey: vi.fn(async () => undefined),
  registerCustomHotkey: vi.fn(async () => "ok"),
  getAiPolishApiKey: vi.fn(async () => ""),
  setAiPolishConfig: vi.fn(async () => undefined),
  setAiPolishScreenContextEnabled: vi.fn(async () => undefined),
  setLlmProviderConfig: vi.fn(async () => undefined),
  listAiModels: vi.fn(async () => ({ models: [], sourceUrl: "x" })),
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
import { TranslationSection } from "@/components/settings/TranslationSection";
import { AiPolishSection } from "@/components/settings/AiPolishSection";
import { AI_POLISH_ENABLED_KEY } from "@/lib/constants";

describe("TranslationSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("renders the section wrapper", () => {
    render(<TranslationSection />);
    expect(screen.getByTestId("settings-section-translation")).toBeInTheDocument();
  });

  it("renders the hotkey buttons", () => {
    render(<TranslationSection />);
    expect(screen.getByTestId("translation-hotkey-btn")).toBeInTheDocument();
  });

  it("renders the target language picker", async () => {
    render(<TranslationSection />);
    expect(await screen.findByTestId("translation-target-picker")).toBeInTheDocument();
  });

  it("selecting a target language calls setTranslationTarget", async () => {
    render(<TranslationSection />);
    const picker = await screen.findByTestId("translation-target-picker");
    await userEvent.click(picker);
    const option = await screen.findByTestId("translation-target-picker-option-English");
    await userEvent.click(option);
    await waitFor(() => {
      expect(vi.mocked(api.setTranslationTarget)).toHaveBeenCalled();
    });
  });

  it("selecting a target language keeps AI polish enabled across restart", async () => {
    render(<TranslationSection />);
    const picker = await screen.findByTestId("translation-target-picker");
    await userEvent.click(picker);
    await userEvent.click(await screen.findByTestId("translation-target-picker-option-English"));

    await waitFor(() => {
      expect(localStorage.getItem(AI_POLISH_ENABLED_KEY)).toBe("true");
    });
  });

  it("rolls back the selected target language when saving fails", async () => {
    vi.mocked(api.getUserProfile).mockResolvedValueOnce({
      hot_words: [],
      correction_patterns: [],
      vocab_frequency: {},
      total_transcriptions: 0,
      last_updated: 0,
      llm_provider: { active: "openai", custom_providers: [] },
      translation_target: "Deutsch",
      translation_hotkey: null,
    });
    vi.mocked(api.setTranslationTarget).mockRejectedValueOnce(new Error("save failed"));

    render(<TranslationSection />);
    const picker = await screen.findByTestId("translation-target-picker");
    expect(picker).toHaveTextContent("Deutsch");
    await userEvent.click(picker);
    await userEvent.click(await screen.findByTestId("translation-target-picker-option-English"));

    await waitFor(() => {
      expect(screen.getByTestId("translation-target-picker")).toHaveTextContent("Deutsch");
    });
  });

  it("updates the mounted AI polish toggle when translation auto-enables polish", async () => {
    localStorage.setItem(AI_POLISH_ENABLED_KEY, "false");
    render(
      <>
        <AiPolishSection />
        <TranslationSection />
      </>,
    );
    expect(await screen.findByTestId("polish-enable-toggle")).toHaveAttribute("aria-checked", "false");

    await userEvent.click(await screen.findByTestId("translation-target-picker"));
    await userEvent.click(await screen.findByTestId("translation-target-picker-option-English"));

    await waitFor(() => {
      expect(screen.getByTestId("polish-enable-toggle")).toHaveAttribute("aria-checked", "true");
    });
  });
});
