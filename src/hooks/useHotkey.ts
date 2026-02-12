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
 * Press-to-talk: hold F2 to record, release to stop.
 *
 * @param onPress  - Called when the hotkey is pressed (start recording).
 * @param onRelease - Called when the hotkey is released (stop recording).
 */
export function useHotkey(onPress?: () => void, onRelease?: () => void): UseHotkeyReturn {
  const [registered, setRegistered] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const onPressRef = useRef(onPress);
  const onReleaseRef = useRef(onRelease);
  onPressRef.current = onPress;
  onReleaseRef.current = onRelease;

  const register = useCallback(async () => {
    try {
      setError(null);
      await registerF2Hotkey();
      if (!mountedRef.current) return;
      setRegistered(true);
    } catch (err) {
      const message =
        err instanceof Error ? err.message : String(err);
      if (!mountedRef.current) return;
      setError(message);
    }
  }, []);

  const unregister = useCallback(async () => {
    try {
      await unregisterF2Hotkey();
      if (!mountedRef.current) return;
      setRegistered(false);
      setError(null);
    } catch {
      // Best-effort during teardown
    }
  }, []);

  // Register on mount, unregister on unmount
  useEffect(() => {
    mountedRef.current = true;
    void register();
    return () => {
      mountedRef.current = false;
      void unregisterF2Hotkey().catch(() => undefined);
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
    hotkeyDisplay: "F2",
    register,
    unregister,
    error,
  };
}
