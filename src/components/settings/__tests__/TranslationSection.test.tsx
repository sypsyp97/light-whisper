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
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { TranslationSection } from "@/components/settings/TranslationSection";

describe("TranslationSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
});
