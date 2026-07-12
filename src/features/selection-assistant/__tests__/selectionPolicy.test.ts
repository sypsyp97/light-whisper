import { describe, expect, it } from "vitest";

import {
  DEFAULT_MAX_SELECTION_CHARS,
  createSelectionEventGate,
  normalizeSelectionText,
} from "../selectionPolicy";

describe("normalizeSelectionText", () => {
  it.each(["", "   ", "\r\n\t"])('rejects an empty selection: %j', (text) => {
    expect(normalizeSelectionText(text)).toBeNull();
  });

  it("normalizes line endings and trims only the outer whitespace", () => {
    expect(normalizeSelectionText("  first\r\n  second  \r\n")).toEqual({
      text: "first\n  second",
      truncated: false,
      originalChars: 14,
    });
  });

  it("caps oversized selections by Unicode code points without splitting an emoji", () => {
    const text = `${"a".repeat(DEFAULT_MAX_SELECTION_CHARS - 1)}😀tail`;

    const normalized = normalizeSelectionText(text);

    expect(normalized).not.toBeNull();
    expect(Array.from(normalized!.text)).toHaveLength(DEFAULT_MAX_SELECTION_CHARS);
    expect(normalized!.text.endsWith("😀")).toBe(true);
    expect(normalized!.truncated).toBe(true);
    expect(normalized!.originalChars).toBe(DEFAULT_MAX_SELECTION_CHARS + 4);
  });
});

describe("createSelectionEventGate", () => {
  const completeEvent = {
    text: "selected text",
    phase: "complete" as const,
    sourceProcess: "notepad.exe",
    target: "external" as const,
    screenshotActive: false,
  };

  it("accepts a completed external selection", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept(completeEvent, 1_000)).toBe(true);
  });

  it("ignores drag-progress events until mouse-up produces a completed selection", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept({ ...completeEvent, phase: "dragging" }, 1_000)).toBe(false);
    expect(gate.accept(completeEvent, 1_010)).toBe(true);
  });

  it("deduplicates identical hook events inside the debounce window", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept(completeEvent, 1_000)).toBe(true);
    expect(gate.accept(completeEvent, 1_100)).toBe(false);
    expect(gate.accept(completeEvent, 1_251)).toBe(true);
  });

  it("does not collapse same text selected in a different application", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept(completeEvent, 1_000)).toBe(true);
    expect(
      gate.accept({ ...completeEvent, sourceProcess: "code.exe" }, 1_050),
    ).toBe(true);
  });

  it("ignores selections originating inside the toolbar", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept({ ...completeEvent, target: "toolbar" }, 1_000)).toBe(false);
  });

  it("suppresses hook events while a screenshot selection owns the pointer", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept({ ...completeEvent, screenshotActive: true }, 1_000)).toBe(false);
    expect(gate.accept(completeEvent, 1_010)).toBe(true);
  });

  it("does not remember rejected empty selections as duplicates", () => {
    const gate = createSelectionEventGate({ dedupeWindowMs: 250 });

    expect(gate.accept({ ...completeEvent, text: "  " }, 1_000)).toBe(false);
    expect(gate.accept(completeEvent, 1_010)).toBe(true);
  });
});
