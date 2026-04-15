import { useEffect, useRef, useState } from "react";

interface SmoothTextOptions {
  /** Graphemes advanced per 16.67ms baseline frame. */
  charsPerFrame?: number;
  /** Snap instantly when more than this many graphemes behind. */
  snapThreshold?: number;
  /** Maximum catch-up multiplier when far behind. */
  maxCatchup?: number;
}

const DEFAULTS: Required<SmoothTextOptions> = {
  charsPerFrame: 2.5,
  snapThreshold: 240,
  maxCatchup: 5,
};

function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" &&
    !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
}

/** Grapheme-aware split: keeps emoji + ZWJ sequences + CJK whole. */
export function segmentGraphemes(text: string): string[] {
  if (!text) return [];
  if (typeof Intl !== "undefined" && "Segmenter" in Intl) {
    try {
      const seg = new (Intl as typeof Intl & {
        Segmenter: new (l?: string, o?: { granularity: "grapheme" }) => {
          segment: (s: string) => Iterable<{ segment: string }>;
        };
      }).Segmenter(undefined, { granularity: "grapheme" });
      return Array.from(seg.segment(text), (s) => s.segment);
    } catch {
      // fall through
    }
  }
  return Array.from(text); // handles BMP surrogate pairs but not ZWJ joiners
}

/**
 * Smooth character-by-character reveal of streaming text.
 * - Drains incoming source at ~60fps, grapheme-aware so emoji and CJK never split.
 * - Snaps immediately on reset, divergence, very-large jumps, or reduced-motion.
 * - Catches up faster as the backlog grows, so latency stays bounded.
 */
export function useSmoothText(source: string, options: SmoothTextOptions = {}): string {
  const { charsPerFrame, snapThreshold, maxCatchup } = { ...DEFAULTS, ...options };

  const [display, setDisplay] = useState(source);
  const displayRef = useRef(source);
  const graphemesRef = useRef<string[]>(segmentGraphemes(source));
  const drawnRef = useRef<number>(graphemesRef.current.length);
  const rafRef = useRef<number | null>(null);
  const lastTimeRef = useRef<number | null>(null);
  const fracRef = useRef(0);

  useEffect(() => {
    displayRef.current = display;
  }, [display]);

  useEffect(() => {
    const cur = displayRef.current;

    // Source cleared or diverged from the current prefix → snap.
    if (!source.startsWith(cur) || source.length < cur.length) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      lastTimeRef.current = null;
      fracRef.current = 0;
      graphemesRef.current = segmentGraphemes(source);
      drawnRef.current = graphemesRef.current.length;
      displayRef.current = source;
      setDisplay(source);
      return;
    }

    // Already caught up.
    if (source === cur) {
      graphemesRef.current = segmentGraphemes(source);
      drawnRef.current = graphemesRef.current.length;
      return;
    }

    // Refresh grapheme view with the latest source (prefix is stable).
    const nextGraphemes = segmentGraphemes(source);
    graphemesRef.current = nextGraphemes;
    // Prefix stability assumption holds, but clamp just in case Segmenter
    // collapses previously-drawn graphemes (e.g., partial ZWJ sequence).
    if (drawnRef.current > nextGraphemes.length) {
      drawnRef.current = nextGraphemes.length;
    }

    const behind = nextGraphemes.length - drawnRef.current;
    if (behind <= 0) {
      // Same (or fewer) graphemes than we've drawn, but the *content* of an
      // already-drawn grapheme may have changed — e.g. a ZWJ joiner or
      // combining mark just arrived and merged into the last grapheme. Snap
      // display to the new drawn-prefix so we never keep showing stale text.
      const expectedDisplay = nextGraphemes.slice(0, drawnRef.current).join("");
      if (expectedDisplay !== cur) {
        displayRef.current = expectedDisplay;
        setDisplay(expectedDisplay);
      }
      return;
    }

    // Far behind or reduced motion → snap.
    if (behind > snapThreshold || prefersReducedMotion()) {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      lastTimeRef.current = null;
      fracRef.current = 0;
      drawnRef.current = nextGraphemes.length;
      displayRef.current = source;
      setDisplay(source);
      return;
    }

    if (rafRef.current !== null) return; // loop already running, will pick up new source via refs

    const tick = (time: number) => {
      if (lastTimeRef.current === null) lastTimeRef.current = time;
      const dt = Math.min(64, time - lastTimeRef.current); // cap to tame tab-return spikes
      lastTimeRef.current = time;

      const graphemes = graphemesRef.current;
      const total = graphemes.length;
      let drawn = drawnRef.current;

      if (drawn >= total) {
        rafRef.current = null;
        lastTimeRef.current = null;
        fracRef.current = 0;
        return;
      }

      const pending = total - drawn;
      const catchup = Math.min(1 + pending / 80, maxCatchup);
      fracRef.current += charsPerFrame * (dt / 16.6667) * catchup;

      const take = Math.floor(fracRef.current);
      if (take > 0) {
        fracRef.current -= take;
        drawn = Math.min(total, drawn + take);
        drawnRef.current = drawn;
        const next = graphemes.slice(0, drawn).join("");
        displayRef.current = next;
        setDisplay(next);
      }

      rafRef.current = requestAnimationFrame(tick);
    };

    rafRef.current = requestAnimationFrame(tick);
  }, [source, charsPerFrame, snapThreshold, maxCatchup]);

  useEffect(() => {
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  return display;
}
