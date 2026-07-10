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
  const handlerCount = (event: string) => handlers.get(event)?.size ?? 0;
  return { listen, emit, reset, handlerCount };
});

const tauriApiMocks = vi.hoisted(() => ({
  getRecordingSnapshot: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: tauriEvents.listen,
}));

vi.mock("@/api/tauri", () => ({
  copyToClipboard: vi.fn(async () => undefined),
  getRecordingSnapshot: tauriApiMocks.getRecordingSnapshot,
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
  tauriApiMocks.getRecordingSnapshot.mockReset();
  tauriApiMocks.getRecordingSnapshot.mockResolvedValue(null);
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

  it("F. shows raw-first status as raw text is replaced by polished text", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 20,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 20,
        text: "ni hao",
        interim: false,
        resultStage: "raw",
        timing: { rawFirst: { status: "pasted" } },
      });
    });

    expect(readSubtitleText(container)).toContain("ni hao");
    expect(screen.getByText("subtitle.rawFirst.pasted")).toBeInTheDocument();

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 20,
        text: "你好。",
        interim: false,
        polished: true,
        resultStage: "polished",
        timing: { rawFirst: { status: "replaced" } },
      });
    });

    expect(readSubtitleText(container)).toContain("你好。");
    expect(screen.getByText("subtitle.rawFirst.replaced")).toBeInTheDocument();
  });

  it("G. changes preview-only label after polished subtitle preview arrives", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 21,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 21,
        text: "ni hao",
        interim: false,
        resultStage: "raw",
        timing: { rawFirst: { status: "preview_only" } },
      });
    });

    expect(readSubtitleText(container)).toContain("ni hao");
    expect(screen.getByText("subtitle.rawFirst.preview_only")).toBeInTheDocument();

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 21,
        text: "你好。",
        interim: false,
        polished: true,
        resultStage: "polished",
        timing: { rawFirst: { status: "preview_only" } },
      });
    });

    expect(readSubtitleText(container)).toContain("你好。");
    expect(screen.getByText("subtitle.rawFirst.polished_preview")).toBeInTheDocument();
    expect(screen.queryByText("subtitle.rawFirst.preview_only")).not.toBeInTheDocument();
  });

  it("H. keeps raw preview visible while waiting for the polished result", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 22,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 22,
        text: "long raw preview",
        interim: false,
        resultStage: "raw",
        timing: { rawFirst: { status: "preview_only" } },
      });
    });

    await advance(5000);

    expect(readSubtitleText(container)).toContain("long raw preview");
    const capsule = container.querySelector(".subtitle-capsule");
    expect(capsule).not.toBeNull();
    expect(capsule?.classList.contains("subtitle-fade-out")).toBe(false);
    expect(screen.getByText("subtitle.rawFirst.preview_only")).toBeInTheDocument();
  });

  it("I. changes preview-only label after AI polish returns unchanged text", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 23,
        isRecording: true,
        isProcessing: false,
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 23,
        text: "unchanged text",
        interim: false,
        resultStage: "raw",
        timing: { rawFirst: { status: "preview_only" } },
      });
    });

    await act(async () => {
      tauriEvents.emit("transcription-result", {
        sessionId: 23,
        text: "unchanged text",
        interim: false,
        polished: false,
        resultStage: "polished",
        timing: { rawFirst: { status: "preview_only" } },
      });
    });

    expect(readSubtitleText(container)).toContain("unchanged text");
    expect(screen.getByText("subtitle.rawFirst.polished_preview")).toBeInTheDocument();
    expect(screen.queryByText("subtitle.rawFirst.preview_only")).not.toBeInTheDocument();
  });

  it.each(["too_short", "no_speech", "asr_error", "processing_error", "start_error"] as const)(
    "J. shows the %s terminal outcome long enough to read",
    async (outcome) => {
      const { container } = render(<SubtitleOverlay />);
      await flushAsyncListeners();

      await act(async () => {
        tauriEvents.emit("recording-state", {
          sessionId: 40,
          isRecording: false,
          isProcessing: true,
          mode: "dictation",
        });
        tauriEvents.emit("recording-outcome", {
          sessionId: 40,
          outcome,
          mode: "dictation",
          detail: "technical detail stays out of the primary UI",
        });
      });

      expect(screen.getByText(`recording.outcome.${outcome}`)).toBeInTheDocument();
      expect(screen.queryByText("technical detail stays out of the primary UI")).not.toBeInTheDocument();

      await advance(1400);
      expect(screen.getByText(`recording.outcome.${outcome}`)).toBeInTheDocument();
      expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(false);

      await advance(700);
      expect(screen.queryByText(`recording.outcome.${outcome}`)).not.toBeInTheDocument();
      expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
    },
  );

  it("K. ignores an older session outcome after a newer recording takes over", async () => {
    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 51,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 50,
        outcome: "asr_error",
        mode: "dictation",
      });
    });

    expect(screen.queryByText("recording.outcome.asr_error")).not.toBeInTheDocument();
    expect(screen.getByText("subtitle.listening")).toBeInTheDocument();
  });

  it("L. preserves partial assistant text after a processing error without presenting success actions", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("assistant-stream", {
        sessionId: 60,
        status: "started",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 60,
        chunk: "partial answer",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 60,
        outcome: "processing_error",
        mode: "assistant",
      });
    });

    expect(readSubtitleText(container)).toContain("partial answer");
    expect(screen.getByText("recording.outcome.processing_error")).toBeInTheDocument();
    expect(container.querySelector(".subtitle-capsule-assistant")).not.toBeNull();
    expect(container.querySelector(".subtitle-copy-button")).toBeNull();
  });

  it("M. ignores terminal outcomes without a valid session identity", async () => {
    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 70,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-outcome", {
        outcome: "asr_error",
        mode: "dictation",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 0,
        outcome: "processing_error",
        mode: "dictation",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: Number.NaN,
        outcome: "too_short",
        mode: "dictation",
      });
    });

    expect(screen.getByText("subtitle.listening")).toBeInTheDocument();
    expect(screen.queryByText("recording.outcome.asr_error")).not.toBeInTheDocument();
    expect(screen.queryByText("recording.outcome.processing_error")).not.toBeInTheDocument();
    expect(screen.queryByText("recording.outcome.too_short")).not.toBeInTheDocument();
  });

  it("N. cancels an old outcome cleanup when a newer recording starts", async () => {
    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-outcome", {
        sessionId: 80,
        outcome: "no_speech",
        mode: "dictation",
      });
    });
    await advance(1000);

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 81,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
    });
    await advance(2000);

    expect(screen.getByText("subtitle.listening")).toBeInTheDocument();
    expect(screen.queryByText("recording.outcome.no_speech")).not.toBeInTheDocument();
  });

  it("O. hydrates a cold-mounted starting session after both lifecycle listeners are ready", async () => {
    tauriApiMocks.getRecordingSnapshot.mockImplementationOnce(async () => {
      expect(tauriEvents.handlerCount("recording-state")).toBe(1);
      expect(tauriEvents.handlerCount("recording-outcome")).toBe(1);
      return {
        sessionId: 90,
        revision: 1,
        phase: "starting",
        mode: "dictation",
      };
    });

    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    expect(screen.getByText("subtitle.connectingMicrophone")).toBeInTheDocument();
    const bars = Array.from(container.querySelectorAll<HTMLElement>(".subtitle-waveform-indicator-bar"));
    expect(bars).toHaveLength(9);
    expect(bars.every((bar) => bar.style.height === "2px")).toBe(true);
  });

  it("P. ignores a late same-session Starting snapshot after live Recording wins", async () => {
    let resolveSnapshot!: (snapshot: unknown) => void;
    tauriApiMocks.getRecordingSnapshot.mockReturnValueOnce(new Promise((resolve) => {
      resolveSnapshot = resolve;
    }));

    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 91,
        revision: 2,
        isStarting: false,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
    });
    expect(screen.getByText("subtitle.listening")).toBeInTheDocument();

    await act(async () => {
      resolveSnapshot({
        sessionId: 91,
        revision: 1,
        phase: "starting",
        mode: "dictation",
      });
      await Promise.resolve();
    });

    expect(screen.getByText("subtitle.listening")).toBeInTheDocument();
    expect(screen.queryByText("subtitle.connectingMicrophone")).not.toBeInTheDocument();
  });

  it("Q. preserves the same nine waveform nodes from Starting through live Recording updates", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 92,
        revision: 1,
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "assistant",
      });
    });
    const startingIndicator = container.querySelector(".subtitle-waveform-indicator");
    const startingBars = Array.from(container.querySelectorAll<HTMLElement>(".subtitle-waveform-indicator-bar"));
    expect(startingBars).toHaveLength(9);
    expect(startingBars.every((bar) => bar.style.height === "2px")).toBe(true);

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 92,
        revision: 2,
        isStarting: false,
        isRecording: true,
        isProcessing: false,
        mode: "assistant",
      });
      tauriEvents.emit("waveform", {
        sessionId: 92,
        bars: [0.25, 0.5, 0.75, 1, 0.6, 0.4, 0.2, 0.1, 0],
      });
    });

    const recordingIndicator = container.querySelector(".subtitle-waveform-indicator");
    const recordingBars = Array.from(container.querySelectorAll<HTMLElement>(".subtitle-waveform-indicator-bar"));
    expect(recordingIndicator).toBe(startingIndicator);
    expect(recordingBars).toHaveLength(9);
    recordingBars.forEach((bar, index) => expect(bar).toBe(startingBars[index]));
    expect(recordingBars.map((bar) => bar.style.height)).toEqual([
      "4px", "8px", "12px", "16px", "9.6px", "6.4px", "3.2px", "2px", "2px",
    ]);
  });

  it("R. lets start_error consume the same revision as a terminal no-op state", async () => {
    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 93,
        revision: 1,
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 93,
        revision: 2,
        phase: "outcome",
        isStarting: false,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 93,
        revision: 1,
        phase: "recording",
        isStarting: false,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 93,
        revision: 2,
        outcome: "start_error",
        mode: "dictation",
        detail: "device internals stay hidden",
      });
    });

    expect(screen.getByText("recording.outcome.start_error")).toBeInTheDocument();
    expect(screen.queryByText("device internals stay hidden")).not.toBeInTheDocument();
  });

  it("S. rejects a lower-revision Recording event after Processing already advanced", async () => {
    render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 94,
        revision: 3,
        isStarting: false,
        isRecording: false,
        isProcessing: true,
        mode: "dictation",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 94,
        revision: 2,
        isStarting: false,
        isRecording: true,
        isProcessing: false,
        mode: "dictation",
      });
    });

    expect(screen.getByText("subtitle.recognizing")).toBeInTheDocument();
    expect(screen.queryByText("subtitle.listening")).not.toBeInTheDocument();
  });

  it("T. fades a quick-cancelled Starting session to idle", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 95,
        revision: 1,
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 95,
        revision: 2,
        isStarting: false,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
    });

    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
    await advance(500);
    expect(screen.queryByText("subtitle.connectingMicrophone")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".subtitle-waveform-indicator-bar")).toHaveLength(0);
  });

  it("U. clears an old outcome timer when a newer session starts connecting", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-outcome", {
        sessionId: 96,
        revision: 4,
        outcome: "start_error",
        mode: "dictation",
      });
    });
    await advance(1000);

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 97,
        revision: 1,
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
    });
    await advance(2000);

    expect(screen.getByText("subtitle.connectingMicrophone")).toBeInTheDocument();
    expect(screen.queryByText("recording.outcome.start_error")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".subtitle-waveform-indicator-bar")).toHaveLength(9);
    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(false);
  });

  it("V. keeps an idle tombstone ahead of a stale same-session Starting event", async () => {
    tauriApiMocks.getRecordingSnapshot.mockResolvedValueOnce({
      sessionId: 98,
      revision: 5,
      phase: "idle",
      mode: "dictation",
    });

    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();
    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 98,
        revision: 4,
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "dictation",
      });
    });

    expect(screen.queryByText("subtitle.connectingMicrophone")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".subtitle-waveform-indicator-bar")).toHaveLength(0);
    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
  });

  it("W. seals a terminal outcome against late same-session content events", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("assistant-stream", {
        sessionId: 99,
        status: "started",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 99,
        chunk: "useful partial",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 99,
        revision: 5,
        outcome: "processing_error",
        mode: "assistant",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 99,
        chunk: " late chunk",
      });
      tauriEvents.emit("ai-polish-status", {
        sessionId: 99,
        status: "polishing",
      });
      tauriEvents.emit("transcription-result", {
        sessionId: 99,
        text: "late success",
        interim: false,
        mode: "assistant",
      });
    });

    expect(readSubtitleText(container)).toContain("useful partial");
    expect(readSubtitleText(container)).not.toContain("late chunk");
    expect(readSubtitleText(container)).not.toContain("late success");
    expect(screen.getByText("recording.outcome.processing_error")).toBeInTheDocument();

    await advance(2000);
    expect(screen.queryByText("recording.outcome.processing_error")).not.toBeInTheDocument();
    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
  });

  it("X. seals the gap between terminal state and its same-revision outcome", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("assistant-stream", {
        sessionId: 100,
        status: "started",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 100,
        chunk: "partial before failure",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 100,
        revision: 6,
        phase: "outcome",
        isStarting: false,
        isRecording: false,
        isProcessing: false,
        mode: "assistant",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 100,
        chunk: " late gap chunk",
      });
      tauriEvents.emit("recording-outcome", {
        sessionId: 100,
        revision: 6,
        outcome: "processing_error",
        mode: "assistant",
      });
    });

    expect(readSubtitleText(container)).toContain("partial before failure");
    expect(readSubtitleText(container)).not.toContain("late gap chunk");
    expect(screen.getByText("recording.outcome.processing_error")).toBeInTheDocument();
  });

  it("Y. restores a missed start_error from a cold-mount snapshot", async () => {
    tauriApiMocks.getRecordingSnapshot.mockResolvedValueOnce({
      sessionId: 101,
      revision: 4,
      phase: "outcome",
      mode: "dictation",
      outcome: "start_error",
      detail: "private device detail",
    });

    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    expect(screen.getByText("recording.outcome.start_error")).toBeInTheDocument();
    expect(screen.queryByText("private device detail")).not.toBeInTheDocument();
    await advance(1400);
    expect(screen.getByText("recording.outcome.start_error")).toBeInTheDocument();
    await advance(600);
    expect(screen.queryByText("recording.outcome.start_error")).not.toBeInTheDocument();
    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
  });

  it("Z. keeps a quick-cancel Idle sealed against late same-session content", async () => {
    const { container } = render(<SubtitleOverlay />);
    await flushAsyncListeners();

    await act(async () => {
      tauriEvents.emit("recording-state", {
        sessionId: 102,
        revision: 1,
        phase: "starting",
        isStarting: true,
        isRecording: false,
        isProcessing: false,
        mode: "assistant",
      });
      tauriEvents.emit("recording-state", {
        sessionId: 102,
        revision: 2,
        phase: "idle",
        isStarting: false,
        isRecording: false,
        isProcessing: false,
        mode: "assistant",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 102,
        status: "started",
      });
      tauriEvents.emit("assistant-stream", {
        sessionId: 102,
        chunk: "late content",
      });
    });

    await advance(500);
    expect(readSubtitleText(container)).not.toContain("late content");
    expect(screen.queryByText("subtitle.aiGenerating")).not.toBeInTheDocument();
    expect(container.querySelector(".subtitle-capsule")?.classList.contains("subtitle-fade-out")).toBe(true);
  });
});
