import { describe, expect, it } from "vitest";
import { searchNoticeKey } from "./searchNotice";

describe("search failure notices", () => {
  it.each([
    ["SEARCH_RATE_LIMITED", "searchRateLimited"],
    ["SEARCH_AUTH_REQUIRED", "searchAuthRequired"],
    ["SEARCH_ACCESS_DENIED", "searchAccessDenied"],
    ["SEARCH_TIMEOUT", "searchTimeout"],
    ["SEARCH_INVALID_RESPONSE", "searchInvalidResponse"],
    ["SEARCH_PROVIDER_ERROR", "searchFailed"],
    [undefined, "searchFailed"],
  ])("localizes %s without exposing raw diagnostics", (code, key) => {
    expect(searchNoticeKey(code)).toBe(`subtitle.conversation.${key}`);
  });
});
