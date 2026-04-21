import { describe, expect, it } from "vitest";
import { shouldShowFastModeToggle } from "./fastMode";

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
});
