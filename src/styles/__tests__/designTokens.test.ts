/**
 * Design-token contract tests — locks in the Apple aesthetic.
 * Reference: voltagent/awesome-design-md (apple/DESIGN.md).
 *
 * These tests parse src/styles/theme.css directly because JSDOM does not
 * apply external stylesheets — the contract is the source-file value, not a
 * resolved getComputedStyle().
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const themeCss = readFileSync(
  resolve(__dirname, "../theme.css"),
  "utf-8",
);

/**
 * Extract a CSS custom property value from a specific block (e.g. `:root` or
 * `[data-theme="dark"]`). Returns the raw value with surrounding whitespace
 * trimmed. Throws if the block or property is missing — failed lookups should
 * make the test fail with a clear message, not silently match.
 */
function tokenValue(block: string, name: string): string {
  // Match the opening of the requested block, then capture up to its closing
  // brace. Block selectors use simple patterns; we escape regex metacharacters.
  const blockEscaped = block.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const blockRe = new RegExp(
    `${blockEscaped}\\s*(?:,\\s*[^{]+)?\\{([\\s\\S]*?)\\n\\}`,
    "m",
  );
  const blockMatch = themeCss.match(blockRe);
  if (!blockMatch) {
    throw new Error(`Block ${block} not found in theme.css`);
  }
  const body = blockMatch[1];
  const propRe = new RegExp(`${name}\\s*:\\s*([^;]+);`);
  const propMatch = body.match(propRe);
  if (!propMatch) {
    throw new Error(`Property ${name} not found in ${block} block`);
  }
  return propMatch[1].trim();
}

describe("Apple design-token contract — light theme (:root)", () => {
  it("uses Action Blue (#0066cc) as the single accent color", () => {
    expect(tokenValue(":root", "--color-accent")).toBe("#0066cc");
  });

  it("uses Focus Blue (#0071e3) as the accent hover state", () => {
    expect(tokenValue(":root", "--color-accent-hover")).toBe("#0071e3");
  });

  it("exposes the accent in RGB triplet form for rgba() use", () => {
    expect(tokenValue(":root", "--color-accent-rgb")).toBe("0, 102, 204");
  });

  it("uses pure white canvas (#ffffff)", () => {
    expect(tokenValue(":root", "--color-bg-primary")).toBe("#ffffff");
  });

  it("uses Apple's signature parchment (#f5f5f7) for the secondary surface", () => {
    expect(tokenValue(":root", "--color-bg-secondary")).toBe("#f5f5f7");
  });

  it("uses near-black ink (#1d1d1f) for primary text — never pure black", () => {
    expect(tokenValue(":root", "--color-text-primary")).toBe("#1d1d1f");
  });

  it("flat canvas instead of decorative gradients (Apple has zero gradient tokens)", () => {
    // The variable still exists for backwards compat, but it must resolve to
    // the flat canvas color rather than a multi-stop gradient.
    expect(tokenValue(":root", "--bg-gradient")).toBe(
      "var(--color-bg-primary)",
    );
  });
});

describe("Apple design-token contract — dark theme", () => {
  it("uses Sky Link Blue (#2997ff) for accent on dark surfaces", () => {
    // Action Blue would disappear against a #272729 tile — dark mode upgrades
    // to Sky Link Blue, the brighter sibling.
    expect(tokenValue('[data-theme="dark"]', "--color-accent")).toBe("#2997ff");
  });

  it("uses Apple's ink (#1d1d1f) as the dark canvas, not warm black", () => {
    expect(tokenValue('[data-theme="dark"]', "--color-bg-primary")).toBe(
      "#1d1d1f",
    );
  });

  it("uses the tile-1 surface (#272729) as the secondary dark layer", () => {
    expect(tokenValue('[data-theme="dark"]', "--color-bg-secondary")).toBe(
      "#272729",
    );
  });

  it("reserves true black (#000000) for sunken/void surfaces only", () => {
    expect(tokenValue('[data-theme="dark"]', "--color-bg-sunken")).toBe(
      "#000000",
    );
  });
});

describe("Apple radius scale — 5 / 8 / 11 / 18 / pill", () => {
  it("xs radius is 5px", () => {
    expect(tokenValue(":root", "--radius-xs")).toBe("5px");
  });

  it("sm radius is 8px", () => {
    expect(tokenValue(":root", "--radius-sm")).toBe("8px");
  });

  it("md radius is 11px (Apple Pearl Button)", () => {
    expect(tokenValue(":root", "--radius-md")).toBe("11px");
  });

  it("lg radius is 18px (Apple store utility cards)", () => {
    expect(tokenValue(":root", "--radius-lg")).toBe("18px");
  });

  it("full radius is pill (9999px)", () => {
    expect(tokenValue(":root", "--radius-full")).toBe("9999px");
  });
});

describe("Apple typography contract", () => {
  it("display font uses SF Pro Display first", () => {
    const value = tokenValue(":root", "--font-display");
    expect(value).toMatch(/SF Pro Display/);
    expect(value).toMatch(/-apple-system|BlinkMacSystemFont/);
  });

  it("body font uses SF Pro Text first", () => {
    const value = tokenValue(":root", "--font-sans");
    expect(value).toMatch(/SF Pro Text/);
  });

  it("body line-height is 1.47 (Apple body) for normal leading", () => {
    expect(tokenValue(":root", "--leading-normal")).toBe("1.47");
  });

  it("display line-height is 1.10 for tight headlines", () => {
    expect(tokenValue(":root", "--leading-tight")).toBe("1.10");
  });

  it("exposes negative tracking tokens for the 'Apple tight' display cadence", () => {
    expect(tokenValue(":root", "--tracking-display")).toMatch(/^-0\.0\d+em$/);
    expect(tokenValue(":root", "--tracking-tight")).toMatch(/^-0\.0\d+em$/);
  });
});

describe("Apple shadow contract — UI is hairline-only", () => {
  it("xs shadow is a hairline ring, not a drop shadow", () => {
    // Apple's UI never uses drop shadows on chrome — the only allowed
    // 'shadow' token at small sizes is a 1px hairline.
    const value = tokenValue(":root", "--shadow-xs");
    expect(value).toMatch(/0 0 0 1px/);
  });

  it("exposes a single product-shadow token reserved for photographic surfaces", () => {
    // The signature drop — `rgba(0, 0, 0, 0.16) 3px 5px 30px` — is the only
    // true drop shadow in the entire system. It MUST exist as a dedicated
    // token so callers can reach for it intentionally rather than reaching
    // for `--shadow-md` and getting fluffy elevation everywhere.
    const value = tokenValue(":root", "--shadow-product");
    expect(value).toMatch(/3px 5px 30px/);
  });
});
