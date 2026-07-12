import { describe, expect, it } from "vitest";

import { sanitizeGoogleSearchEntryPointHtml } from "../GoogleSearchEntryPoint";

describe("Google Search entry point isolation", () => {
  it("keeps HTTPS suggestions while removing executable or navigation-capable markup", () => {
    const sanitized = sanitizeGoogleSearchEntryPointHtml(
      '<style>.chip{color:blue}</style><a href="https://google.com/search?q=rust" onclick="steal()">Rust</a><script>steal()</script><a href="javascript:steal()">Bad</a>',
    );

    expect(sanitized).toContain("https://google.com/search?q=rust");
    expect(sanitized).toContain("<style>");
    expect(sanitized).not.toContain("onclick");
    expect(sanitized).not.toContain("<script");
    expect(sanitized).not.toContain("javascript:");
  });
});
