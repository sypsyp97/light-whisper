import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const source = readFileSync(resolve(process.cwd(), "src/pages/SettingsPage.tsx"), "utf8");

function toolbarRefreshHandler(inputRef: string): string {
  const toolbarStart = source.indexOf(`ref={${inputRef}}`);
  expect(toolbarStart).toBeGreaterThanOrEqual(0);
  const handlerStart = source.indexOf("onClick={() => {", toolbarStart);
  const handlerEnd = source.indexOf("}}", handlerStart);
  expect(handlerStart).toBeGreaterThan(toolbarStart);
  expect(handlerEnd).toBeGreaterThan(handlerStart);
  return source.slice(handlerStart, handlerEnd);
}

describe("SettingsPage model discovery ordering", () => {
  it("cancels queued polish discovery before a forced refresh", () => {
    const handler = toolbarRefreshHandler("modelSearchInputRef");
    expect(handler.indexOf("aiModelsFetch.cancel();")).toBeLessThan(
      handler.indexOf("void refreshAiModels();"),
    );
  });

  it("cancels the matching assistant discovery queue before a forced refresh", () => {
    const handler = toolbarRefreshHandler("assistantModelSearchInputRef");
    expect(handler.indexOf("assistantModelsFetch.cancel();")).toBeLessThan(
      handler.indexOf("void refreshAssistantModels();"),
    );
    expect(handler.indexOf("aiModelsFetch.cancel();")).toBeLessThan(
      handler.indexOf("void refreshAiModels();"),
    );
  });

  it("keeps independent assistant discovery errors separate from polish errors", () => {
    expect(source).toContain(
      'const [assistantModelsError, setAssistantModelsError] = useState("");',
    );
    expect(source).toContain("setAssistantModelsError(message);");
    expect(source).toContain(
      "(assistantProviderDiffers ? assistantModelsError : aiModelsError)",
    );
  });
});
