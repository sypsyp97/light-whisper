import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  registerCustomHotkey,
  unregisterAllHotkeys,
} from "../api/hotkey";
import { DEFAULT_HOTKEY, HOTKEY_STORAGE_KEY } from "@/lib/constants";

interface UseHotkeyReturn {
  /** Whether the hotkey is currently registered. */
  registered: boolean;
  /** A human-readable string describing the current hotkey (e.g. "F2"). */
  hotkeyDisplay: string;
  /** Manually register the current hotkey (called automatically on mount). */
  register: () => Promise<void>;
  /** Manually unregister all global hotkeys for this app. */
  unregister: () => Promise<void>;
  /** Update hotkey, persist it locally and re-register immediately. */
  setHotkey: (shortcut: string) => Promise<void>;
  /** Error message if registration failed. */
  error: string | null;
}

const MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Super"] as const;

function formatHotkeyForDisplay(shortcut: string): string {
  return shortcut.replace(/\bSuper\b/g, "Win");
}

function normalizeMainKeyToken(token: string): string {
  const value = token.trim();
  if (!value) return "";

  if (/^[a-z]$/i.test(value)) return value.toUpperCase();
  if (/^\d$/.test(value)) return value;
  if (/^f([1-9]|1\d|2[0-4])$/i.test(value)) return value.toUpperCase();

  const map: Record<string, string> = {
    escape: "Escape",
    esc: "Escape",
    enter: "Enter",
    space: "Space",
    tab: "Tab",
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

  return map[value.toLowerCase()] ?? "";
}

function normalizeHotkey(raw: string): string {
  const parts = raw
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);

  if (parts.length === 0) return DEFAULT_HOTKEY;

  const modifiers = new Set<string>();
  let mainKey = "";

  for (const token of parts) {
    const lower = token.toLowerCase();
    if (lower === "ctrl" || lower === "control") {
      modifiers.add("Ctrl");
      continue;
    }
    if (lower === "alt") {
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
      lower === "command"
    ) {
      modifiers.add("Super");
      continue;
    }
    mainKey = normalizeMainKeyToken(token);
  }

  const orderedModifiers = MODIFIER_ORDER.filter((key) => modifiers.has(key));
  if (!mainKey) {
    // Allow Ctrl+Win as a special modifier-only hotkey.
    if (
      orderedModifiers.length === 2 &&
      orderedModifiers[0] === "Ctrl" &&
      orderedModifiers[1] === "Super"
    ) {
      return "Ctrl+Super";
    }
    return DEFAULT_HOTKEY;
  }

  return [...orderedModifiers, mainKey].join("+");
}

function readStoredHotkey(): string {
  try {
    const stored = localStorage.getItem(HOTKEY_STORAGE_KEY);
    if (stored) return normalizeHotkey(stored);
  } catch {
    // localStorage may be unavailable
  }
  return DEFAULT_HOTKEY;
}

function writeStoredHotkey(shortcut: string): void {
  try {
    localStorage.setItem(HOTKEY_STORAGE_KEY, shortcut);
  } catch {
    // ignore write failures
  }
}

/**
 * React hook that manages the global push-to-talk hotkey.
 *
 * Press-to-talk: hold key to record, release to stop.
 *
 * @param onPress - Called when the hotkey is pressed (start recording).
 * @param onRelease - Called when the hotkey is released (stop recording).
 */
export function useHotkey(onPress?: () => void, onRelease?: () => void): UseHotkeyReturn {
  const [registered, setRegistered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hotkeyRaw, setHotkeyRaw] = useState<string>(() => readStoredHotkey());
  const hotkeyDisplay = formatHotkeyForDisplay(hotkeyRaw);

  const mountedRef = useRef(true);
  const hotkeyRef = useRef(hotkeyRaw);
  const onPressRef = useRef(onPress);
  const onReleaseRef = useRef(onRelease);

  hotkeyRef.current = hotkeyRaw;
  onPressRef.current = onPress;
  onReleaseRef.current = onRelease;

  const registerShortcut = useCallback(async (shortcut: string) => {
    const normalized = normalizeHotkey(shortcut);
    await registerCustomHotkey(normalized);
    if (!mountedRef.current) return normalized;
    setRegistered(true);
    setError(null);
    setHotkeyRaw(normalized);
    hotkeyRef.current = normalized;
    writeStoredHotkey(normalized);
    return normalized;
  }, []);

  const register = useCallback(async () => {
    try {
      setError(null);
      await registerShortcut(hotkeyRef.current || DEFAULT_HOTKEY);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : String(err);
      if (!mountedRef.current) return;
      setError(message);
    }
  }, [registerShortcut]);

  const unregister = useCallback(async () => {
    try {
      await unregisterAllHotkeys();
      if (!mountedRef.current) return;
      setRegistered(false);
      setError(null);
    } catch {
      // Best-effort during teardown
    }
  }, []);

  const setHotkey = useCallback(async (shortcut: string) => {
    const normalized = normalizeHotkey(shortcut);
    try {
      setError(null);
      await registerShortcut(normalized);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (mountedRef.current) {
        setError(message);
      }
      throw err;
    }
  }, [registerShortcut]);

  // Register on mount, unregister on unmount
  useEffect(() => {
    mountedRef.current = true;
    void register();
    return () => {
      mountedRef.current = false;
      void unregisterAllHotkeys().catch(() => undefined);
    };
  }, [register]);

  // Listen for hotkey press/release events
  useEffect(() => {
    let disposed = false;
    let unlistenPress: (() => void) | null = null;
    let unlistenRelease: (() => void) | null = null;

    void (async () => {
      try {
        const [pressUnlisten, releaseUnlisten] = await Promise.all([
          listen("hotkey-press", () => {
            onPressRef.current?.();
          }),
          listen("hotkey-release", () => {
            onReleaseRef.current?.();
          }),
        ]);

        if (disposed) {
          pressUnlisten();
          releaseUnlisten();
          return;
        }

        unlistenPress = pressUnlisten;
        unlistenRelease = releaseUnlisten;
      } catch (err) {
        if (!mountedRef.current) return;
        const message = err instanceof Error ? err.message : "监听热键事件失败";
        setError(message);
      }
    })();

    return () => {
      disposed = true;
      unlistenPress?.();
      unlistenRelease?.();
    };
  }, []);

  return {
    registered,
    hotkeyDisplay,
    register,
    unregister,
    setHotkey,
    error,
  };
}
