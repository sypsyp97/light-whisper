import { vi } from "vitest";

/**
 * Shared helper for mocking `@tauri-apps/api/event`. Returns a controller that
 * lets tests manually emit events to listeners registered through `listen()`.
 *
 * Usage:
 *   const events = createTauriEventController();
 *   vi.mock("@tauri-apps/api/event", () => events.module);
 *   // in test:
 *   events.emit("recording-state", { sessionId: 5, isRecording: true });
 */
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
