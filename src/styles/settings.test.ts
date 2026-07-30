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

  it("shows structure intensity as a filled progress track", () => {
    const track = ruleBody(
      settingsCss,
      ".polish-structure-control .polish-structure-range::-webkit-slider-runnable-track",
    );

    expect(track).toMatch(/linear-gradient/);
    expect(track).toMatch(/--structure-progress/);
    expect(track).toMatch(/--color-accent/);
  });

  it("bundles MiSans with a real file for every UI weight", () => {
    const fontFaces = themeCss.match(/@font-face\s*\{[^}]*\}/g) ?? [];
    const fontFamilyProperty = ["font", "family"].join("-");
    const root = ruleBody(themeCss, ":root");

    expect(fontFaces).toHaveLength(4);
    for (const [file, weight] of [
      ["MiSans-Regular.woff2", 400],
      ["MiSans-Medium.woff2", 500],
      ["MiSans-Demibold.woff2", 600],
      ["MiSans-Semibold.woff2", 700],
    ] as const) {
      const face = fontFaces.find((block) => block.includes(file));
      expect(face, `Missing @font-face for ${file}`).toBeDefined();
      expect(face).toContain(`${fontFamilyProperty}:`);
      expect(face).toContain('"MiSans"');
      expect(face).toMatch(new RegExp(`font-weight:\\s*${weight};`));
      expect(face).toMatch(/font-display:\s*swap;/);
    }
    expect(root).toMatch(/--font-ui:\s*"MiSans"/);
    expect(root).toMatch(/--font-serif:\s*var\(--font-ui\);/);
    expect(root).toMatch(/--font-sans:\s*var\(--font-ui\);/);
    expect(root).toMatch(/--font-mono:\s*var\(--font-ui\);/);
    expect(root).toMatch(/--font-display:\s*var\(--font-ui\);/);
    expect(root).toMatch(/--tracking-body:\s*-0\.008em;/);
    expect(root).toMatch(/--tracking-display:\s*-0\.02em;/);
    expect(themeCss).not.toMatch(new RegExp("Maple" + " Mono"));
    expect(themeCss).not.toMatch(new RegExp("Source" + " Han"));
  });

  it("keeps picker selection indicators beside their option on narrow windows", () => {
    const option = ruleBody(settingsCss, ".picker-option");
    const indicator = ruleBody(settingsCss, ".picker-option[data-active]::after");

    expect(option).toMatch(/align-items:\s*center;/);
    expect(indicator).toMatch(/flex:\s*0 0 18px;/);
    expect(indicator).toMatch(/align-self:\s*center;/);
    expect(settingsCss).not.toMatch(
      /@media\s*\(max-width:\s*720px\)[\s\S]*?\.picker-option\s*\{[^}]*flex-direction:\s*column;/,
    );
  });
});
