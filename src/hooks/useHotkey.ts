import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { registerF2Hotkey, unregisterF2Hotkey } from "../api/hotkey";

interface UseHotkeyReturn {
  /** Whether the F2 hotkey is currently registered. */
  registered: boolean;
  /** A human-readable string describing the hotkey (e.g. "F2"). */
  hotkeyDisplay: string;
  /** Manually register the hotkey (called automatically on mount). */
  register: () => Promise<void>;
  /** Manually unregister the hotkey. */
  unregister: () => Promise<void>;
  /** Error message if registration failed. */
  error: string | null;
}

/**
 * React hook that manages the global F2 push-to-talk hotkey.
 *
 * - Registers the hotkey on mount and unregisters on unmount.
 * - Listens for the `toggle-recording` Tauri event emitted by the Rust backend
 *   when the user presses F2.
 * - Accepts an optional callback that fires on each press event.
 *
 * @param onTrigger - Callback invoked when the hotkey is activated.
 */
export function useHotkey(onTrigger?: () => void): UseHotkeyReturn {
  const [registered, setRegistered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const onTriggerRef = useRef(onTrigger);
  onTriggerRef.current = onTrigger;

  const register = useCallback(async () => {
    try {
      setError(null);
      // Rust returns a plain string on success, throws on failure
      await registerF2Hotkey();
      setRegistered(true);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : String(err);
      setError(message);
    }
  }, []);

  const unregister = useCallback(async () => {
    try {
      await unregisterF2Hotkey();
      setRegistered(false);
      setError(null);
    } catch {
      // Best-effort during teardown
    }
  }, []);

  // Register on mount, unregister on unmount
  useEffect(() => {
    register();
    return () => {
      unregisterF2Hotkey().catch(() => {});
    };
  }, [register]);

  // Listen for the toggle-recording event from the Rust backend
  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const setup = async () => {
      unlisten = await listen("toggle-recording", () => {
        onTriggerRef.current?.();
      });
    };

    setup();

    return () => {
      unlisten?.();
    };
  }, []);

  return {
    registered,
    hotkeyDisplay: "F2",
    register,
    unregister,
    error,
  };
}
