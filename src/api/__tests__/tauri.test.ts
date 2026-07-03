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

describe("engine-neutral ASR commands", () => {
  it("uses start_asr_engine for the neutral start wrapper", async () => {
    invokeMock.invoke.mockResolvedValue("ok");
    const { startAsrEngine, startFunASR } = await import("@/api/tauri");
    await startAsrEngine();
    await startFunASR();

    expect(invokeMock.invoke).toHaveBeenNthCalledWith(1, "start_asr_engine");
    expect(invokeMock.invoke).toHaveBeenNthCalledWith(2, "start_asr_engine");
  });

  it("uses check_asr_status for the neutral status wrapper", async () => {
    invokeMock.invoke.mockResolvedValue({
      running: true,
      ready: true,
      model_loaded: true,
      message: "ready",
    });
    const { checkAsrStatus, checkFunASRStatus } = await import("@/api/tauri");
    await checkAsrStatus();
    await checkFunASRStatus();

    expect(invokeMock.invoke).toHaveBeenNthCalledWith(1, "check_asr_status");
    expect(invokeMock.invoke).toHaveBeenNthCalledWith(2, "check_asr_status");
  });
});

describe("setLlmProviderConfig payload", () => {
  it("omits assistantProvider when the caller does not update it", async () => {
    const mod = await import("@/api/tauri");
    const { setLlmProviderConfig } = mod as {
      setLlmProviderConfig: (
        active: string,
        customBaseUrl?: string,
        customModel?: string,
        polishReasoningMode?: string,
        assistantReasoningMode?: string,
        assistantUseSeparateModel?: boolean,
        assistantModel?: string,
        assistantProvider?: string | null,
      ) => Promise<void>;
    };
    invokeMock.invoke.mockResolvedValueOnce(undefined);

    await setLlmProviderConfig(
      "custom",
      "https://example.com/v1",
      "model-a",
      "balanced",
      "light",
      true,
      "assistant-model",
      undefined,
    );

    const [, args] = invokeMock.invoke.mock.calls[0];
    expect(args).not.toHaveProperty("assistantProvider");
    expect(args).not.toHaveProperty("assistantProviderSet");
  });

  it("keeps explicit assistantProvider values in the payload", async () => {
    const mod = await import("@/api/tauri");
    const { setLlmProviderConfig } = mod as {
      setLlmProviderConfig: (
        active: string,
        customBaseUrl?: string,
        customModel?: string,
        polishReasoningMode?: string,
        assistantReasoningMode?: string,
        assistantUseSeparateModel?: boolean,
        assistantModel?: string,
        assistantProvider?: string | null,
      ) => Promise<void>;
    };
    invokeMock.invoke.mockResolvedValueOnce(undefined);

    await setLlmProviderConfig(
      "custom",
      "https://example.com/v1",
      "model-a",
      "balanced",
      "light",
      true,
      "assistant-model",
      "assistant-provider",
    );

    expect(invokeMock.invoke).toHaveBeenCalledWith(
      "set_llm_provider_config",
      expect.objectContaining({
        assistantProvider: "assistant-provider",
        assistantProviderSet: true,
      }),
    );
  });

  it("keeps explicit null assistantProvider so callers can clear it", async () => {
    const mod = await import("@/api/tauri");
    const { setLlmProviderConfig } = mod as {
      setLlmProviderConfig: (
        active: string,
        customBaseUrl?: string,
        customModel?: string,
        polishReasoningMode?: string,
        assistantReasoningMode?: string,
        assistantUseSeparateModel?: boolean,
        assistantModel?: string,
        assistantProvider?: string | null,
      ) => Promise<void>;
    };
    invokeMock.invoke.mockResolvedValueOnce(undefined);

    await setLlmProviderConfig(
      "custom",
      "https://example.com/v1",
      "model-a",
      "balanced",
      "light",
      false,
      "model-a",
      null,
    );

    expect(invokeMock.invoke).toHaveBeenCalledWith(
      "set_llm_provider_config",
      expect.objectContaining({
        assistantProvider: null,
        assistantProviderSet: true,
      }),
    );
  });
});

describe("setCorrectionValidationConfig payload", () => {
  it("omits provider/model set flags when those fields are not updated", async () => {
    const mod = await import("@/api/tauri");
    const { setCorrectionValidationConfig } = mod as {
      setCorrectionValidationConfig: (params: {
        enabled: boolean;
        useSeparateModel?: boolean;
        provider?: string | null;
        model?: string | null;
      }) => Promise<void>;
    };
    invokeMock.invoke.mockResolvedValueOnce(undefined);

    await setCorrectionValidationConfig({ enabled: true, useSeparateModel: false });

    const [, args] = invokeMock.invoke.mock.calls[0];
    expect(args).not.toHaveProperty("provider");
    expect(args).not.toHaveProperty("providerSet");
    expect(args).not.toHaveProperty("model");
    expect(args).not.toHaveProperty("modelSet");
  });

  it("keeps explicit null provider/model updates with set flags", async () => {
    const mod = await import("@/api/tauri");
    const { setCorrectionValidationConfig } = mod as {
      setCorrectionValidationConfig: (params: {
        enabled: boolean;
        useSeparateModel?: boolean;
        provider?: string | null;
        model?: string | null;
      }) => Promise<void>;
    };
    invokeMock.invoke.mockResolvedValueOnce(undefined);

    await setCorrectionValidationConfig({
      enabled: true,
      provider: null,
      model: null,
    });

    expect(invokeMock.invoke).toHaveBeenCalledWith(
      "set_correction_validation_config",
      expect.objectContaining({
        provider: null,
        providerSet: true,
        model: null,
        modelSet: true,
      }),
    );
  });
});
