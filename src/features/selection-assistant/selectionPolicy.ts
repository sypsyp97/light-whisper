export const DEFAULT_MAX_SELECTION_CHARS = 8_000;

export interface NormalizedSelectionText {
  text: string;
  truncated: boolean;
  originalChars: number;
}

export function normalizeSelectionText(
  value: string,
  maxChars = DEFAULT_MAX_SELECTION_CHARS,
): NormalizedSelectionText | null {
  const normalized = value.replace(/\r\n?/g, "\n").trim();
  if (!normalized) return null;

  const characters = Array.from(normalized);
  const limit = Math.max(1, Math.floor(maxChars));
  return {
    text: characters.slice(0, limit).join(""),
    truncated: characters.length > limit,
    originalChars: characters.length,
  };
}

export interface SelectionEventCandidate {
  text: string;
  phase: "dragging" | "complete";
  sourceProcess: string;
  target: "external" | "toolbar";
  screenshotActive: boolean;
}

export function createSelectionEventGate({ dedupeWindowMs }: { dedupeWindowMs: number }) {
  let lastKey = "";
  let lastAcceptedAt = Number.NEGATIVE_INFINITY;

  return {
    accept(event: SelectionEventCandidate, now = Date.now()): boolean {
      if (
        event.phase !== "complete"
        || event.target !== "external"
        || event.screenshotActive
        || !normalizeSelectionText(event.text)
      ) {
        return false;
      }

      const key = `${event.sourceProcess.toLocaleLowerCase()}\u0000${event.text}`;
      if (key === lastKey && now - lastAcceptedAt <= Math.max(0, dedupeWindowMs)) {
        return false;
      }
      lastKey = key;
      lastAcceptedAt = now;
      return true;
    },
  };
}
