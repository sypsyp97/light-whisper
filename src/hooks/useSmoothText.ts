import { useEffect, useRef, useState } from "react";
import { prefersReducedMotion } from "@/lib/motion";

interface SmoothTextOptions {

  enabled?: boolean;

  charsPerFrame?: number;

  snapThreshold?: number;

  maxCatchup?: number;
}

const DEFAULTS: Required<SmoothTextOptions> = {
  enabled: true,
  charsPerFrame: 2.5,
  snapThreshold: 240,
  maxCatchup: 5,
};

interface GraphemeSegmenter {
  segment(text: string): Iterable<{ segment: string }>;
}

let cachedGraphemeSegmenter: GraphemeSegmenter | null | undefined;

function getGraphemeSegmenter(): GraphemeSegmenter | null {
  if (cachedGraphemeSegmenter !== undefined) return cachedGraphemeSegmenter;

  try {
    const Segmenter = (Intl as typeof Intl & {
      Segmenter?: new (
        locales?: string,
        options?: { granularity: "grapheme" },
      ) => GraphemeSegmenter;
    }).Segmenter;
    cachedGraphemeSegmenter = Segmenter
      ? new Segmenter(undefined, { granularity: "grapheme" })
      : null;
  } catch {
    cachedGraphemeSegmenter = null;
  }

  return cachedGraphemeSegmenter;
}


export function segmentGraphemes(text: string): string[] {
  if (!text) return [];
  const segmenter = getGraphemeSegmenter();
  if (segmenter) return Array.from(segmenter.segment(text), ({ segment }) => segment);
  return Array.from(text);
}







export function useSmoothText(source: string, options: SmoothTextOptions = {}): string {
  const { enabled, charsPerFrame, snapThreshold, maxCatchup } = { ...DEFAULTS, ...options };

  const [display, setDisplay] = useState(source);
  const displayRef = useRef(source);
  const graphemesRef = useRef<string[]>([]);
  const drawnRef = useRef<number>(graphemesRef.current.length);
  const rafRef = useRef<number | null>(null);
  const lastTimeRef = useRef<number | null>(null);
  const fracRef = useRef(0);

  useEffect(() => {
    displayRef.current = display;
  }, [display]);

  useEffect(() => {
    const cur = displayRef.current;



    if (!enabled || !source.startsWith(cur) || source.length < cur.length) {
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


    if (source === cur) {
      graphemesRef.current = segmentGraphemes(source);
      drawnRef.current = graphemesRef.current.length;
      return;
    }


    const nextGraphemes = segmentGraphemes(source);
    graphemesRef.current = nextGraphemes;


    if (drawnRef.current > nextGraphemes.length) {
      drawnRef.current = nextGraphemes.length;
    }

    const behind = nextGraphemes.length - drawnRef.current;
    if (behind <= 0) {




      const expectedDisplay = nextGraphemes.slice(0, drawnRef.current).join("");
      if (expectedDisplay !== cur) {
        displayRef.current = expectedDisplay;
        setDisplay(expectedDisplay);
      }
      return;
    }


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

    if (rafRef.current !== null) return;

    const tick = (time: number) => {
      if (lastTimeRef.current === null) lastTimeRef.current = time;
      const dt = Math.min(64, time - lastTimeRef.current);
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
  }, [source, enabled, charsPerFrame, snapThreshold, maxCatchup]);

  useEffect(() => {
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, []);

  return enabled ? display : source;
}
