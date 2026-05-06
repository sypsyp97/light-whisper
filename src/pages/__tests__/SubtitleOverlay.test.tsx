/**
 * Regression suite for the "stale flash" bug in SubtitleOverlay.
 *
 * Background:
 *   After a non-assistant final `transcription-result` arrives, the capsule
 *   shows the result for ~2s then runs a CSS fade-out. Until the bug fix,
 *   React state was never cleared after the fade — `text` and `phase` kept
 *   stale values, so the next time the (still-alive, just-hidden) Tauri
 *   subtitle window was re-shown there was a one-frame race where the
 *   previous result flashed before the new state committed.
 *
 *   These tests pin down the contract:
 *     - Fade still happens (regression guard, was already working).
 *     - After fade completes, the component MUST reset to "idle".
 *     - The reset MUST be guarded by the latest-session id rule, so that a
 *       stale cleanup timer scheduled by an older session can't clobber a
 *       newer session that has already taken over the window.
 *
 *   We import only the default export of `SubtitleOverlay` and hard-code the
 *   timer outer bounds (1000 / 2200 / 5000 ms). Per-case comments justify
 *   each chosen advance value against the spec; we deliberately do NOT
 *   import constants from the component so the test acts as an external
 *   spec assertion rather than a tautology.
 */
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Hoisted: shared handler registry for the mocked `@tauri-apps/api/event`.
// Pattern copied verbatim from src/hooks/__tests__/useRecording.test.tsx.
const tauriEvents = vi.hoisted(() => {
  type Handler = (event: { payload: unknown }) => void;
  const handlers = new Map<string, Set<Handler>>();
  const listen = async (event: string, handler: Handler) => {
    let set = handlers.get(event);
    if (!set) {
      set = new Set();
      handlers.set(event, set);
    }
    set.add(handler);
    return () => {
      set?.delete(handler);
    };
  };
  const emit = (event: string, payload: unknown) => {
    const set = handlers.get(event);
    if (!set) return;
    for (const h of Array.from(set)) h({ payload });
  };
  const reset = () => handlers.clear();
  return { listen, emit, reset };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvents.listen,
}));

vi.mock("@/api/tauri", () => ({
  copyToClipboard: vi.fn(async () => undefined),
  hideSubtitleWindow: vi.fn(async () => undefined),
}));

vi.mock("react-i18next", () => {
  const useTranslation = () => ({
    t: (key: string) => key,
    i18n: { changeLanguage: vi.fn() },
  });
  return {
    useTranslation,
    default: { useTranslation },
  };
});

vi.mock("@/i18n", () => ({
  default: {
    t: (key: string) => key,
    changeLanguage: vi.fn(),
  },
}));

// Bypass the rAF-driven character-by-character reveal so that text appears
// immediately under fake timers. We're testing cleanup/fade lifecycle, not
// streaming animation, so this is safe.
vi.mock("@/hooks/useSmoothText", () => ({
  useSmoothText: (source: string) => source,
  segmentGraphemes: (text: string) =>
    text ? Array.from(text) : ([] as string[]),
}));

import SubtitleOverlay from "@/pages/SubtitleOverlay";

beforeEach(() => {
  tauriEvents.reset();
  // jsdom does not provide window.matchMedia; SubtitleOverlay's theme effect
  // calls it on mount and registers a "change" listener.
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: () => ({
      matches: false,
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn(),
    }),
  });
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

/** Wait for `listen()` async wiring inside each useEffect to settle. */
async function flushAsyncListeners() {
  await act(async () => {
    // Several rounds because each event listener registers via an awaited
    // promise inside an IIFE; we need enough microtask turns for them all.
    for (let i = 0; i < 8; i++) {
      await Promise.resolve();
    }
  });
}

/**
 * Advance fake timers (which also tick rAF, used by useSmoothText to drain
 * the streamed text) and let chained microtasks flush.
 */
async function advance(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
}

/**
 * The component renders each grapheme in its own `<span class="stream-char">`,
 * so testing-library's per-element text matcher cannot see the joined string.
 * Read the rendered text directly off `.subtitle-text` instead.
 */
function readSubtitleText(container: HTMLElement): string {
  const node = container.querySelector(".subtitle-text");
  return node?.textContent ?? "";
}

describe("SubtitleOverlay stale-flash cleanup", () => {
  it("A. clears state after the fade completes", async () => {
    // Cleanup target: idle reset must occur after fade delay (~2000ms) +
    // animation (~300ms) + small buffer. 5000ms is well past any plausible
    // total, so cleanup MUST have fired by then if it is implemented.
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 10,
        text: "hello world",
        interim: false,
      });
    });

    expect(readSubtitleText(container)).toContain("hello world");

    await advance(5000);

    expect(readSubtitleText(container)).not.toContain("hello world");
    const capsule = container.querySelector(".subtitle-capsule");
    expect(capsule).not.toBeNull();
    expect(capsule?.classList.contains("subtitle-fade-out")).toBe(false);
    expect(container.querySelector(".subtitle-dot-recording")).toBeNull();
  });

  it("B. does not clear or fade prematurely", async () => {
    // 1000ms is well before the 2000ms fade delay, so neither fade-out nor
    // cleanup may have fired yet — text must still be on screen and the
    // capsule must NOT have the fade-out class.
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 10,
        text: "hello world",
        interim: false,
      });
    });

    expect(readSubtitleText(container)).toContain("hello world");

    await advance(1000);

    expect(readSubtitleText(container)).toContain("hello world");
    const capsule = container.querySelector(".subtitle-capsule");
    expect(capsule).not.toBeNull();
    expect(capsule?.classList.contains("subtitle-fade-out")).toBe(false);
  });

  it("C. fade-out class applies before cleanup fires", async () => {
    // 2200ms is past the 2000ms fade delay (so the fade-out class must be
    // on the capsule) but well before 2000+300+buffer, so cleanup must NOT
    // have removed the text yet.
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 10,
        text: "hello world",
        interim: false,
      });
    });

    await advance(2200);

    const capsule = container.querySelector(".subtitle-capsule");
    expect(capsule).not.toBeNull();
    expect(capsule?.classList.contains("subtitle-fade-out")).toBe(true);
    expect(readSubtitleText(container)).toContain("hello world");
  });

  it("D. session-10 cleanup must not clobber a session-11 takeover", async () => {
    // After 5000ms any session-10 cleanup timer would have fired; the
    // regression guard is that its cleanup callback must NOT reset a newer
    // (session 11) state that already took over the window.
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    // Session 10 records and finalises.
    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });
    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 10,
        text: "hello world",
        interim: false,
      });
    });

    // Session 11 takes over and starts recording (clears text, sets phase
    // back to "recording").
    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 11,
        isRecording: true,
        isProcessing: false,
      });
    });

    await advance(5000);

    // The session-10 final text must not still be on screen — but more
    // importantly the new session 11 must still be in a "recording" phase.
    expect(readSubtitleText(container)).not.toContain("hello world");

    const recordingDot = container.querySelector(".subtitle-dot-recording");
    const waveform = container.querySelector(".subtitle-waveform-indicator");
    const listeningHint = screen.queryByText("subtitle.listening");
    const stillRecording =
      recordingDot !== null || waveform !== null || listeningHint !== null;
    expect(stillRecording).toBe(true);
  });

  it("E. empty final text does not crash and still runs cleanly", async () => {
    // Empty-final path uses a different branch (sets fadingOut=true with
    // no text). 5000ms drains any timers. We only need: no throw, component
    // still mounted.
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 10,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 10,
        text: "",
        interim: false,
      });
    });

    await advance(5000);

    expect(container.querySelector(".subtitle-root")).not.toBeNull();
  });
});
