import { DEFAULT_HOTKEY } from "./constants";

export const HOTKEY_MODIFIER_ORDER = [
  "Ctrl",
  "LeftCtrl",
  "RightCtrl",
  "Alt",
  "LeftAlt",
  "RightAlt",
  "Shift",
  "Super",
] as const;

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
  capslock: "CapsLock",
  "caps lock": "CapsLock",
};

const MODIFIER_ALIASES: Record<string, HotkeyModifier> = {
  ctrl: "Ctrl",
  control: "Ctrl",
  leftctrl: "LeftCtrl",
  ctrlleft: "LeftCtrl",
  leftcontrol: "LeftCtrl",
  controlleft: "LeftCtrl",
  rightctrl: "RightCtrl",
  ctrlright: "RightCtrl",
  rightcontrol: "RightCtrl",
  controlright: "RightCtrl",
  alt: "Alt",
  option: "Alt",
  altgraph: "Alt",
  leftalt: "LeftAlt",
  altleft: "LeftAlt",
  leftoption: "LeftAlt",
  rightalt: "RightAlt",
  altright: "RightAlt",
  rightoption: "RightAlt",
  shift: "Shift",
  meta: "Super",
  super: "Super",
  win: "Super",
  cmd: "Super",
  command: "Super",
  os: "Super",
  windows: "Super",
};

const MODIFIER_DISPLAY: Partial<Record<HotkeyModifier, string>> = {
  LeftCtrl: "Left Ctrl",
  RightCtrl: "Right Ctrl",
  LeftAlt: "Left Alt",
  RightAlt: "Right Alt",
  Super: "Win",
};

function isModifierOnlyCombo(modifiers: HotkeyModifier[]): boolean {
  return modifiers.length > 0;
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
  return shortcut
    .split("+")
    .map((token) => MODIFIER_DISPLAY[token.trim() as HotkeyModifier] ?? token.trim())
    .join("+");
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
    const modifier = MODIFIER_ALIASES[lower];
    if (modifier) {
      modifiers.add(modifier);
      continue;
    }

    mainKey = normalizeMainKeyToken(token);
  }

  const orderedModifiers = HOTKEY_MODIFIER_ORDER.filter((key) =>
    modifiers.has(key)
  );

  if (!mainKey) {
    return isModifierOnlyCombo(orderedModifiers) ? orderedModifiers.join("+") : fallback;
  }

  return [...orderedModifiers, mainKey].join("+");
}

export function modifierFromKeyboardEvent(event: KeyboardEvent): HotkeyModifier | null {
  const key = event.key.toLowerCase();
  const code = event.code.toLowerCase();

  if (code === "controlleft") return "LeftCtrl";
  if (code === "controlright") return "RightCtrl";
  if (key === "control") return "Ctrl";
  if (code === "altleft") return "LeftAlt";
  if (code === "altright") return "RightAlt";
  if (key === "alt" || key === "altgraph") {
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
  const hasPhysicalCtrl = activeModifiers.has("LeftCtrl") || activeModifiers.has("RightCtrl");
  const hasPhysicalAlt = activeModifiers.has("LeftAlt") || activeModifiers.has("RightAlt");
  const altGraphActive = activeModifiers.has("RightAlt") && event.getModifierState("AltGraph");

  if (!hasPhysicalCtrl && !altGraphActive && (event.ctrlKey || event.getModifierState("Control"))) {
    modifiers.add("Ctrl");
  }
  if (
    !hasPhysicalAlt &&
    (event.altKey || event.getModifierState("Alt") || event.getModifierState("AltGraph"))
  ) {
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
