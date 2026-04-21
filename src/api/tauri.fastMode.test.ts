/**
 * TDD red-state tests for the new `setOpenaiFastMode` helper in
 * `src/api/tauri.ts`.
 *
 * Contract (to be implemented):
 *   - `setOpenaiFastMode(enabled: boolean): Promise<void>` is a named export
 *     of `@/api/tauri`.
 *   - Internally it calls `invoke("set_openai_fast_mode", { enabled })`.
 *     The Rust side will use Tauri's auto snake_case conversion so the JS
 *     object key stays camelCase as `enabled` (a single word, so no mapping
 *     needed here — but the command name MUST be `set_openai_fast_mode`).
 *
 * Both `true` and `false` variants are tested so the impl agent can't hard-
 * code one branch and silently ignore the argument.
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
  invokeMock.invoke.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("setOpenaiFastMode", () => {
  it("exports a function named setOpenaiFastMode", async () => {
    const mod = await import("@/api/tauri");
    expect(typeof (mod as Record<string, unknown>).setOpenaiFastMode).toBe(
      "function",
    );
  });

  it("invokes set_openai_fast_mode with { enabled: true }", async () => {
    const mod = await import("@/api/tauri");
    const { setOpenaiFastMode } = mod as {
      setOpenaiFastMode: (enabled: boolean) => Promise<void>;
    };

    await setOpenaiFastMode(true);

    expect(invokeMock.invoke).toHaveBeenCalledWith("set_openai_fast_mode", {
      enabled: true,
    });
  });

  it("invokes set_openai_fast_mode with { enabled: false }", async () => {
    const mod = await import("@/api/tauri");
    const { setOpenaiFastMode } = mod as {
      setOpenaiFastMode: (enabled: boolean) => Promise<void>;
    };

    await setOpenaiFastMode(false);

    expect(invokeMock.invoke).toHaveBeenCalledWith("set_openai_fast_mode", {
      enabled: false,
    });
  });
});
