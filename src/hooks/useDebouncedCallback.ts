import { useCallback, useEffect, useEffectEvent, useRef } from "react";

interface UseDebouncedCallbackOptions {
  onUnmount?: "cancel" | "flush";
}

export function useDebouncedCallback<TArgs extends unknown[]>(
  callback: (...args: TArgs) => void | Promise<void>,
  delayMs: number,
  options?: UseDebouncedCallbackOptions,
) {
  const callbackEvent = useEffectEvent(callback);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingArgsRef = useRef<TArgs | null>(null);

  const cancel = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    pendingArgsRef.current = null;
  }, []);

  const flush = useCallback(() => {
    if (timerRef.current === null || pendingArgsRef.current === null) {
      return;
    }
    const args = pendingArgsRef.current;
    clearTimeout(timerRef.current);
    timerRef.current = null;
    pendingArgsRef.current = null;
    void callbackEvent(...args);
  }, [callbackEvent]);

  const schedule = useCallback((...args: TArgs) => {
    cancel();
    pendingArgsRef.current = args;
    timerRef.current = setTimeout(() => {
      const pendingArgs = pendingArgsRef.current;
      timerRef.current = null;
      pendingArgsRef.current = null;
      if (pendingArgs !== null) {
        void callbackEvent(...pendingArgs);
      }
    }, delayMs);
  }, [callbackEvent, cancel, delayMs]);

  useEffect(() => {
    return () => {
      if (options?.onUnmount === "flush") {
        flush();
      } else {
        cancel();
      }
    };
  }, [cancel, flush, options?.onUnmount]);

  return { schedule, cancel, flush };
}
