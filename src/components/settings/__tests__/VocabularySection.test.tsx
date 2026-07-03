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
  setLlmProviderConfig: vi.fn(async () => undefined),
}));

vi.mock("@tauri-apps/api/event", async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController().module;
});

import * as api from "@/api/tauri";
import { VocabularySection } from "@/components/settings/VocabularySection";

async function openCorrectionRules() {
  const button = screen.getByTestId("correction-rules-btn");
  await waitFor(() => expect(button).not.toBeDisabled());
  await userEvent.click(button);
  return screen.findByTestId("modal-correction-rules");
}

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
    expect(await openCorrectionRules()).toBeInTheDocument();
  });

  it("running validation calls validateCorrections", async () => {
    render(<VocabularySection />);
    await openCorrectionRules();
    const validateBtn = await screen.findByTestId("correction-validate-btn");
    await userEvent.click(validateBtn);
    await waitFor(() => {
      expect(vi.mocked(api.validateCorrections)).toHaveBeenCalled();
    });
  });

  it("toggling validation separate model does not mutate the main polish provider", async () => {
    render(<VocabularySection />);
    await openCorrectionRules();
    await userEvent.click(await screen.findByTestId("correction-validation-separate-toggle"));

    await waitFor(() => {
      expect(vi.mocked(api.setCorrectionValidationConfig)).toHaveBeenCalledWith(expect.objectContaining({
        useSeparateModel: true,
      }));
    });
    expect(vi.mocked(api.setLlmProviderConfig)).not.toHaveBeenCalled();
  });

  it("shows named custom providers in correction validation provider picker", async () => {
    vi.mocked(api.getUserProfile).mockResolvedValueOnce({
      hot_words: [],
      correction_patterns: [],
      vocab_frequency: {},
      total_transcriptions: 0,
      last_updated: 0,
      correction_validation_enabled: true,
      llm_provider: {
        active: "openai",
        validation_use_separate_model: true,
        validation_provider: "provider-id",
        validation_model: "validator-model",
        custom_providers: [{
          id: "provider-id",
          name: "OpenRouter",
          base_url: "https://openrouter.ai/api/v1",
          model: "openai/gpt-4o-mini",
          api_format: "openai_compat",
        }],
      },
    });

    render(<VocabularySection />);
    await openCorrectionRules();
    const picker = await screen.findByTestId("correction-validation-provider");
    expect(picker).toHaveTextContent("OpenRouter");
    await userEvent.click(picker);
    expect(await screen.findByTestId("correction-validation-provider-option-provider-id")).toHaveTextContent("OpenRouter");
  });

  it("saves validation provider and model before running validation", async () => {
    vi.mocked(api.getUserProfile).mockResolvedValueOnce({
      hot_words: [],
      correction_patterns: [
        { original: "teh", corrected: "the", count: 1, last_seen: 0, source: "ai" },
      ],
      vocab_frequency: {},
      total_transcriptions: 0,
      last_updated: 0,
      correction_validation_enabled: true,
      llm_provider: {
        active: "openai",
        validation_use_separate_model: true,
        validation_provider: "openai",
        validation_model: "validator-v1",
        custom_providers: [],
      },
    });

    render(<VocabularySection />);
    await openCorrectionRules();
    const providerPicker = await screen.findByTestId("correction-validation-provider");
    await userEvent.click(providerPicker);
    await userEvent.click(await screen.findByTestId("correction-validation-provider-option-deepseek"));
    const modelInput = await screen.findByTestId("correction-validation-model");
    await userEvent.clear(modelInput);
    await userEvent.type(modelInput, "validator-v2");
    await userEvent.click(await screen.findByTestId("correction-validate-btn"));

    await waitFor(() => {
      expect(vi.mocked(api.validateCorrections)).toHaveBeenCalled();
    });

    const saveCalls = vi.mocked(api.setCorrectionValidationConfig).mock.calls;
    const savedIndex = saveCalls.findIndex(([params]) => (
      params.provider === "deepseek" && params.model === "validator-v2"
    ));
    expect(savedIndex).toBeGreaterThanOrEqual(0);
    expect(vi.mocked(api.setCorrectionValidationConfig).mock.invocationCallOrder[savedIndex])
      .toBeLessThan(vi.mocked(api.validateCorrections).mock.invocationCallOrder[0]);
  });

  it("does not open or save correction validation defaults before profile loads", async () => {
    vi.mocked(api.getUserProfile).mockReturnValueOnce(new Promise(() => {}) as ReturnType<typeof api.getUserProfile>);

    render(<VocabularySection />);
    const button = screen.getByTestId("correction-rules-btn");
    expect(button).toBeDisabled();
    await userEvent.click(button);

    expect(screen.queryByTestId("modal-correction-rules")).not.toBeInTheDocument();
    expect(vi.mocked(api.setCorrectionValidationConfig)).not.toHaveBeenCalled();
  });
});
