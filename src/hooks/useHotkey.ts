import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  registerCustomHotkey,
  unregisterAllHotkeys,
} from "../api/hotkey";
import { DEFAULT_HOTKEY, HOTKEY_STORAGE_KEY } from "@/lib/constants";
import { formatHotkeyForDisplay, normalizeHotkey } from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

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

function readStoredHotkey(): string {
  const stored = readLocalStorage(HOTKEY_STORAGE_KEY);
  if (stored) return normalizeHotkey(stored);
  return DEFAULT_HOTKEY;
}

function writeStoredHotkey(shortcut: string): void {
  writeLocalStorage(HOTKEY_STORAGE_KEY, shortcut);
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
