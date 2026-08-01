export function normalizeSelectionText(value: string): string | null {
  const normalized = value.replace(/\r\n?/g, "\n").trim();
  return normalized || null;
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
