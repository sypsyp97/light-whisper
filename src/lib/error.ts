/**
 * Extract a human-readable message from an unknown error value.
 * Falls back to the provided string when the error is not an Error instance.
 */
export function toErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback;
}
