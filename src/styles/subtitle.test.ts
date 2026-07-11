import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const subtitleCss = readFileSync(resolve("src/styles/subtitle.css"), "utf8");

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
