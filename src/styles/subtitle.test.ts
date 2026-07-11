import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const subtitleCss = readFileSync(resolve("src/styles/subtitle.css"), "utf8");
const themeCss = readFileSync(resolve("src/styles/theme.css"), "utf8");

function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

function customProperty(block: string, name: string): string {
  return block.match(new RegExp(`${name}:\\s*([^;]+);`))?.[1].trim() ?? "";
}

describe("subtitle indicator alignment", () => {
  it("centers assistant and polishing marks in the same fixed slot", () => {
    const slotRule = subtitleCss.match(/\.subtitle-status-indicator\s*\{([^}]*)\}/)?.[1];
    const textRule = subtitleCss.match(/\.subtitle-text\s*\{([^}]*)\}/)?.[1];
    const waveformRule = subtitleCss.match(
      /(?:^|\n)\.subtitle-waveform-indicator\s*\{([^}]*)\}/,
    )?.[1];
    const waveformBarRule = subtitleCss.match(
      /(?:^|\n)\.subtitle-waveform-indicator-bar\s*\{([^}]*)\}/,
    )?.[1];
    const sharedMarkRule = subtitleCss.match(
      /\.subtitle-dot-assistant,\s*\.subtitle-dot-polishing\s*\{([^}]*)\}/,
    )?.[1];

    expect(slotRule).toMatch(/display:\s*inline-flex;/);
    expect(slotRule).toMatch(/align-items:\s*center;/);
    expect(slotRule).toMatch(/justify-content:\s*center;/);
    expect(slotRule).toMatch(/width:\s*28px;/);
    expect(slotRule).toMatch(/height:\s*24px;/);
    expect(textRule).toMatch(/line-height:\s*24px;/);
    expect(waveformRule).toMatch(/width:\s*28px;/);
    expect(waveformRule).toMatch(/gap:\s*1px;/);
    expect(waveformBarRule).toMatch(/flex:\s*0\s+0\s+2px;/);
    expect(sharedMarkRule).toMatch(/width:\s*10px;/);
    expect(sharedMarkRule).toMatch(/height:\s*10px;/);
    expect(sharedMarkRule).toMatch(/margin-top:\s*0;/);
  });
});

describe("assistant and polish visual contracts", () => {
  it("keeps assistant and polish colors distinct in both themes", () => {
    const lightTheme = themeCss.match(/:root\s*\{([\s\S]*?)\n\}/)?.[1] ?? "";
    const darkTheme = themeCss.match(
      /\[data-theme="dark"\],\s*\.dark\s*\{([\s\S]*?)\n\}/,
    )?.[1] ?? "";

    expect(customProperty(lightTheme, "--color-assistant")).not.toBe("");
    expect(customProperty(lightTheme, "--color-assistant")).not.toBe(
      customProperty(lightTheme, "--color-accent"),
    );
    expect(customProperty(darkTheme, "--color-assistant")).not.toBe("");
    expect(customProperty(darkTheme, "--color-assistant")).not.toBe(
      customProperty(darkTheme, "--color-accent"),
    );
  });

  it("routes assistant indicators through the assistant token and polish through accent", () => {
    expect(ruleBody(subtitleCss, ".subtitle-dot-assistant")).toMatch(
      /background:\s*var\(--color-assistant\);/,
    );
    const polishRules = Array.from(
      subtitleCss.matchAll(/\.subtitle-dot-polishing\s*\{([^}]*)\}/g),
      (match) => match[1],
    );
    expect(polishRules).toContainEqual(
      expect.stringMatching(/background:\s*var\(--color-accent\);/),
    );
    expect(
      ruleBody(
        subtitleCss,
        ".subtitle-waveform-indicator.is-assistant .subtitle-waveform-indicator-bar",
      ),
    ).toMatch(/background:\s*var\(--color-assistant\);/);
  });

  it("keeps status-only assistant processing in compact capsule padding", () => {
    const assistantPanel = ruleBody(subtitleCss, ".subtitle-capsule-assistant");
    const statusCapsule = ruleBody(
      subtitleCss,
      ".subtitle-capsule:has(.subtitle-hint)",
    );

    expect(assistantPanel).toMatch(/padding:\s*14px\s+16px;/);
    expect(statusCapsule).toMatch(/padding:\s*8px\s+16px;/);
    expect(
      subtitleCss.indexOf(".subtitle-capsule:has(.subtitle-hint)"),
    ).toBeGreaterThan(subtitleCss.indexOf(".subtitle-capsule-assistant"));
  });
});

describe("autostart switch visual contract", () => {
  it("uses different off/on tracks and moves the knob when checked", () => {
    const offTrack = ruleBody(themeCss, '.toggle-switch[aria-checked="false"]');
    const onTrack = ruleBody(themeCss, '.toggle-switch[aria-checked="true"]');
    const offKnob = ruleBody(
      themeCss,
      '.toggle-switch[aria-checked="false"] .toggle-knob',
    );
    const onKnob = ruleBody(
      themeCss,
      '.toggle-switch[aria-checked="true"] .toggle-knob',
    );

    expect(offTrack).toMatch(/background:\s*var\(--color-bg-tertiary\);/);
    expect(onTrack).toMatch(/background:\s*var\(--color-accent\);/);
    expect(offTrack.match(/background:\s*([^;]+);/)?.[1]).not.toBe(
      onTrack.match(/background:\s*([^;]+);/)?.[1],
    );
    expect(offKnob).toMatch(/transform:\s*translateX\(0\);/);
    expect(onKnob).toMatch(/transform:\s*translateX\((?!0(?:px)?\))[^)]+\);/);
  });
});
