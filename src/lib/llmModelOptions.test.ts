import { describe, expect, it } from "vitest";

import { findLlmPreset, isFixedPresetProvider, llmProviderOptions, resolveLlmBaseUrl, resolveLlmModel } from "./llmModelOptions";

describe("DeepSeek model options", () => {
  it("defaults new configurations to canonical Flash and retains the previous model choices", () => {
    const deepseek = llmProviderOptions.find((option) => option.key === "deepseek");

    expect(deepseek?.models).toEqual(["deepseek-flash", "deepseek-v4-flash", "deepseek-v4-pro"]);
    expect(deepseek?.defaultModel).toBe("deepseek-flash");
    expect(resolveLlmModel("deepseek")).toBe("deepseek-flash");
  });

  it("preserves explicitly saved DeepSeek model choices", () => {
    for (const model of ["deepseek-v4-flash", "deepseek-v4-pro", "deepseek-chat", "deepseek-reasoner", "deepseek-future"]) {
      expect(resolveLlmModel("deepseek", model)).toBe(model);
    }
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
      defaultModel: "grok-4.7",
      models: ["grok-4.7", "grok-4.6", "grok-4.5", "grok-build-0.1", "grok-composer-2.5-fast"],
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
  it("defaults to Luna and gets model choices from the live catalog", () => {
    const openai = findLlmPreset("openai");

    expect(openai.defaultModel).toBe("gpt-6-luna");
    expect(openai.models).toEqual([]);
    expect(isFixedPresetProvider("openai")).toBe(true);
  });
});

describe("OpenRouter provider preset", () => {
  it("registers its built-in endpoint and default model alias", () => {
    const openrouter = findLlmPreset("openrouter");

    expect(isFixedPresetProvider("openrouter")).toBe(true);
    expect(openrouter.baseUrl).toBe("https://openrouter.ai/api");
    expect(openrouter.defaultModel).toBe("~deepseek/deepseek-flash-latest");
    expect(openrouter.models).toContain(openrouter.defaultModel);
    expect(resolveLlmBaseUrl("openrouter", "https://stale-custom.example")).toBe(openrouter.baseUrl);
    expect(resolveLlmModel("openrouter")).toBe(openrouter.defaultModel);
  });

  it("preserves explicitly saved upstream model choices", () => {
    for (const model of ["anthropic/claude-sonnet-4-5", "openai/gpt-5-mini", "future-provider/future-model"]) {
      expect(resolveLlmModel("openrouter", model)).toBe(model);
    }
  });
});

describe("current provider model defaults", () => {
  it.each([
    ["cerebras", "qwen-3.8-27b", ["qwen-3.8-27b", "gpt-oss-120b"]],
    ["siliconflow", "Qwen/Qwen3.8-27B", ["Qwen/Qwen3.8-27B", "deepseek-ai/DeepSeek-V4.1-Flash", "deepseek-ai/DeepSeek-V4-Pro"]],
  ] as const)("uses the current %s default and fallback model list", (provider, model, models) => {
    const preset = findLlmPreset(provider);

    expect(preset.defaultModel).toBe(model);
    expect(preset.models).toEqual(models);
    expect(resolveLlmModel(provider)).toBe(model);
  });

  it("preserves explicit model choices for every provider", () => {
    for (const [provider, model] of [
      ["openai", "gpt-4.1-mini"],
      ["xai", "grok-4.6"],
      ["cerebras", "gpt-oss-20b"],
      ["siliconflow", "Qwen/Qwen3-32B"],
      ["custom-gateway", "saved-model"],
    ]) {
      expect(resolveLlmModel(provider, model)).toBe(model);
    }
  });

  it("uses the OpenAI default for empty custom model selections", () => {
    for (const provider of ["custom", "custom-gateway"]) {
      expect(resolveLlmModel(provider)).toBe("gpt-6-luna");
      expect(resolveLlmModel(provider, " ")).toBe("gpt-6-luna");
    }
  });
});

