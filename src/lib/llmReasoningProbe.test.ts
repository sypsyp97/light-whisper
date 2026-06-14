import { describe, expect, it } from "vitest";
import {
  resolveAssistantLlmReasoningProbeTarget,
  resolveLlmReasoningProbeTarget,
} from "./llmReasoningProbe";
import type { CustomProvider } from "@/types";

const customProviders: CustomProvider[] = [
  {
    id: "custom-polish",
    name: "Polish API",
    base_url: "https://polish.example/v1",
    model: "polish-model",
    api_format: "openai_compat",
  },
  {
    id: "custom-assistant",
    name: "Assistant Anthropic API",
    base_url: "https://assistant.example",
    model: "assistant-model",
    api_format: "anthropic",
  },
];

describe("resolveLlmReasoningProbeTarget", () => {
  it("uses the selected custom provider instead of the polish draft", () => {
    const target = resolveLlmReasoningProbeTarget({
      provider: "custom-assistant",
      customBaseUrl: null,
      customModel: "",
      customProviders,
      defaults: {
        baseUrl: "https://default.example/v1",
        model: "default-model",
      },
    });

    expect(target).toEqual({
      provider: "custom-assistant",
      baseUrl: "https://assistant.example",
      model: "assistant-model",
      apiFormat: "anthropic",
    });
  });

  it("uses draft values for the selected custom provider being edited", () => {
    const target = resolveLlmReasoningProbeTarget({
      provider: "custom-polish",
      customBaseUrl: "https://edited-polish.example/v1",
      customModel: "edited-polish-model",
      customProviders,
      defaults: {
        baseUrl: "https://default.example/v1",
        model: "default-model",
      },
    });

    expect(target).toEqual({
      provider: "custom-polish",
      baseUrl: "https://edited-polish.example/v1",
      model: "edited-polish-model",
      apiFormat: "openai_compat",
    });
  });

  it("can reuse an edited provider base while probing a separate model", () => {
    const target = resolveLlmReasoningProbeTarget({
      provider: "custom-polish",
      customBaseUrl: "https://edited-polish.example/v1",
      customModel: "assistant-on-same-provider",
      customProviders,
      defaults: {
        baseUrl: "https://default.example/v1",
        model: "default-model",
      },
    });

    expect(target).toEqual({
      provider: "custom-polish",
      baseUrl: "https://edited-polish.example/v1",
      model: "assistant-on-same-provider",
      apiFormat: "openai_compat",
    });
  });

  it("can reuse an edited provider model when the separate model is blank", () => {
    const target = resolveLlmReasoningProbeTarget({
      provider: "custom-polish",
      customBaseUrl: "https://edited-polish.example/v1",
      customModel: "edited-polish-model",
      customProviders,
      defaults: {
        baseUrl: "https://default.example/v1",
        model: "default-model",
      },
    });

    expect(target.model).toBe("edited-polish-model");
  });

  it("uses active draft values for the edited provider", () => {
    const target = resolveLlmReasoningProbeTarget({
      provider: "custom",
      customBaseUrl: "https://draft.example/v1",
      customModel: "draft-model",
      customProviders,
      defaults: {
        baseUrl: "http://127.0.0.1:8000",
        model: "gpt-4.1-mini",
      },
    });

    expect(target).toEqual({
      provider: "custom",
      baseUrl: "https://draft.example/v1",
      model: "draft-model",
      apiFormat: "openai_compat",
    });
  });
});

describe("resolveAssistantLlmReasoningProbeTarget", () => {
  const polishTarget = {
    provider: "custom-polish",
    baseUrl: "https://edited-polish.example/v1",
    model: "polish-model",
    apiFormat: "openai_compat" as const,
  };

  it("uses the assistant provider model when a separate assistant model is blank", () => {
    const target = resolveAssistantLlmReasoningProbeTarget({
      assistantUseSeparateModel: true,
      polishTarget,
      llmProvider: "custom-polish",
      effectiveAssistantProvider: "custom-assistant",
      assistantModel: "",
      customBaseUrl: "https://edited-polish.example/v1",
      customModel: "polish-model",
      customProviders,
      defaults: {
        baseUrl: "https://default-assistant.example",
        model: "default-assistant-model",
      },
    });

    expect(target).toEqual({
      provider: "custom-assistant",
      baseUrl: "https://assistant.example",
      model: "assistant-model",
      apiFormat: "anthropic",
    });
  });

  it("reuses the polish target when assistant separate mode is off", () => {
    const target = resolveAssistantLlmReasoningProbeTarget({
      assistantUseSeparateModel: false,
      polishTarget,
      llmProvider: "custom-polish",
      effectiveAssistantProvider: "custom-assistant",
      assistantModel: "assistant-model",
      customBaseUrl: "https://edited-polish.example/v1",
      customModel: "polish-model",
      customProviders,
      defaults: {
        baseUrl: "https://default-assistant.example",
        model: "default-assistant-model",
      },
    });

    expect(target).toBe(polishTarget);
  });
});
