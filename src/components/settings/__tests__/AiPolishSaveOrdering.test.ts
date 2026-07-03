import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const source = readFileSync(
  resolve(process.cwd(), "src/components/settings/AiPolishSection.tsx"),
  "utf8",
);

function expectCancelBeforeImmediateSave(marker: string) {
  const start = source.indexOf(marker);
  expect(start).toBeGreaterThanOrEqual(0);

  const save = source.indexOf("await persistProviderConfig(", start);
  expect(save).toBeGreaterThan(start);

  const cancel = source.indexOf("baseUrlSave.cancel();", start);
  expect(cancel).toBeGreaterThan(start);
  expect(cancel).toBeLessThan(save);
}

describe("AiPolishSection immediate LLM config saves", () => {
  it("cancels pending debounced saves before switching the polish provider", () => {
    expectCancelBeforeImmediateSave("const handleProviderChange = useCallback");
  });

  it("cancels pending debounced saves before switching the polish model", () => {
    expectCancelBeforeImmediateSave("const handleModelChange = useCallback");
  });

  it("cancels pending debounced saves before switching the polish reasoning mode", () => {
    expectCancelBeforeImmediateSave("const handleReasoningChange = useCallback");
  });

  it("cancels pending debounced saves before switching OpenAI auth mode", () => {
    expectCancelBeforeImmediateSave("const handleAuthModeChange = useCallback");
  });
});
