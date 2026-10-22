import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAsrEngineSettings } from "../useAsrEngineSettings";

const api = vi.hoisted(() => ({
  getEngine: vi.fn(),
  getOnlineAsrApiKey: vi.fn(),
  getOnlineAsrEndpoint: vi.fn(),
  getAlibabaAsrConfig: vi.fn(),
  listAlibabaAsrModels: vi.fn(),
  setAlibabaAsrModel: vi.fn(),
  setEngine: vi.fn(),
  setOnlineAsrApiKey: vi.fn(),
  setOnlineAsrEndpoint: vi.fn(),
}));
const errorToast = vi.hoisted(() => vi.fn());
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: errorToast, success: vi.fn() } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const domestic = { region: "domestic", url: "https://cn.example" };
const international = { region: "international", url: "https://intl.example" };

async function mountSettings() {
  const hook = renderHook(() => useAsrEngineSettings({ engineLabel: (value) => value, retryModel: vi.fn() }));
  await act(async () => { await Promise.resolve(); });
  return hook;
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  api.getEngine.mockResolvedValue("alibaba-asr");
  api.getOnlineAsrApiKey.mockResolvedValue("FAKE_STORED_KEY");
  api.getOnlineAsrEndpoint.mockResolvedValue(international);
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "qwen3-asr-flash", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setEngine.mockImplementation(async (engine: string) => engine);
  api.setOnlineAsrApiKey.mockResolvedValue(undefined);
  api.setOnlineAsrEndpoint.mockResolvedValue(domestic);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("ASR credential identity hydration", () => {
  it.each([
    ["pending", "edit", "debounce"],
    ["pending", "clear", "debounce"],
    ["pending", "edit", "unmount"],
    ["pending", "clear", "unmount"],
    ["failed", "edit", "debounce"],
    ["failed", "clear", "debounce"],
    ["failed", "edit", "unmount"],
    ["failed", "clear", "unmount"],
  ] as const)("blocks Alibaba %s endpoint %s through %s", async (readState, action, delivery) => {
    const endpoint = deferred<typeof domestic>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const { result, unmount } = await mountSettings();
    expect(result.current.engine).toBe("alibaba-asr");
    if (readState === "failed") {
      await act(async () => { endpoint.reject(new Error("endpoint unavailable")); });
      expect(result.current.engineLoading).toBe(false);
      expect(result.current.onlineAsrRegionLoading).toBe(false);
    }

    const savedKey = result.current.onlineAsrApiKey;
    act(() => { result.current.handleOnlineAsrApiKeyChange(action === "edit" ? "FAKE_UNSAFE_EDIT" : ""); });
    expect.soft(result.current.onlineAsrApiKey).toBe(savedKey);
    if (delivery === "debounce") {
      await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    }
    unmount();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });

  it.each(["pending", "failed"] as const)("blocks direct key handlers while the initial engine is %s", async (readState) => {
    const engine = deferred<string>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    api.getOnlineAsrEndpoint.mockResolvedValue(domestic);
    const { result, unmount } = await mountSettings();
    if (readState === "failed") {
      await act(async () => { engine.reject(new Error("engine unavailable")); });
      expect(result.current.engineLoading).toBe(false);
    }

    const savedKey = result.current.onlineAsrApiKey;
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_UNKNOWN_ENGINE_EDIT"); });
    expect.soft(result.current.onlineAsrApiKey).toBe(savedKey);
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    act(() => { result.current.handleOnlineAsrApiKeyChange(""); });
    expect.soft(result.current.onlineAsrApiKey).toBe(savedKey);
    unmount();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });

  it.each(["pending", "failed"] as const)("blocks region mutation while Alibaba endpoint is %s", async (readState) => {
    const endpoint = deferred<typeof domestic>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const { result } = await mountSettings();
    if (readState === "failed") {
      await act(async () => { endpoint.reject(new Error("endpoint unavailable")); });
    }
    await act(async () => { await result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
  });

  it("saves only the post-hydration draft after the real 600ms debounce", async () => {
    const endpoint = deferred<typeof domestic>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const { result } = await mountSettings();
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_UNSAFE_EDIT"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect.soft(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    await act(async () => { endpoint.resolve(domestic); });
    expect(result.current.onlineAsrRegion).toBe("domestic");
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_CN_EDIT"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(599); });
    expect.soft(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_CN_EDIT", "alibaba-asr-cn-api-key");
  });

  it("keeps a successful Alibaba switch but blocks editing after its endpoint read fails", async () => {
    api.getEngine.mockResolvedValue("glm-asr");
    const { result, unmount } = await mountSettings();
    api.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("endpoint unavailable"));
    await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.engineLoading).toBe(false);
    const savedKey = result.current.onlineAsrApiKey;
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_AFTER_FAILED_READ"); });
    expect.soft(result.current.onlineAsrApiKey).toBe(savedKey);
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    act(() => { result.current.handleOnlineAsrApiKeyChange(""); });
    unmount();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    expect(api.setEngine).toHaveBeenCalledExactlyOnceWith("alibaba-asr");
  });

  it.each(["initial", "switch"] as const)("retains a domestic endpoint after an independent %s key-read failure", async (phase) => {
    api.getEngine.mockResolvedValue(phase === "switch" ? "glm-asr" : "alibaba-asr");
    if (phase === "initial") {
      api.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("keyring unavailable"));
      api.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    }
    const { result } = await mountSettings();
    if (phase === "switch") {
      expect(result.current.onlineAsrApiKey).toBe("FAKE_STORED_KEY");
      api.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("keyring unavailable"));
      api.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
      await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    }
    expect(result.current.onlineAsrApiKey).toBe("");
    expect.soft(result.current.onlineAsrRegion).toBe("domestic");
    expect.soft(result.current.onlineAsrUrl).toBe(domestic.url);
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_MANUAL_CN_KEY"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_MANUAL_CN_KEY", "alibaba-asr-cn-api-key");
  });

  it.each(["pending", "failed"] as const)("lets confirmed GLM edit and clear its shared slot with a %s endpoint", async (readState) => {
    api.getEngine.mockResolvedValue("glm-asr");
    const endpoint = deferred<typeof domestic>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const { result, unmount } = await mountSettings();
    if (readState === "failed") {
      await act(async () => { endpoint.reject(new Error("endpoint unavailable")); });
    }
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_GLM_EDIT"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenNthCalledWith(1, "FAKE_GLM_EDIT", "glm-asr-api-key");
    act(() => { result.current.handleOnlineAsrApiKeyChange(""); });
    unmount();
    expect(api.setOnlineAsrApiKey).toHaveBeenNthCalledWith(2, "", "glm-asr-api-key");
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledTimes(2);
  });

  it("ignores the old initial endpoint after a successful switch confirms a new identity", async () => {
    api.getEngine.mockResolvedValue("glm-asr");
    const oldEndpoint = deferred<typeof domestic>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(oldEndpoint.promise);
    const { result } = await mountSettings();
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    await act(async () => { oldEndpoint.resolve(international); });
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrUrl).toBe(domestic.url);
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_CURRENT_CN_KEY"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("FAKE_CURRENT_CN_KEY", "alibaba-asr-cn-api-key");
  });
});
