import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/api/tauri", () => ({
  getUserProfile: vi.fn(async () => ({
    hot_words: [
      { text: "Claude", weight: 5, source: "user", use_count: 0, last_used: 0 },
    ],
    correction_patterns: [
      { original: "clod", corrected: "Claude", count: 1, last_seen: 0, source: "user" },
    ],
    vocab_frequency: {},
    total_transcriptions: 3,
    last_updated: 0,
    llm_provider: { active: "openai", custom_providers: [] },
  })),
  addHotWord: vi.fn(async () => undefined),
  removeHotWord: vi.fn(async () => undefined),
  removeCorrection: vi.fn(async () => undefined),
  validateCorrections: vi.fn(async () => 2),
  setCorrectionValidationConfig: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { VocabularySection } from "@/components/settings/VocabularySection";

describe("VocabularySection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the section wrapper", () => {
    render(<VocabularySection />);
    expect(screen.getByTestId("settings-section-vocabulary")).toBeInTheDocument();
  });

  it("renders the hot-word add input and add button", () => {
    render(<VocabularySection />);
    expect(screen.getByTestId("hot-word-input")).toBeInTheDocument();
    expect(screen.getByTestId("hot-word-add-btn")).toBeInTheDocument();
  });

  it("typing and clicking add calls addHotWord with weight 5", async () => {
    render(<VocabularySection />);
    await userEvent.type(screen.getByTestId("hot-word-input"), "NewWord");
    await userEvent.click(screen.getByTestId("hot-word-add-btn"));
    await waitFor(() => {
      expect(vi.mocked(api.addHotWord)).toHaveBeenCalledWith("NewWord", 5);
    });
  });

  it("opens correction rules modal when the manage button is clicked", async () => {
    render(<VocabularySection />);
    await userEvent.click(screen.getByTestId("correction-rules-btn"));
    expect(await screen.findByTestId("modal-correction-rules")).toBeInTheDocument();
  });

  it("running validation calls validateCorrections", async () => {
    render(<VocabularySection />);
    await userEvent.click(screen.getByTestId("correction-rules-btn"));
    const validateBtn = await screen.findByTestId("correction-validate-btn");
    await userEvent.click(validateBtn);
    await waitFor(() => {
      expect(vi.mocked(api.validateCorrections)).toHaveBeenCalled();
    });
  });
});
