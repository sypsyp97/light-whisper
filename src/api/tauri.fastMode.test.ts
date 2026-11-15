














import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));

vi.mock("@tauri-apps/api/core", () => invokeMock);



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
