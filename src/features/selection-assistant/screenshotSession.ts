export type ScreenshotSession =
  | { status: "idle" }
  | { status: "capturing"; captureId: string }
  | { status: "processing"; captureId: string; imageBase64: string }
  | { status: "ready"; captureId: string; imageBase64: string; recognizedText?: string };

export const initialScreenshotSession: ScreenshotSession = { status: "idle" };

type ScreenshotEvent =
  | { type: "start"; captureId: string }
  | { type: "cancel" | "capture_cancelled"; captureId: string }
  | { type: "captured"; captureId: string; imageBase64: string }
  | { type: "ocr_complete"; captureId: string; text: string };

export function reduceScreenshotSession(
  state: ScreenshotSession,
  event: ScreenshotEvent,
): ScreenshotSession {
  if (event.type === "start") {
    return state.status === "idle"
      ? { status: "capturing", captureId: event.captureId }
      : state;
  }
  if (state.status === "idle" || state.captureId !== event.captureId) return state;
  if (event.type === "cancel" || event.type === "capture_cancelled") return initialScreenshotSession;
  if (event.type === "captured" && state.status === "capturing") {
    return { status: "processing", captureId: event.captureId, imageBase64: event.imageBase64 };
  }
  if (event.type === "ocr_complete" && state.status === "processing") {
    const recognizedText = event.text.trim() || undefined;
    return { ...state, status: "ready", recognizedText };
  }
  return state;
}
