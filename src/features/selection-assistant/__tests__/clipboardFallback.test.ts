import { describe, expect, it, vi } from "vitest";

import {
  readSelectedText,
  shouldRestoreClipboard,
  type SelectionReadPort,
} from "../clipboardFallback";

function createPort(
  overrides: Partial<SelectionReadPort> = {},
): SelectionReadPort {
  return {
    readDirect: vi.fn().mockResolvedValue(null),
    snapshotClipboard: vi.fn().mockResolvedValue({ formats: ["text"], data: "before" }),
    requestCopy: vi.fn().mockResolvedValue(undefined),
    readClipboardText: vi.fn().mockResolvedValue("copied selection"),
    restoreClipboard: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

describe("readSelectedText", () => {
  it("uses UI Automation without touching the clipboard when direct reading succeeds", async () => {
    const port = createPort({ readDirect: vi.fn().mockResolvedValue(" direct text ") });

    await expect(readSelectedText(port)).resolves.toEqual({
      kind: "text",
      text: "direct text",
      source: "direct",
      truncated: false,
    });
    expect(port.snapshotClipboard).not.toHaveBeenCalled();
    expect(port.requestCopy).not.toHaveBeenCalled();
  });

  it("falls back to a copy transaction and restores the complete clipboard snapshot", async () => {
    const snapshot = { formats: ["html", "text"], data: "opaque snapshot" };
    const port = createPort({
      snapshotClipboard: vi.fn().mockResolvedValue(snapshot),
      readClipboardText: vi
        .fn()
        .mockResolvedValueOnce("copied selection")
        .mockResolvedValueOnce("copied selection"),
    });

    await expect(readSelectedText(port)).resolves.toMatchObject({
      kind: "text",
      text: "copied selection",
      source: "clipboard",
    });
    expect(port.restoreClipboard).toHaveBeenCalledWith(snapshot);
  });

  it("restores the clipboard even when the synthetic copy operation fails", async () => {
    const port = createPort({
      requestCopy: vi.fn().mockRejectedValue(new Error("access denied")),
    });

    await expect(readSelectedText(port)).resolves.toEqual({
      kind: "unavailable",
      reason: "copy_failed",
    });
    expect(port.restoreClipboard).toHaveBeenCalledTimes(1);
  });

  it("does not overwrite a clipboard value the user changed during the transaction", async () => {
    const port = createPort({
      readClipboardText: vi
        .fn()
        .mockResolvedValueOnce("copied selection")
        .mockResolvedValueOnce("new user value"),
    });

    await expect(readSelectedText(port)).resolves.toMatchObject({ kind: "text" });
    expect(port.restoreClipboard).not.toHaveBeenCalled();
  });

  it("returns unavailable instead of throwing for a restricted incompatible application", async () => {
    const port = createPort({
      readDirect: vi.fn().mockRejectedValue(new Error("UIA unavailable")),
      requestCopy: vi.fn().mockRejectedValue(new Error("copy blocked")),
    });

    await expect(readSelectedText(port)).resolves.toEqual({
      kind: "unavailable",
      reason: "copy_failed",
    });
  });
});

describe("shouldRestoreClipboard", () => {
  it("restores only while the temporary copied text still owns the clipboard", () => {
    expect(shouldRestoreClipboard("selected", "selected")).toBe(true);
    expect(shouldRestoreClipboard("user update", "selected")).toBe(false);
    expect(shouldRestoreClipboard(null, "selected")).toBe(false);
  });
});
