import { listen } from "@tauri-apps/api/event";
import { useState, useEffect, useCallback, useRef } from "react";
import {
  getHotkeyDiagnostic,
  registerCustomHotkey,
} from "@/api/tauri";
import { DEFAULT_HOTKEY, HOTKEY_STORAGE_KEY } from "@/lib/constants";
import { formatHotkeyForDisplay, normalizeHotkey } from "@/lib/hotkey";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";
import type { HotkeyDiagnostic } from "@/types";

interface UseHotkeyReturn {

  hotkeyDisplay: string;

  setHotkey: (shortcut: string) => Promise<void>;

  error: string | null;

  diagnostic: HotkeyDiagnostic | null;
}

function readStoredHotkey(): string {
  const stored = readLocalStorage(HOTKEY_STORAGE_KEY);
  if (stored) return normalizeHotkey(stored);
  return DEFAULT_HOTKEY;
}

function writeStoredHotkey(shortcut: string): void {
  writeLocalStorage(HOTKEY_STORAGE_KEY, shortcut);
}







export function useHotkey(): UseHotkeyReturn {
  const [error, setError] = useState<string | null>(null);
  const [hotkeyRaw, setHotkeyRaw] = useState<string>(() => readStoredHotkey());
  const [diagnostic, setDiagnostic] = useState<HotkeyDiagnostic | null>(null);
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



  useEffect(() => {
    mountedRef.current = true;
    void register();
    return () => {
      mountedRef.current = false;
    };
  }, [register]);

  useEffect(() => {
    let disposed = false;
    let unlisten: null | (() => void) = null;

    void getHotkeyDiagnostic()
      .then((payload) => {
        if (!disposed) setDiagnostic(payload);
      })
      .catch(() => undefined);

    void (async () => {
      try {
        unlisten = await listen<HotkeyDiagnostic>("hotkey-diagnostic", (event) => {
          if (!disposed) setDiagnostic(event.payload);
        });

        if (disposed && unlisten) {
          unlisten();
          unlisten = null;
        }
      } catch {

      }
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return {
    hotkeyDisplay,
    setHotkey,
    error,
    diagnostic,
  };
}
