import { describe, expect, it } from "vitest";
import {
  resolveAssistantModelToggleState,
  resolveAssistantModelForProviderChange,
  resolveAssistantModelForPolishProviderChange,
  resolveAssistantModelState,
  resolveAssistantProviderToPersist,
  resolveEffectiveAssistantProvider,
  shouldShowFastModeToggle,
} from "./fastMode";

const base = {
  loggedIn: true,
  authMode: "oauth" as const,
  llmProvider: "openai",
  effectiveAssistantProvider: "openai",
};

describe("shouldShowFastModeToggle", () => {
  it("shows in polish block when polish uses openai + oauth + logged in", () => {
    expect(
      shouldShowFastModeToggle({ ...base, scope: "polish" }),
    ).toBe(true);
  });

  it("hides when user is logged out of oauth", () => {
    expect(
      shouldShowFastModeToggle({ ...base, scope: "polish", loggedIn: false }),
    ).toBe(false);
    expect(
      shouldShowFastModeToggle({ ...base, scope: "assistant", loggedIn: false }),
    ).toBe(false);
  });

  it("hides when auth mode is api_key (fast mode is ChatGPT-only)", () => {
    expect(
      shouldShowFastModeToggle({ ...base, scope: "polish", authMode: "api_key" }),
    ).toBe(false);
    expect(
      shouldShowFastModeToggle({ ...base, scope: "assistant", authMode: "api_key" }),
    ).toBe(false);
  });

  it("hides in polish block when polish uses a non-openai provider", () => {
    expect(
      shouldShowFastModeToggle({
        ...base,
        scope: "polish",
        llmProvider: "cerebras",
      }),
    ).toBe(false);
  });

  it("shows in assistant block when polish is non-openai but assistant is openai", () => {
    expect(
      shouldShowFastModeToggle({
        ...base,
        scope: "assistant",
        llmProvider: "cerebras",
        effectiveAssistantProvider: "openai",
      }),
    ).toBe(true);
  });

  it("hides in assistant block when polish is already openai (polish block owns the toggle)", () => {
    // Prevents two toggles bound to the same flag when both sides use OpenAI.
    expect(
      shouldShowFastModeToggle({
        ...base,
        scope: "assistant",
        llmProvider: "openai",
        effectiveAssistantProvider: "openai",
      }),
    ).toBe(false);
  });

  it("hides in assistant block when assistant does not use openai", () => {
    expect(
      shouldShowFastModeToggle({
        ...base,
        scope: "assistant",
        llmProvider: "cerebras",
        effectiveAssistantProvider: "deepseek",
      }),
    ).toBe(false);
  });

  it("hides in assistant block when separate config is off with a stale openai assistant provider", () => {
    const effectiveAssistantProvider = resolveEffectiveAssistantProvider({
      assistantUseSeparateModel: false,
      assistantProvider: "openai",
      llmProvider: "cerebras",
    });

    expect(effectiveAssistantProvider).toBe("cerebras");
    expect(
      shouldShowFastModeToggle({
        ...base,
        scope: "assistant",
        llmProvider: "cerebras",
        effectiveAssistantProvider,
      }),
    ).toBe(false);
  });
});

describe("resolveEffectiveAssistantProvider", () => {
  it("uses the polish provider when assistant separate config is off", () => {
    expect(
      resolveEffectiveAssistantProvider({
        assistantUseSeparateModel: false,
        assistantProvider: "openai",
        llmProvider: "deepseek",
      }),
    ).toBe("deepseek");
  });

  it("uses the assistant provider when assistant separate config is on", () => {
    expect(
      resolveEffectiveAssistantProvider({
        assistantUseSeparateModel: true,
        assistantProvider: "openai",
        llmProvider: "deepseek",
      }),
    ).toBe("openai");
  });

  it("falls back to the polish provider when assistant provider is blank", () => {
    expect(
      resolveEffectiveAssistantProvider({
        assistantUseSeparateModel: true,
        assistantProvider: " ",
        llmProvider: "cerebras",
      }),
    ).toBe("cerebras");
  });

  it("falls back to the polish provider when assistant provider is unavailable", () => {
    expect(
      resolveEffectiveAssistantProvider({
        assistantUseSeparateModel: true,
        assistantProvider: "deleted-custom-provider",
        llmProvider: "cerebras",
        availableProviders: ["openai", "deepseek", "cerebras"],
      }),
    ).toBe("cerebras");
  });
});

describe("resolveAssistantModelState", () => {
  it("follows the polish model when assistant separate config is off", () => {
    expect(resolveAssistantModelState({
      assistantUseSeparateModel: false,
      savedAssistantModel: "stale-assistant-model",
      polishModel: "current-polish-model",
      assistantDefaultModel: "assistant-default-model",
    })).toBe("current-polish-model");
  });

  it("uses the saved assistant model when separate config is on", () => {
    expect(resolveAssistantModelState({
      assistantUseSeparateModel: true,
      savedAssistantModel: "assistant-model",
      polishModel: "current-polish-model",
      assistantDefaultModel: "assistant-default-model",
    })).toBe("assistant-model");
  });

  it("falls back to the assistant provider default when separate config is on and saved model is blank", () => {
    expect(resolveAssistantModelState({
      assistantUseSeparateModel: true,
      savedAssistantModel: "   ",
      polishModel: "current-polish-model",
      assistantDefaultModel: "assistant-default-model",
    })).toBe("assistant-default-model");
  });
});

describe("resolveAssistantProviderToPersist", () => {
  it("clears provider when separate config is off", () => {
    expect(resolveAssistantProviderToPersist({
      assistantUseSeparateModel: false,
      assistantProvider: "openai",
    })).toBeNull();
  });

  it("clears provider when separate config follows the polish provider", () => {
    expect(resolveAssistantProviderToPersist({
      assistantUseSeparateModel: true,
      assistantProvider: " ",
      availableProviders: ["openai", "deepseek"],
    })).toBeNull();
  });

  it("clears provider when the saved provider is unavailable", () => {
    expect(resolveAssistantProviderToPersist({
      assistantUseSeparateModel: true,
      assistantProvider: "deleted-custom-provider",
      availableProviders: ["openai", "deepseek"],
    })).toBeNull();
  });

  it("persists an explicit available provider", () => {
    expect(resolveAssistantProviderToPersist({
      assistantUseSeparateModel: true,
      assistantProvider: "deepseek",
      availableProviders: ["openai", "deepseek"],
    })).toBe("deepseek");
  });
});

describe("resolveAssistantModelToggleState", () => {
  it("keeps follow-polish provider unpinned when enabling from stale disabled state", () => {
    expect(resolveAssistantModelToggleState({
      enabled: true,
      previousAssistantUseSeparateModel: false,
      assistantProvider: "openai",
      availableProviders: ["openai", "deepseek", "cerebras"],
      savedAssistantModel: "stale-openai-model",
      polishModel: "cerebras-polish-model",
      assistantDefaultModel: "openai-default-model",
    })).toEqual({
      assistantModel: "cerebras-polish-model",
      assistantProviderState: "",
      assistantProviderToPersist: null,
    });
  });

  it("keeps an explicit provider only when it already belonged to enabled separate config", () => {
    expect(resolveAssistantModelToggleState({
      enabled: true,
      previousAssistantUseSeparateModel: true,
      assistantProvider: "openai",
      availableProviders: ["openai", "deepseek", "cerebras"],
      savedAssistantModel: "assistant-openai-model",
      polishModel: "cerebras-polish-model",
      assistantDefaultModel: "openai-default-model",
    })).toEqual({
      assistantModel: "assistant-openai-model",
      assistantProviderState: "openai",
      assistantProviderToPersist: "openai",
    });
  });

  it("clears provider and follows the polish model when disabling separate config", () => {
    expect(resolveAssistantModelToggleState({
      enabled: false,
      previousAssistantUseSeparateModel: true,
      assistantProvider: "openai",
      availableProviders: ["openai", "deepseek", "cerebras"],
      savedAssistantModel: "assistant-openai-model",
      polishModel: "cerebras-polish-model",
      assistantDefaultModel: "openai-default-model",
    })).toEqual({
      assistantModel: "cerebras-polish-model",
      assistantProviderState: "",
      assistantProviderToPersist: null,
    });
  });
});

describe("resolveAssistantModelForProviderChange", () => {
  it("uses the new provider default when switching assistant provider", () => {
    expect(resolveAssistantModelForProviderChange({
      nextAssistantProvider: "deepseek",
      polishProvider: "openai",
      polishModel: "gpt-4.1-mini",
      nextProviderDefaultModel: "deepseek-v4-flash",
    })).toBe("deepseek-v4-flash");
  });

  it("uses the polish model when the assistant provider changes back to polish provider", () => {
    expect(resolveAssistantModelForProviderChange({
      nextAssistantProvider: "custom-polish",
      polishProvider: "custom-polish",
      polishModel: "edited-polish-model",
      nextProviderDefaultModel: "saved-custom-default",
    })).toBe("edited-polish-model");
  });
});

describe("resolveAssistantModelForPolishProviderChange", () => {
  it("uses the new polish model when assistant follows the polish provider", () => {
    expect(resolveAssistantModelForPolishProviderChange({
      assistantUseSeparateModel: true,
      assistantProviderToPersist: null,
      savedAssistantModel: "old-displayed-default",
      nextPolishModel: "new-polish-default",
      assistantDefaultModel: "old-assistant-default",
    })).toBe("new-polish-default");
  });

  it("keeps an explicit assistant provider model when assistant provider is pinned", () => {
    expect(resolveAssistantModelForPolishProviderChange({
      assistantUseSeparateModel: true,
      assistantProviderToPersist: "openai",
      savedAssistantModel: "assistant-override",
      nextPolishModel: "deepseek-v4-flash",
      assistantDefaultModel: "gpt-4.1-mini",
    })).toBe("assistant-override");
  });

  it("uses the explicit assistant provider default when pinned provider has no model override", () => {
    expect(resolveAssistantModelForPolishProviderChange({
      assistantUseSeparateModel: true,
      assistantProviderToPersist: "openai",
      savedAssistantModel: " ",
      nextPolishModel: "deepseek-v4-flash",
      assistantDefaultModel: "gpt-4.1-mini",
    })).toBe("gpt-4.1-mini");
  });
});
