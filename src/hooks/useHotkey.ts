import { useState, useEffect, useCallback, useRef } from "react";
import {
  registerCustomHotkey,
  unregisterAllHotkeys,
} from "@/api/tauri";
import { DEFAULT_HOTKEY, HOTKEY_STORAGE_KEY } from "@/lib/constants";
import { formatHotkeyForDisplay, normalizeHotkey } from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

interface UseHotkeyReturn {
  /** A human-readable string describing the current hotkey (e.g. "F2"). */
  hotkeyDisplay: string;
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
 * Registers and manages the global push-to-talk hotkey.
 *
 * Recording start/stop is handled in the Rust backend so the UI only needs
 * to keep the registration in sync and surface any registration errors.
 */
export function useHotkey(): UseHotkeyReturn {
  const [error, setError] = useState<string | null>(null);
  const [hotkeyRaw, setHotkeyRaw] = useState<string>(() => readStoredHotkey());
  const hotkeyDisplay = formatHotkeyForDisplay(hotkeyRaw);

  const mountedRef = useRef(true);
  const hotkeyRef = useRef(hotkeyRaw);

  hotkeyRef.current = hotkeyRaw;

  const registerShortcut = useCallback(async (shortcut: string) => {
    const normalized = normalizeHotkey(shortcut);
    await registerCustomHotkey(normalized);
    if (!mountedRef.current) return normalized;
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

  return {
    hotkeyDisplay,
    setHotkey,
    error,
  };
}
