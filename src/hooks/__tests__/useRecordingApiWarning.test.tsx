import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const events = vi.hoisted(() => ({
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
  warning: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (event: string, handler: (event: { payload: unknown }) => void) => {
    events.handlers.set(event, handler);
    return () => events.handlers.delete(event);
  },
}));
vi.mock("@/api/tauri", () => ({
  startRecording: vi.fn(),
  stopRecording: vi.fn(),
  cancelDictationPolish: vi.fn(),
  copyToClipboard: vi.fn(),
}));
vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: events.error, warning: events.warning },
}));
vi.mock("@/i18n", () => ({
  default: {
    language: "en",
    t: (key: string, options?: { error?: string }) => `${key}: ${options?.error ?? ""}`,
  },
}));

import { useRecording } from "@/hooks/useRecording";

function emit(event: string, payload: unknown) {
  act(() => events.handlers.get(event)?.({ payload }));
}

beforeEach(() => {
  events.handlers.clear();
  events.warning.mockClear();
  events.error.mockClear();
});
afterEach(cleanup);

it("shows API warnings with a stable toast ID and enough reading time", () => {
  renderHook(() => useRecording());
  const payload = { message: "Provider authentication expired; sign in again.", sessionId: 12 };
  emit("api-warning", payload);
  expect(events.warning).toHaveBeenCalledWith(payload.message, expect.any(Object));
  const firstOptions = events.warning.mock.calls[0][1];
  expect(firstOptions.id).toBeTruthy();
  expect(firstOptions.duration).toBeGreaterThanOrEqual(5000);
  emit("api-warning", payload);
  for (const [, options] of events.warning.mock.calls) {
    expect(options.id).toBe(firstOptions.id);
  }
});

it("ignores stale-session warnings while displaying current and sessionless warnings", () => {
  renderHook(() => useRecording());
  emit("recording-state", { sessionId: 12, isRecording: true, isProcessing: false });
  emit("api-warning", { message: "Stale warning", sessionId: 11 });
  expect(events.warning).not.toHaveBeenCalled();
  emit("api-warning", { message: "Current warning", sessionId: 12 });
  emit("api-warning", { message: "Sessionless warning" });
  expect(events.warning.mock.calls.map(([message]) => message)).toEqual([
    "Current warning", "Sessionless warning",
  ]);
});

it("ignores empty or non-string warning messages", () => {
  renderHook(() => useRecording());
  for (const message of ["", "   ", null, undefined, 123, {}, []]) {
    emit("api-warning", { message });
  }
  expect(events.warning).not.toHaveBeenCalled();
});

it("preserves raw transcription and history when an API warning arrives", () => {
  const { result } = renderHook(() => useRecording());
  const text = "Keep this original dictated text.";
  emit("transcription-result", { sessionId: 12, text, originalText: text, interim: false });
  emit("api-warning", { message: "Polishing failed; original text retained.", sessionId: 12 });
  expect(events.warning).toHaveBeenCalled();
  expect(result.current.transcriptionResult).toBe(text);
  expect(result.current.originalAsrText).toBe(text);
  expect(result.current.history[0]).toMatchObject({ text, originalText: text });
});

it.each(["error", "message"])("shows AI polish errors carried in the %s field", (field) => {
  renderHook(() => useRecording());
  const message = "The provider rejected authentication; sign in again.";
  emit("ai-polish-status", { status: "error", [field]: message });
  expect(events.error).toHaveBeenCalledWith(expect.stringContaining(message), expect.any(Object));
});
