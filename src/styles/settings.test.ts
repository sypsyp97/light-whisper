import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const settingsCss = readFileSync(resolve("src/styles/pages.css"), "utf8");
const themeCss = readFileSync(resolve("src/styles/theme.css"), "utf8");

function ruleBody(css: string, selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escapedSelector}\\s*\\{([^}]*)\\}`));
  expect(match, `Missing CSS rule for ${selector}`).not.toBeNull();
  return match?.[1] ?? "";
}

describe("settings CSS contracts", () => {
  it("opens the right-aligned language popover inward", () => {
    const languagePopover = ruleBody(
      settingsCss,
      ".settings-language-picker .settings-language-popover",
    );

    expect(languagePopover).toMatch(/left:\s*auto;/);
    expect(languagePopover).toMatch(/right:\s*0;/);
    expect(languagePopover).toMatch(/transform-origin:\s*top right;/);
  });

  it("keeps compact slider visuals while exposing a 24px pointer target", () => {
    const input = ruleBody(themeCss, 'input[type="range"]');
    const track = ruleBody(themeCss, 'input[type="range"]::-webkit-slider-runnable-track');

    expect(input).toMatch(/min-height:\s*24px;/);
    expect(input).toMatch(/height:\s*24px;/);
    expect(track).toMatch(/height:\s*4px;/);
  });

  it("uses the bundled full-coverage Source Han Sans variable font", () => {
    const fontFace = ruleBody(themeCss, "@font-face");
    const root = ruleBody(themeCss, ":root");

    expect(fontFace).toMatch(/font-family:\s*"Source Han Sans SC";/);
    expect(fontFace).toMatch(/SourceHanSansSC-VF\.woff2/);
    expect(fontFace).toMatch(/font-weight:\s*250 900;/);
    expect(root).toMatch(/--font-sans:\s*"Source Han Sans SC"/);
  });
});
