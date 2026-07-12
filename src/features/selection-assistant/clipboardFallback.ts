import { normalizeSelectionText } from "./selectionPolicy";

export interface SelectionReadPort {
  readDirect(): Promise<string | null>;
  snapshotClipboard(): Promise<unknown>;
  requestCopy(): Promise<void>;
  readClipboardText(): Promise<string | null>;
  restoreClipboard(snapshot: unknown): Promise<void>;
}

export function shouldRestoreClipboard(current: string | null, temporary: string): boolean {
  return current === temporary;
}

export async function readSelectedText(port: SelectionReadPort) {
  try {
    const direct = normalizeSelectionText((await port.readDirect()) ?? "");
    if (direct) {
      return {
        kind: "text" as const,
        text: direct.text,
        truncated: direct.truncated,
        source: "direct" as const,
      };
    }
  } catch {
    // Some applications do not expose UI Automation TextPattern; copy is the fallback.
  }

  let snapshot: unknown;
  try {
    snapshot = await port.snapshotClipboard();
  } catch {
    return { kind: "unavailable" as const, reason: "snapshot_failed" as const };
  }

  try {
    await port.requestCopy();
  } catch {
    await port.restoreClipboard(snapshot).catch(() => undefined);
    return { kind: "unavailable" as const, reason: "copy_failed" as const };
  }

  const copied = await port.readClipboardText().catch(() => null);
  const normalized = normalizeSelectionText(copied ?? "");
  const current = await port.readClipboardText().catch(() => null);
  if (copied !== null && shouldRestoreClipboard(current, copied)) {
    await port.restoreClipboard(snapshot).catch(() => undefined);
  }
  if (!normalized) return { kind: "unavailable" as const, reason: "empty" as const };
  return {
    kind: "text" as const,
    text: normalized.text,
    truncated: normalized.truncated,
    source: "clipboard" as const,
  };
}
