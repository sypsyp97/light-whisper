import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const settingsCss = readFileSync(resolve("src/styles/pages.css"), "utf8");

function ruleBody(selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = settingsCss.match(new RegExp(`${escapedSelector}\\s*\\{([^}]*)\\}`));
  expect(match, `Missing CSS rule for ${selector}`).not.toBeNull();
  return match?.[1] ?? "";
}

describe("settings CSS contracts", () => {
  it("opens the right-aligned language popover inward", () => {
    const languagePopover = ruleBody(
      ".settings-language-picker .settings-language-popover",
    );

    expect(languagePopover).toMatch(/left:\s*auto;/);
    expect(languagePopover).toMatch(/right:\s*0;/);
    expect(languagePopover).toMatch(/transform-origin:\s*top right;/);
  });
});
