import { vi } from "vitest";











export function createTauriEventController() {
  type Handler = (event: { payload: unknown }) => void;
  const handlers = new Map<string, Set<Handler>>();

  const listen = vi.fn(
    async (event: string, handler: Handler) => {
      let set = handlers.get(event);
      if (!set) {
        set = new Set();
        handlers.set(event, set);
      }
      set.add(handler);
      return () => {
        set?.delete(handler);
      };
    }
  );

  const emit = (event: string, payload: unknown) => {
    const set = handlers.get(event);
    if (!set) return;
    for (const h of Array.from(set)) {
      h({ payload });
    }
  };

  const reset = () => {
    handlers.clear();
    listen.mockClear();
  };

  return {
    listen,
    emit,
    reset,
    module: {
      listen,
    },
  };
}
