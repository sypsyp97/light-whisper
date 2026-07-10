import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const subtitleCss = readFileSync(resolve("src/styles/subtitle.css"), "utf8");

describe("subtitle indicator alignment", () => {
  it("centers the polishing mark on the first subtitle line", () => {
    const polishingRule = subtitleCss.match(/\.subtitle-dot-polishing\s*\{([^}]*)\}/)?.[1];

    expect(polishingRule).toBeDefined();
    expect(polishingRule).toMatch(/margin-top:\s*5\.25px;/);
  });
});
