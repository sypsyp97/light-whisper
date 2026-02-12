import { DEFAULT_HOTKEY } from "./constants";

export const HOTKEY_MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Super"] as const;

export type HotkeyModifier = (typeof HOTKEY_MODIFIER_ORDER)[number];

const NAMED_KEY_ALIASES: Record<string, string> = {
  escape: "Escape",
  esc: "Escape",
  enter: "Enter",
  tab: "Tab",
  " ": "Space",
  space: "Space",
  backspace: "Backspace",
  delete: "Delete",
  insert: "Insert",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  arrowup: "ArrowUp",
  up: "ArrowUp",
  arrowdown: "ArrowDown",
  down: "ArrowDown",
  arrowleft: "ArrowLeft",
  left: "ArrowLeft",
  arrowright: "ArrowRight",
  right: "ArrowRight",
};

function isCtrlSuperOnly(modifiers: HotkeyModifier[]): boolean {
  return (
    modifiers.length === 2 &&
    modifiers[0] === "Ctrl" &&
    modifiers[1] === "Super"
  );
}

function normalizeMainKeyToken(token: string): string {
  const value = token.trim();
  if (!value) return "";

  if (/^[a-z]$/i.test(value)) return value.toUpperCase();
  if (/^\d$/.test(value)) return value;
  if (/^f([1-9]|1\d|2[0-4])$/i.test(value)) return value.toUpperCase();

  return NAMED_KEY_ALIASES[value.toLowerCase()] ?? "";
}

export function formatHotkeyForDisplay(shortcut: string): string {
  return shortcut.replace(/\bSuper\b/g, "Win");
}

export function normalizeHotkey(raw: string, fallback = DEFAULT_HOTKEY): string {
  const parts = raw
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);

  if (parts.length === 0) return fallback;

  const modifiers = new Set<HotkeyModifier>();
  let mainKey = "";

  for (const token of parts) {
    const lower = token.toLowerCase();
    if (lower === "ctrl" || lower === "control") {
      modifiers.add("Ctrl");
      continue;
    }
    if (lower === "alt" || lower === "option" || lower === "altgraph") {
      modifiers.add("Alt");
      continue;
    }
    if (lower === "shift") {
      modifiers.add("Shift");
      continue;
    }
    if (
      lower === "meta" ||
      lower === "super" ||
      lower === "win" ||
      lower === "cmd" ||
      lower === "command" ||
      lower === "os" ||
      lower === "windows"
    ) {
      modifiers.add("Super");
      continue;
    }

    mainKey = normalizeMainKeyToken(token);
  }

  const orderedModifiers = HOTKEY_MODIFIER_ORDER.filter((key) =>
    modifiers.has(key)
  );

  if (!mainKey) {
    return isCtrlSuperOnly(orderedModifiers) ? "Ctrl+Super" : fallback;
  }

  return [...orderedModifiers, mainKey].join("+");
}

export function modifierFromKeyboardEvent(event: KeyboardEvent): HotkeyModifier | null {
  const key = event.key.toLowerCase();
  const code = event.code.toLowerCase();

  if (key === "control" || code === "controlleft" || code === "controlright") {
    return "Ctrl";
  }
  if (key === "alt" || key === "altgraph" || code === "altleft" || code === "altright") {
    return "Alt";
  }
  if (key === "shift" || code === "shiftleft" || code === "shiftright") {
    return "Shift";
  }
  if (
    key === "meta" ||
    key === "os" ||
    key === "win" ||
    code === "metaleft" ||
    code === "metaright"
  ) {
    return "Super";
  }

  return null;
}

function collectModifiers(
  event: KeyboardEvent,
  activeModifiers: Set<HotkeyModifier>
): Set<HotkeyModifier> {
  const modifiers = new Set<HotkeyModifier>(activeModifiers);

  if (event.ctrlKey || event.getModifierState("Control")) modifiers.add("Ctrl");
  if (event.altKey || event.getModifierState("Alt") || event.getModifierState("AltGraph")) {
    modifiers.add("Alt");
  }
  if (event.shiftKey || event.getModifierState("Shift")) modifiers.add("Shift");
  if (event.metaKey || event.getModifierState("Meta") || event.getModifierState("OS")) {
    modifiers.add("Super");
  }

  return modifiers;
}

function eventMainKey(event: KeyboardEvent): string {
  if (/^Key[A-Z]$/.test(event.code)) {
    return event.code.slice(3);
  }
  if (/^Digit[0-9]$/.test(event.code)) {
    return event.code.slice(5);
  }
  if (/^F([1-9]|1\d|2[0-4])$/.test(event.key.toUpperCase())) {
    return event.key.toUpperCase();
  }
  return NAMED_KEY_ALIASES[event.key.toLowerCase()] ?? "";
}

export function keyboardEventToHotkey(
  event: KeyboardEvent,
  activeModifiers: Set<HotkeyModifier>
): string | null {
  if (modifierFromKeyboardEvent(event)) return null;

  const mainKey = eventMainKey(event);
  if (!mainKey) return null;

  const modifiers = collectModifiers(event, activeModifiers);
  const parts: string[] = HOTKEY_MODIFIER_ORDER.filter((modifier) =>
    modifiers.has(modifier)
  );
  parts.push(mainKey);

  return parts.join("+");
}
