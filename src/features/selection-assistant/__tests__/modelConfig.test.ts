import { describe, expect, it } from "vitest";

import { resolveSelectionModelConfig } from "../modelConfig";

const baseConfig = {
  active: "deepseek",
  reasoning_mode: "balanced" as const,
  polish_reasoning_mode: "light" as const,
  custom_providers: [
    {
      id: "custom-vision",
      name: "Vision",
      base_url: "https://example.test/v1",
      model: "vision-model",
      api_format: "openai_compat" as const,
    },
  ],
};

describe("resolveSelectionModelConfig", () => {
  it("follows the AI polish provider and reasoning mode by default", () => {
    expect(resolveSelectionModelConfig(baseConfig)).toEqual({
      provider: "deepseek",
      model: undefined,
      reasoningMode: "light",
      followsPolish: true,
    });
  });

  it("uses an independently configured selection provider and model", () => {
    expect(
      resolveSelectionModelConfig({
        ...baseConfig,
        selection_use_separate_model: true,
        selection_provider: "custom-vision",
        selection_model: "vision-model-v2",
        selection_reasoning_mode: "off",
      }),
    ).toEqual({
      provider: "custom-vision",
      model: "vision-model-v2",
      reasoningMode: "off",
      followsPolish: false,
    });
  });

  it("accepts OpenAI as an independent provider for the Codex route", () => {
    expect(
      resolveSelectionModelConfig({
        ...baseConfig,
        selection_use_separate_model: true,
        selection_provider: "openai",
        selection_model: "gpt-5.5",
      }),
    ).toMatchObject({
      provider: "openai",
      model: "gpt-5.5",
      followsPolish: false,
    });
  });

  it("falls back atomically when the saved independent provider no longer exists", () => {
    expect(
      resolveSelectionModelConfig({
        ...baseConfig,
        selection_use_separate_model: true,
        selection_provider: "removed-provider",
        selection_model: "stale-model",
        selection_reasoning_mode: "deep",
      }),
    ).toEqual({
      provider: "deepseek",
      model: undefined,
      reasoningMode: "light",
      followsPolish: true,
    });
  });

  it("falls back atomically when the independent model is blank", () => {
    expect(
      resolveSelectionModelConfig({
        ...baseConfig,
        selection_use_separate_model: true,
        selection_provider: "custom-vision",
        selection_model: "  ",
        selection_reasoning_mode: "deep",
      }),
    ).toEqual({
      provider: "deepseek",
      model: undefined,
      reasoningMode: "light",
      followsPolish: true,
    });
  });

  it("inherits the legacy reasoning mode when no polish-specific mode exists", () => {
    expect(
      resolveSelectionModelConfig({
        ...baseConfig,
        polish_reasoning_mode: undefined,
      }).reasoningMode,
    ).toBe("balanced");
  });
});
