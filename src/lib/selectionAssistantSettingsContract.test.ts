import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = path.resolve(import.meta.dirname, "../..");
const selectionSettings = fs.readFileSync(
  path.join(repoRoot, "src", "components", "settings", "SelectionAssistantSettingsSection.tsx"),
  "utf8",
);
const settingsPage = fs.readFileSync(
  path.join(repoRoot, "src", "pages", "SettingsPage.tsx"),
  "utf8",
);
const selectionCommand = fs.readFileSync(
  path.join(repoRoot, "src-tauri", "src", "commands", "selection.rs"),
  "utf8",
);

describe("selection assistant independent model settings contract", () => {
  it("shares provider and reasoning options with the voice assistant", () => {
    expect(selectionSettings).toContain('from "@/lib/llmModelOptions"');
    expect(settingsPage).toContain('from "@/lib/llmModelOptions"');
    expect(selectionSettings).toContain('className="picker-trigger"');
    expect(selectionSettings).toContain('className="picker-inline-row"');
  });

  it("exposes the same OpenAI authentication and Codex login controls", () => {
    expect(selectionSettings).toContain("openaiControls");
    expect(settingsPage).toMatch(
      /openaiControls={[\s\S]*renderOpenaiAuthModeToggle\(\)[\s\S]*renderOpenaiCodexOauthBlock\("assistant", true\)/,
    );
  });

  it("resolves selection requests through the Codex OAuth-aware route", () => {
    expect(selectionCommand).toContain("selection_endpoint_for_config");
    expect(selectionCommand).toContain("resolve_api_key_for_provider");
  });

  it("configures automatic screenshots as a persistent setting instead of a toolbar action", () => {
    expect(selectionSettings).toContain("autoScreenshot");
    expect(selectionSettings).toContain("settings.selectionAutoScreenshot");
    expect(selectionCommand).toContain("auto_screenshot");
  });
});
