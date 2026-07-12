import { useCallback, useEffect, useMemo, useRef } from "react";

interface UseDebouncedCallbackOptions {
  onUnmount?: "cancel" | "flush";
}

export function useDebouncedCallback<TArgs extends unknown[]>(
  callback: (...args: TArgs) => void | Promise<void>,
  delayMs: number,
  options?: UseDebouncedCallbackOptions,
) {
  const callbackRef = useRef(callback);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingArgsRef = useRef<TArgs | null>(null);
  const tailRef = useRef<Promise<void> | null>(null);

  callbackRef.current = callback;

  const cancel = useCallback(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    pendingArgsRef.current = null;
  }, []);

  const invoke = useCallback((args: TArgs): Promise<void> => {
    const currentCallback = callbackRef.current;
    const run = (): Promise<void> => {
      try {
        return Promise.resolve(currentCallback(...args));
      } catch (error) {
        return Promise.reject(error);
      }
    };
    const previous = tailRef.current;
    const running = previous === null
      ? run()
      : previous.then(run, run);
    tailRef.current = running;
    void running.then(
      () => {
        if (tailRef.current === running) tailRef.current = null;
      },
      () => {
        if (tailRef.current === running) tailRef.current = null;
      },
    );
    return running;
  }, []);

  const flush = useCallback(async (): Promise<void> => {
    if (timerRef.current === null || pendingArgsRef.current === null) {
      await tailRef.current;
      return;
    }
    const args = pendingArgsRef.current;
    clearTimeout(timerRef.current);
    timerRef.current = null;
    pendingArgsRef.current = null;
    await invoke(args);
  }, [invoke]);

  const schedule = useCallback((...args: TArgs) => {
    cancel();
    pendingArgsRef.current = args;
    timerRef.current = setTimeout(() => {
      const pendingArgs = pendingArgsRef.current;
      timerRef.current = null;
      pendingArgsRef.current = null;
      if (pendingArgs !== null) {
        void invoke(pendingArgs).catch(() => undefined);
      }
    }, delayMs);
  }, [cancel, delayMs, invoke]);

  useEffect(() => {
    return () => {
      if (options?.onUnmount === "flush") {
        void flush().catch(() => undefined);
      } else {
        cancel();
      }
    };
  }, [cancel, flush, options?.onUnmount]);

  return useMemo(() => ({ schedule, cancel, flush }), [schedule, cancel, flush]);
}
