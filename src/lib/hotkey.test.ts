import { describe, expect, it } from "vitest";
import {
  formatHotkeyForDisplay,
  keyboardEventToHotkey,
  modifierFromKeyboardEvent,
  normalizeHotkey,
  type HotkeyModifier,
} from "@/lib/hotkey";

function keyboardEvent(
  key: string,
  code: string,
  init: KeyboardEventInit = {}
): KeyboardEvent {
  return new KeyboardEvent("keydown", { key, code, ...init });
}

describe("hotkey normalization", () => {
  it.each([
    ["ControlLeft", "LeftCtrl"],
    ["ControlRight", "RightCtrl"],
    ["AltLeft", "LeftAlt"],
    ["AltRight", "RightAlt"],
  ] as const)("preserves the physical side for %s", (code, expected) => {
    const key = code.startsWith("Control") ? "Control" : "Alt";
    expect(modifierFromKeyboardEvent(keyboardEvent(key, code))).toBe(expected);
  });

  it("keeps legacy generic modifier shortcuts compatible", () => {
    expect(normalizeHotkey("control+option+f2")).toBe("Ctrl+Alt+F2");
  });

  it("normalizes side-specific aliases and Caps Lock", () => {
    expect(normalizeHotkey("ctrlleft+rightalt+caps lock")).toBe(
      "LeftCtrl+RightAlt+CapsLock"
    );
  });

  it("captures a side-specific modifier with a main key", () => {
    const active = new Set<HotkeyModifier>(["RightCtrl"]);
    expect(keyboardEventToHotkey(keyboardEvent("r", "KeyR", { ctrlKey: true }), active))
      .toBe("RightCtrl+R");
  });

  it("formats side-specific modifiers for display", () => {
    expect(formatHotkeyForDisplay("LeftCtrl+RightAlt+CapsLock"))
      .toBe("Left Ctrl+Right Alt+CapsLock");
  });
});
