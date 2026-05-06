/**
 * Component-level CSS contract tests — locks in the Apple aesthetic for
 * specific UI primitives so that future refactors of the stylesheet can't
 * silently un-Apple things.
 *
 * Reference: voltagent/awesome-design-md (apple/DESIGN.md).
 *
 * These tests parse the CSS source rather than relying on JSDOM, because
 * JSDOM does not apply external stylesheets at all.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const appCss = readFileSync(resolve(__dirname, "../app.css"), "utf-8");
const themeCss = readFileSync(resolve(__dirname, "../theme.css"), "utf-8");

/** Extract the body of a single CSS rule (`.selector { … }`). */
function ruleBody(css: string, selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  // Match the selector at the start of a rule and capture everything up to
  // its (top-level) closing brace.
  const re = new RegExp(`(^|\\n)${escaped}\\s*\\{([\\s\\S]*?)\\n\\}`, "m");
  const match = css.match(re);
  if (!match) {
    throw new Error(`Selector ${selector} not found`);
  }
  return match[2];
}

describe("Apple radius scale — :root token values", () => {
  it("--lw-radius-sm is 8px", () => {
    expect(ruleBody(appCss, ":root")).toMatch(/--lw-radius-sm:\s*8px/);
  });

  it("--lw-radius-md is 11px (Apple Pearl Button)", () => {
    expect(ruleBody(appCss, ":root")).toMatch(/--lw-radius-md:\s*11px/);
  });

  it("--lw-radius-lg is 18px (Apple store utility cards)", () => {
    expect(ruleBody(appCss, ":root")).toMatch(/--lw-radius-lg:\s*18px/);
  });
});

describe("Primary Button — pill shape (Apple's signature CTA)", () => {
  it("the primary variant uses pill border-radius", () => {
    // Apple's `button-primary` is full-pill (`{rounded.pill}`). The full-pill
    // radius IS the brand action signal — flat-radius primary buttons read
    // as "rectangle utility", which is the wrong grammar.
    expect(ruleBody(appCss, ".lw-button--primary")).toMatch(
      /border-radius:\s*var\(--lw-radius-full\)/,
    );
  });

  it("the primary variant uses Apple Action Blue", () => {
    expect(ruleBody(appCss, ".lw-button--primary")).toMatch(
      /background:\s*var\(--lw-accent\)/,
    );
  });

  it("press state uses transform: scale(0.95) — the system-wide micro-interaction", () => {
    // Apple's universal press feedback is `transform: scale(0.95)`. We accept
    // any value in the [0.94, 0.96] range as "close enough".
    expect(ruleBody(appCss, ".lw-button:active:not(:disabled)")).toMatch(
      /transform:\s*scale\(0\.9[456]\)/,
    );
  });
});

describe("Card — hairline border, no decorative drop shadow", () => {
  it("Card uses 18px (Apple lg) border-radius", () => {
    expect(ruleBody(appCss, ".lw-card")).toMatch(
      /border-radius:\s*var\(--lw-radius-lg\)/,
    );
  });

  it("the hairline shadow token resolves to a 1px ring, not a fluffy drop", () => {
    // `--shadow-card` is the token Cards reach for. Apple's UI never uses
    // multi-radius drop shadows on chrome — only hairline rings.
    expect(ruleBody(themeCss, ":root")).toMatch(
      /--shadow-card:\s*0 0 0 1px[^;]+;/,
    );
  });
});

describe("Settings layout — top horizontal nav, scrollable content below", () => {
  it("html / body / #root pin to 100% height so the layout chain doesn't collapse", () => {
    // Without this, `.lw-root { height: 100% }` resolves to 0 in modes where
    // the document root has no intrinsic height (e.g. dev `vite`), and every
    // scrollable region fails to get a bounded height to scroll within.
    expect(themeCss).toMatch(
      /html,\s*body,\s*#root\s*\{[^}]*height:\s*100%/,
    );
  });

  it("settings body is a flex column, NOT a sidebar grid", () => {
    // The Apple branch shipped a 220px sidebar layout that broke scrolling
    // and didn't match main's UX. Pin the column flex shape so it can't
    // regress to a grid behind a refactor.
    const body = ruleBody(appCss, ".lw-settings-body");
    expect(body).toMatch(/display:\s*flex/);
    expect(body).toMatch(/flex-direction:\s*column/);
    expect(body).not.toMatch(/grid-template-columns/);
  });

  it("settings nav is a single horizontal row of tabs at the top", () => {
    const nav = ruleBody(appCss, ".lw-settings-nav");
    expect(nav).toMatch(/flex-direction:\s*row/);
    // Long tab lists should scroll horizontally instead of wrapping —
    // wrapping breaks the "one row" promise the user explicitly asked for.
    expect(nav).toMatch(/overflow-x:\s*auto/);
  });

  it("settings content has min-height: 0 so overflow-y: auto actually scrolls", () => {
    // Flex items default to `min-height: auto`, which forbids shrinking
    // below their intrinsic content size. Without `min-height: 0`, the
    // inner `overflow-y: auto` never engages and the page becomes
    // un-scrollable when sections exceed the viewport.
    const content = ruleBody(appCss, ".lw-settings-content");
    expect(content).toMatch(/min-height:\s*0/);
    expect(content).toMatch(/overflow-y:\s*auto/);
  });

  it("main content also pins min-height: 0 so its history list can scroll", () => {
    const content = ruleBody(appCss, ".lw-main-content");
    expect(content).toMatch(/min-height:\s*0/);
    expect(content).toMatch(/overflow-y:\s*auto/);
  });
});

describe("Recording button — the one product surface that may use the signature drop-shadow", () => {
  it("--shadow-product token uses Apple's signature 3px 5px 30px drop", () => {
    // The single drop-shadow Apple ships is reserved for photographic product
    // imagery. In our app the recording mic IS the product — it's the only
    // surface that should ever reach for `--shadow-product`.
    expect(ruleBody(themeCss, ":root")).toMatch(
      /--shadow-product:[^;]*3px 5px 30px[^;]*;/,
    );
  });

  it("the record button reaches for --shadow-product (or the lw-namespaced alias)", () => {
    // The recording mic is the Light Whisper "product" — the button at rest
    // should carry the signature Apple drop-shadow, not the hairline shadow
    // tokens used by mundane chrome.
    const body = ruleBody(appCss, ".lw-record-btn");
    expect(body).toMatch(
      /box-shadow:\s*var\(--(lw-)?shadow-product\)/,
    );
  });
});
