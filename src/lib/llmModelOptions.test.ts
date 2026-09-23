import { describe, expect, it } from "vitest";

import { findLlmPreset, isFixedPresetProvider, llmProviderOptions } from "./llmModelOptions";

describe("DeepSeek model options", () => {
  it("offers only the currently supported built-in models for new configurations", () => {
    const deepseek = llmProviderOptions.find((option) => option.key === "deepseek");

    expect(deepseek?.models).toEqual(["deepseek-v4-flash", "deepseek-v4-pro"]);
  });
});

describe("xAI Grok provider preset", () => {
  it("registers a built-in provider with key xai", () => {
    const xai = llmProviderOptions.find((option) => option.key === "xai");

    expect(xai).toEqual({
      key: "xai",
      label: "xAI Grok",
      descKey: "settings.xaiDesc",
      baseUrl: "https://api.x.ai",
      defaultModel: "grok-4.6",
      models: ["grok-4.6", "grok-4.5", "grok-build-0.1", "grok-composer-2.5-fast"],
    });
    expect(isFixedPresetProvider("xai")).toBe(true);
    expect(findLlmPreset("xai")).toEqual(xai);
  });

  it("does not replace the existing OpenAI or DeepSeek presets", () => {
    expect(llmProviderOptions.find((option) => option.key === "openai")).toMatchObject({
      key: "openai",
      baseUrl: "https://api.openai.com",
    });
    expect(llmProviderOptions.find((option) => option.key === "deepseek")).toMatchObject({
      key: "deepseek",
      baseUrl: "https://api.deepseek.com",
    });
  });
});

describe("OpenAI provider preset", () => {
  it("keeps the existing default and gets model choices from the live catalog", () => {
    const openai = findLlmPreset("openai");

    expect(openai.defaultModel).toBe("gpt-4.1-mini");
    expect(openai.models).toEqual([]);
    expect(isFixedPresetProvider("openai")).toBe(true);
  });
});

