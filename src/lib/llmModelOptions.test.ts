import { describe, expect, it } from "vitest";

import { llmProviderOptions } from "./llmModelOptions";

describe("DeepSeek model options", () => {
  it("offers only the currently supported built-in models for new configurations", () => {
    const deepseek = llmProviderOptions.find((option) => option.key === "deepseek");

    expect(deepseek?.models).toEqual(["deepseek-v4-flash", "deepseek-v4-pro"]);
  });
});
