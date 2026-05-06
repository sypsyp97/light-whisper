/**
 * TDD red-state tests for structured IPC error normalization in
 * `src/api/tauri.ts`.
 *
 * Contract (to be implemented):
 *   - `IpcError` is a named export class extending `Error` with
 *     readonly `code`, `category`, `details` fields.
 *   - When `invoke(...)` rejects with a structured object shaped
 *     `{ code, category, message, details? }`, `invokeCommand` must
 *     produce an `IpcError` instance carrying those fields.
 *   - String rejections, plain objects without `code`, and Error
 *     instances continue to flow through `normalizeInvokeError` as
 *     plain `Error` (not `IpcError`).
 *
 * All tests drive through the public `transcribeAudio` wrapper, which
 * calls `invoke("transcribe_audio", { audioBase64 })`. We mock
 * `@tauri-apps/api/core` so we can deterministically reject `invoke`
 * and inspect the wrapped error.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));

vi.mock("@tauri-apps/api/core", () => invokeMock);

// Autostart plugin is imported eagerly by tauri.ts; stub it out so jsdom
// doesn't blow up on the missing plugin.
vi.mock("@tauri-apps/plugin-autostart", () => ({
  disable: vi.fn(),
  enable: vi.fn(),
  isEnabled: vi.fn(),
}));

beforeEach(() => {
  invokeMock.invoke.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("normalizeInvokeError -> IpcError", () => {
  it("structured_error_object_becomes_IpcError", async () => {
    const mod = await import("@/api/tauri");
    const { transcribeAudio, IpcError } = mod as {
      transcribeAudio: (audioBase64: string) => Promise<unknown>;
      IpcError: new (
        message: string,
        code: string,
        category: string,
        details?: unknown,
      ) => Error;
    };

    invokeMock.invoke.mockRejectedValueOnce({
      code: "ASR_ERROR",
      category: "asr",
      message: "foo",
      details: null,
    });

    let caught: unknown;
    try {
      await transcribeAudio("base64");
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(IpcError);
    expect(caught).toBeInstanceOf(Error);
    const err = caught as Error & {
      code: string;
      category: string;
    };
    expect(err.code).toBe("ASR_ERROR");
    expect(err.category).toBe("asr");
    expect(err.message).toBe("foo");
  });

  it("IpcError_carries_details_field", async () => {
    const mod = await import("@/api/tauri");
    const { transcribeAudio, IpcError } = mod as {
      transcribeAudio: (audioBase64: string) => Promise<unknown>;
      IpcError: new (
        message: string,
        code: string,
        category: string,
        details?: unknown,
      ) => Error;
    };

    invokeMock.invoke.mockRejectedValueOnce({
      code: "X",
      category: "y",
      message: "m",
      details: { nested: 1 },
    });

    let caught: unknown;
    try {
      await transcribeAudio("base64");
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(IpcError);
    const err = caught as Error & { details: unknown };
    expect(err.details).toEqual({ nested: 1 });
  });

  it("plain_string_rejection_produces_plain_Error_not_IpcError", async () => {
    const mod = await import("@/api/tauri");
    const { transcribeAudio, IpcError } = mod as {
      transcribeAudio: (audioBase64: string) => Promise<unknown>;
      IpcError: new (
        message: string,
        code: string,
        category: string,
        details?: unknown,
      ) => Error;
    };

    invokeMock.invoke.mockRejectedValueOnce("plain string error");

    let caught: unknown;
    try {
      await transcribeAudio("base64");
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(Error);
    expect(caught).not.toBeInstanceOf(IpcError);
    expect((caught as Error).message).toBe("plain string error");
  });

  it("object_without_code_falls_back_to_plain_Error", async () => {
    const mod = await import("@/api/tauri");
    const { transcribeAudio, IpcError } = mod as {
      transcribeAudio: (audioBase64: string) => Promise<unknown>;
      IpcError: new (
        message: string,
        code: string,
        category: string,
        details?: unknown,
      ) => Error;
    };

    invokeMock.invoke.mockRejectedValueOnce({
      message: "no code",
      error: "x",
    });

    let caught: unknown;
    try {
      await transcribeAudio("base64");
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(Error);
    expect(caught).not.toBeInstanceOf(IpcError);
    expect((caught as Error).message).toBe("no code");
  });

  it("Error_instance_passes_through", async () => {
    const mod = await import("@/api/tauri");
    const { transcribeAudio } = mod as {
      transcribeAudio: (audioBase64: string) => Promise<unknown>;
    };

    const native = new Error("native");
    invokeMock.invoke.mockRejectedValueOnce(native);

    let caught: unknown;
    try {
      await transcribeAudio("base64");
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(Error);
    expect((caught as Error).message).toBe("native");
  });
});

describe("isPermissionDeniedError type guard", () => {
  it("returns true when code is PERMISSION_DENIED with kind+settingsUrl details", async () => {
    const { isPermissionDeniedError, IpcError } = await import("@/api/tauri");
    const err = new IpcError("denied", "PERMISSION_DENIED", "permission", {
      kind: "microphone",
      settingsUrl:
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
    });
    expect(isPermissionDeniedError(err)).toBe(true);
    if (isPermissionDeniedError(err)) {
      // After narrowing, details.kind/settingsUrl are typed accessible.
      expect(err.details.kind).toBe("microphone");
      expect(err.details.settingsUrl).toContain("Privacy_Microphone");
    }
  });

  it("returns false for non-permission IpcError", async () => {
    const { isPermissionDeniedError, IpcError } = await import("@/api/tauri");
    const err = new IpcError("audio failed", "AUDIO_ERROR", "audio");
    expect(isPermissionDeniedError(err)).toBe(false);
  });

  it("returns false for plain Error", async () => {
    const { isPermissionDeniedError } = await import("@/api/tauri");
    expect(isPermissionDeniedError(new Error("boom"))).toBe(false);
  });

  it("returns false when details object is missing required keys", async () => {
    // Defensive: a future backend bug shouldn't be silently treated as a
    // valid permission error and crash the deeplink button.
    const { isPermissionDeniedError, IpcError } = await import("@/api/tauri");
    const err = new IpcError(
      "denied",
      "PERMISSION_DENIED",
      "permission",
      // missing settingsUrl
      { kind: "microphone" },
    );
    expect(isPermissionDeniedError(err)).toBe(false);
  });
});

describe("openPermissionSettings", () => {
  it("invokes the open_permission_settings IPC command with the given kind", async () => {
    invokeMock.invoke.mockResolvedValueOnce(undefined);
    const { openPermissionSettings } = await import("@/api/tauri");
    await openPermissionSettings("accessibility");
    // The IPC command name must match the tauri::command we registered;
    // changing either side without the other will silently break the
    // settings deeplink in production. This test pins both halves.
    expect(invokeMock.invoke).toHaveBeenCalledWith(
      "open_permission_settings",
      { kind: "accessibility" },
    );
  });
});
