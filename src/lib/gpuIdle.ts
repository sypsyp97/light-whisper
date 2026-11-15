
export const GPU_IDLE_OFF_SECONDS = 0;


export const GPU_IDLE_SUGGESTED_SECONDS = 180;

export const GPU_IDLE_MAX_SECONDS = 7 * 24 * 60 * 60;

export function normalizeGpuIdleSeconds(value: unknown): number {
  const parsed = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return GPU_IDLE_OFF_SECONDS;
  return Math.min(GPU_IDLE_MAX_SECONDS, Math.floor(parsed));
}
