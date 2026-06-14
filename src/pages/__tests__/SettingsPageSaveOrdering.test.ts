import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const source = readFileSync(resolve(process.cwd(), "src/pages/SettingsPage.tsx"), "utf8");

function expectCancelBeforeImmediateSave(marker: string) {
  const start = source.indexOf(marker);
  expect(start).toBeGreaterThanOrEqual(0);

  const save = source.indexOf("await setLlmProviderConfig(", start);
  expect(save).toBeGreaterThan(start);

  const cancel = source.indexOf("llmConfigSave.cancel();", start);
  expect(cancel).toBeGreaterThan(start);
  expect(cancel).toBeLessThan(save);
}

describe("SettingsPage immediate LLM config saves", () => {
  it("cancels pending debounced saves before switching the polish provider", () => {
    expectCancelBeforeImmediateSave("const handleProviderSelect = useCallback");
  });

  it("cancels pending debounced saves before switching the assistant provider", () => {
    expectCancelBeforeImmediateSave("const handleAssistantProviderSelect = useCallback");
  });

  it("cancels pending debounced saves before selecting a newly added custom provider", () => {
    expectCancelBeforeImmediateSave("disabled={!newProviderName.trim() || !newProviderBaseUrl.trim()}");
  });
});
