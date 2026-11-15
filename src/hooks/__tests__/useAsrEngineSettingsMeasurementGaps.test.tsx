import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAsrEngineSettings } from "@/hooks/useAsrEngineSettings";

const api = vi.hoisted(() => ({
  getEngine: vi.fn(), getOnlineAsrApiKey: vi.fn(), getOnlineAsrEndpoint: vi.fn(),
  getAlibabaAsrConfig: vi.fn(), listAlibabaAsrModels: vi.fn(), setAlibabaAsrModel: vi.fn(),
  setEngine: vi.fn(), setOnlineAsrApiKey: vi.fn(), setOnlineAsrEndpoint: vi.fn(),
}));
const listen = vi.hoisted(() => vi.fn());
const notices = vi.hoisted(() => ({ error: vi.fn(), warning: vi.fn(), success: vi.fn() }));
const translation = vi.hoisted(() => ({
  t: vi.fn((key: string, _options?: { label?: string }) => { void _options; return key; }),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: notices }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translation.t }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const international = { region: "international", url: "https://intl.example.test" };
const domestic = { region: "domestic", url: "https://cn.example.test" };
const retryModel = vi.fn();
const engineLabel = (value: string) => value;
let invalidate: () => void;

async function mountSettings() {
  const hook = renderHook(() => useAsrEngineSettings({ engineLabel, retryModel }));
  await act(async () => { await Promise.resolve(); });
  return hook;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  translation.t = vi.fn((key: string, _options?: { label?: string }) => { void _options; return key; });
  listen.mockImplementation((_name: string, callback: () => void) => {
    invalidate = callback;
    return Promise.resolve(vi.fn());
  });
  api.getEngine.mockResolvedValue("alibaba-asr");
  api.getOnlineAsrApiKey.mockResolvedValue("SYNTHETIC_STORED_KEY");
  api.getOnlineAsrEndpoint.mockResolvedValue(international);
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "saved-model", models: ["saved-model"] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setAlibabaAsrModel.mockResolvedValue({ model: "selected-model" });
  api.setEngine.mockImplementation(async (engine: string) => engine);
  api.setOnlineAsrApiKey.mockResolvedValue(undefined);
  api.setOnlineAsrEndpoint.mockResolvedValue(domestic);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("ASR settings measured ownership gaps", () => {
  it.each(["engine", "region"] as const)("retains the saved identity when %s credential flush fails and permits a later transition", async (kind) => {
    const { result } = await mountSettings();
    api.setOnlineAsrApiKey.mockRejectedValueOnce(new Error("synthetic keyring write failed"));
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_UNSAVED_DRAFT"); });
    const transition = () => kind === "engine"
      ? result.current.handleEngineSwitch("glm-asr")
      : result.current.handleOnlineAsrRegionChange("domestic");
    await act(async () => { await transition(); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_UNSAVED_DRAFT", "alibaba-asr-intl-api-key");
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrRegion).toBe("international");
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_UNSAVED_DRAFT");
    expect(result.current.engineLoading).toBe(false);
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(notices.error).toHaveBeenCalledTimes(1);
    expect(notices.error).toHaveBeenCalledWith(expect.stringMatching(/\S/));
    expect(notices.success).not.toHaveBeenCalled();
    expect(retryModel).not.toHaveBeenCalled();

    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_RECOVERED_DRAFT"); });
    await act(async () => { await transition(); });
    expect(api.setOnlineAsrApiKey).toHaveBeenNthCalledWith(2, "SYNTHETIC_RECOVERED_DRAFT", "alibaba-asr-intl-api-key");
    if (kind === "engine") expect(api.setEngine).toHaveBeenCalledExactlyOnceWith("glm-asr");
    else expect(api.setOnlineAsrEndpoint).toHaveBeenCalledExactlyOnceWith("domestic");
  });

  it("deduplicates a retry and keeps a newer retry busy when the retired retry settles", async () => {
    api.getEngine.mockRejectedValueOnce(new Error("initial engine read failed"));
    const { result } = await mountSettings();
    const retired = deferred<string>();
    api.getEngine.mockReturnValueOnce(retired.promise);
    let oldRetry!: Promise<void>;
    await act(async () => { oldRetry = result.current.retryAsrConfig(); });
    expect(result.current.asrConfigRetrying).toBe(true);
    await act(async () => { await result.current.retryAsrConfig(); });
    expect(api.getEngine).toHaveBeenCalledTimes(2);
    expect(result.current.asrConfigRetrying).toBe(true);

    api.getEngine.mockRejectedValueOnce(new Error("new saved engine read failed"));
    await act(async () => { invalidate(); });
    expect(result.current.asrConfigError).toBe(true);
    expect(result.current.asrConfigRetrying).toBe(false);
    const latest = deferred<string>();
    api.getEngine.mockReturnValueOnce(latest.promise);
    let newRetry!: Promise<void>;
    await act(async () => { newRetry = result.current.retryAsrConfig(); });
    expect(result.current.asrConfigRetrying).toBe(true);
    await act(async () => { retired.resolve("glm-asr"); await oldRetry; });
    expect(result.current.asrConfigRetrying).toBe(true);
    expect(result.current.engineLoading).toBe(true);
    await act(async () => { latest.resolve("confucius4-r2t2"); await newRetry; });
    expect(result.current.asrConfigRetrying).toBe(false);
    expect(result.current.asrConfigError).toBe(false);
    expect(result.current.engine).toBe("confucius4-r2t2");
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("does not retry configuration during an admitted engine transition", async () => {
    api.getOnlineAsrEndpoint.mockRejectedValueOnce(new Error("initial endpoint read failed"));
    const { result } = await mountSettings();
    const mutation = deferred<string>();
    api.setEngine.mockReturnValueOnce(mutation.promise);
    let switching!: Promise<void>;
    await act(async () => { switching = result.current.handleEngineSwitch("confucius4-r2t2"); });
    await act(async () => { await result.current.retryAsrConfig(); });
    expect(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(1);
    expect(result.current.asrConfigRetrying).toBe(false);
    expect(result.current.asrConfigError).toBe(true);
    await act(async () => { mutation.resolve("confucius4-r2t2"); await switching; });
    expect(result.current.engine).toBe("confucius4-r2t2");
    expect(result.current.engineLoading).toBe(false);
  });

  it("keeps a rejected listener registration quiet after disposal", async () => {
    const registration = deferred<() => void>();
    listen.mockReturnValueOnce(registration.promise);
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    const hook = await mountSettings();
    hook.unmount();
    await act(async () => { registration.reject(new Error("retired registration rejected")); });
    expect(error).not.toHaveBeenCalled();
    expect(warning).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
    expect(notices.warning).not.toHaveBeenCalled();
  });

  it("keeps a newer catalog request loading and quiet when a retired request rejects", async () => {
    const { result } = await mountSettings();
    const retired = deferred<{ models: string[]; source: "live" }>();
    const latest = deferred<{ models: string[]; source: "live" }>();
    api.listAlibabaAsrModels.mockReturnValueOnce(retired.promise).mockReturnValueOnce(latest.promise);
    let oldRefresh!: Promise<void>;
    let newRefresh!: Promise<void>;
    await act(async () => {
      oldRefresh = result.current.refreshAlibabaModels();
      newRefresh = result.current.refreshAlibabaModels();
    });
    await act(async () => { retired.reject(new Error("retired catalog request failed")); await oldRefresh; });
    expect(result.current.alibabaAsrModelsLoading).toBe(true);
    expect(notices.error).not.toHaveBeenCalled();
    expect(notices.warning).not.toHaveBeenCalled();
    await act(async () => { latest.resolve({ models: ["latest-model"], source: "live" }); await newRefresh; });
    expect(result.current.alibabaAsrModels).toEqual(["latest-model"]);
    expect(result.current.alibabaAsrModelsSource).toBe("live");
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
  });

  it("preserves the GLM shared key across a region change without rereading it", async () => {
    api.getEngine.mockResolvedValue("glm-asr");
    const { result } = await mountSettings();
    await act(async () => { await result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledExactlyOnceWith("domestic");
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(1);
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_STORED_KEY");
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrUrl).toBe(domestic.url);
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_GLM_DRAFT"); });
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_GLM_DRAFT", "glm-asr-api-key");
  });

  it("hydrates the saved model and fallback catalog without requiring a cloud credential", async () => {
    api.getOnlineAsrApiKey.mockResolvedValue("");
    const saved = deferred<{ model: string; models: string[] }>();
    api.getAlibabaAsrConfig.mockReturnValueOnce(saved.promise);
    const { result } = await mountSettings();
    await act(async () => { saved.resolve({ model: "saved-selected", models: ["saved-selected", "saved-other"] }); });
    expect(result.current.alibabaAsrModel).toBe("saved-selected");
    expect(result.current.alibabaAsrModels).toEqual(["saved-selected", "saved-other"]);
    expect(result.current.alibabaAsrModelsSource).toBe("fallback");
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
  });

  it("keeps a nonempty model and an empty fallback catalog when the initial saved-model read fails", async () => {
    api.getOnlineAsrApiKey.mockResolvedValue("");
    api.getAlibabaAsrConfig.mockRejectedValueOnce(new Error("saved model read unavailable"));
    const { result } = await mountSettings();
    expect(result.current.alibabaAsrModel).toEqual(expect.stringMatching(/\S/));
    expect(result.current.alibabaAsrModels).toEqual([]);
    expect(result.current.alibabaAsrModelsSource).toBe("fallback");
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
  });

  it("preserves the live catalog when the older saved selection and catalog arrive later", async () => {
    const saved = deferred<{ model: string; models: string[] }>();
    api.getAlibabaAsrConfig.mockReturnValueOnce(saved.promise);
    api.listAlibabaAsrModels.mockResolvedValueOnce({ models: ["live-model"], source: "live" });
    const { result } = await mountSettings();
    expect(result.current.alibabaAsrModels).toEqual(["live-model"]);
    await act(async () => { saved.resolve({ model: "saved-selected", models: ["retired-catalog-model"] }); });
    expect(result.current.alibabaAsrModel).toBe("saved-selected");
    expect(result.current.alibabaAsrModels).toEqual(["live-model"]);
    expect(result.current.alibabaAsrModelsSource).toBe("live");
  });

  it("retains the usable catalog and selected model when a fresh catalog result is empty", async () => {
    const { result } = await mountSettings();
    api.listAlibabaAsrModels.mockResolvedValueOnce({ models: ["selected-model", "other-model"], source: "live" });
    await act(async () => { await result.current.refreshAlibabaModels(); });
    await act(async () => { await result.current.handleAlibabaAsrModelSelect("selected-model"); });
    api.listAlibabaAsrModels.mockResolvedValueOnce({ models: [], source: "fallback" });
    await act(async () => { await result.current.refreshAlibabaModels(); });
    expect(result.current.alibabaAsrModels).toEqual(["selected-model", "other-model"]);
    expect(result.current.alibabaAsrModel).toBe("selected-model");
    expect(result.current.alibabaAsrModelsSource).toBe("live");
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
  });

  it.each(["", "   "])("does not automatically query a catalog with an empty credential %j", async (key) => {
    api.getOnlineAsrApiKey.mockResolvedValue(key);
    const { result } = await mountSettings();
    expect(result.current.onlineAsrApiKey).toBe(key);
    expect(result.current.alibabaHasKey).toBe(false);
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
  });

  it.each(["glm-asr", "qwen3-asr-0.6b"])("keeps Alibaba catalog eligibility scoped to its engine when %s has a stored key", async (engine) => {
    api.getEngine.mockResolvedValue(engine);
    const { result } = await mountSettings();
    expect(result.current.alibabaHasKey).toBe(false);
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
    if (engine === "qwen3-asr-0.6b") {
      expect(result.current.onlineAsrKeyReady).toBe(false);
      act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_LOCAL_EDIT"); });
      await act(async () => { await vi.advanceTimersByTimeAsync(600); });
      expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    }
  });

  it.each(["engine", "region"] as const)("keeps an identical %s selection free of persistence, read, loading and flush effects", async (kind) => {
    const { result } = await mountSettings();
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_PENDING_DRAFT"); });
    await act(async () => {
      if (kind === "engine") await result.current.handleEngineSwitch("alibaba-asr");
      else await result.current.handleOnlineAsrRegionChange("international");
    });
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    expect(api.getEngine).toHaveBeenCalledTimes(1);
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(1);
    expect(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(1);
    expect(result.current.engineLoading).toBe(false);
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
    expect(retryModel).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_PENDING_DRAFT", "alibaba-asr-intl-api-key");
  });

  it("keeps an admitted engine change busy, reports its current native failure and permits retry", async () => {
    const { result } = await mountSettings();
    const mutation = deferred<string>();
    api.setEngine.mockReturnValueOnce(mutation.promise);
    let switching!: Promise<void>;
    await act(async () => { switching = result.current.handleEngineSwitch("glm-asr"); });
    expect(result.current.engineLoading).toBe(true);
    await act(async () => { mutation.reject(new Error("current native engine failure")); await switching; });
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_STORED_KEY");
    expect(result.current.engineLoading).toBe(false);
    expect(notices.error).toHaveBeenCalledExactlyOnceWith(expect.stringMatching(/\S/));
    expect(notices.success).not.toHaveBeenCalled();
    await act(async () => { await result.current.handleEngineSwitch("glm-asr"); });
    expect(api.setEngine).toHaveBeenCalledTimes(2);
    expect(result.current.engine).toBe("glm-asr");
    expect(result.current.engineLoading).toBe(false);
  });

  it("keeps a successful current cloud switch quiet after its endpoint and key reads succeed", async () => {
    const { result } = await mountSettings();
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_CURRENT_GLM_KEY");
    await act(async () => { await result.current.handleEngineSwitch("glm-asr"); });
    expect(result.current.engine).toBe("glm-asr");
    expect(result.current.onlineAsrUrl).toBe(domestic.url);
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_CURRENT_GLM_KEY");
    expect(result.current.asrConfigError).toBe(false);
    expect(result.current.engineLoading).toBe(false);
    expect(notices.error).not.toHaveBeenCalled();
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("owns region loading through persistence and key settlement without acquiring engine or catalog loading", async () => {
    api.getOnlineAsrApiKey.mockResolvedValue("");
    const { result } = await mountSettings();
    const mutation = deferred<typeof domestic>();
    const key = deferred<string>();
    api.setOnlineAsrEndpoint.mockReturnValueOnce(mutation.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    let changing!: Promise<void>;
    await act(async () => { changing = result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(result.current.onlineAsrRegionLoading).toBe(true);
    expect(result.current.engineLoading).toBe(false);
    await act(async () => { mutation.resolve(domestic); });
    expect(result.current.onlineAsrRegionLoading).toBe(true);
    expect(result.current.engineLoading).toBe(false);
    expect(result.current.onlineAsrEndpointReady).toBe(true);
    expect(result.current.onlineAsrKeyReady).toBe(true);
    await act(async () => { key.resolve(""); await changing; });
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(result.current.engineLoading).toBe(false);
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
    expect(notices.error).not.toHaveBeenCalled();
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
  });

  it.each(["engine", "region"] as const)("reports a current %s key-read failure and retries only the failed read", async (kind) => {
    if (kind === "engine") api.getEngine.mockResolvedValue("glm-asr");
    const { result } = await mountSettings();
    api.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("current key read unavailable"));
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(domestic);
    await act(async () => {
      if (kind === "engine") await result.current.handleEngineSwitch("alibaba-asr");
      else await result.current.handleOnlineAsrRegionChange("domestic");
    });
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrApiKey).toBe("");
    expect(result.current.asrConfigError).toBe(true);
    expect(result.current.engineLoading).toBe(false);
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(notices.error).toHaveBeenCalledExactlyOnceWith(expect.stringMatching(/\S/));
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_RETRIED_KEY");
    await act(async () => { await result.current.retryAsrConfig(); });
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_RETRIED_KEY");
    expect(result.current.asrConfigError).toBe(false);
    expect(result.current.asrConfigRetrying).toBe(false);
    expect(result.current.engineLoading).toBe(false);
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(3);
    expect(api.setEngine).toHaveBeenCalledTimes(kind === "engine" ? 1 : 0);
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledTimes(kind === "region" ? 1 : 0);
  });

  it("announces the selected local engine label and retires cloud error and catalog-loading state", async () => {
    translation.t.mockImplementation((key, options) => key ? `${key} ${options?.label ?? ""}` : "");
    const label = vi.fn((engine: string) => `Selected ${engine}`);
    const hook = renderHook(() => useAsrEngineSettings({ engineLabel: label, retryModel }));
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await hook.result.current.handleEngineSwitch("confucius4-r2t2"); });
    expect(label).toHaveBeenCalledWith("confucius4-r2t2");
    expect(notices.success).toHaveBeenCalledExactlyOnceWith(expect.stringContaining("Selected confucius4-r2t2"));
    expect(hook.result.current.engine).toBe("confucius4-r2t2");
    expect(hook.result.current.engineLoading).toBe(false);
    expect(hook.result.current.alibabaAsrModelsLoading).toBe(false);
    expect(hook.result.current.asrConfigError).toBe(false);
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(1);
    expect(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(1);
    expect(notices.error).not.toHaveBeenCalled();
  });

  it("clears the retired key and endpoint while a new saved identity remains pending", async () => {
    const { result } = await mountSettings();
    const engine = deferred<string>();
    const endpoint = deferred<typeof domestic>();
    const key = deferred<string>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    await act(async () => { invalidate(); });
    expect(result.current.onlineAsrApiKey).toBe("");
    expect(result.current.onlineAsrUrl).toBe("");
    expect(result.current.onlineAsrKeyReady).toBe(false);
    expect(result.current.onlineAsrEndpointReady).toBe(false);
    expect(result.current.engineLoading).toBe(true);
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(result.current.alibabaAsrModelsLoading).toBe(false);
    expect(retryModel).not.toHaveBeenCalled();
    await act(async () => {
      engine.resolve("glm-asr");
      endpoint.resolve(domestic);
      key.resolve("SYNTHETIC_LATEST_KEY");
    });
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_LATEST_KEY");
    expect(result.current.onlineAsrUrl).toBe(domestic.url);
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("keeps a user draft healthy when the older key read rejects after that edit", async () => {
    const key = deferred<string>();
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    const { result } = await mountSettings();
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_CURRENT_DRAFT"); });
    await act(async () => { key.reject(new Error("retired key read failed")); });
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_CURRENT_DRAFT");
    expect(result.current.asrConfigError).toBe(false);
    expect(notices.error).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_CURRENT_DRAFT", "alibaba-asr-intl-api-key");
  });

  it("keeps its current listener installed until the mounted settings owner disposes it", async () => {
    const stop = vi.fn();
    listen.mockResolvedValueOnce(stop);
    const hook = await mountSettings();
    expect(stop).not.toHaveBeenCalled();
    hook.unmount();
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it("reports a current catalog failure in the latest rendered locale", async () => {
    translation.t.mockImplementation((key) => key ? `old-locale:${key}` : "");
    const hook = await mountSettings();
    translation.t = vi.fn((key: string, _options?: { label?: string }) => {
      void _options;
      return key ? `new-locale:${key}` : "";
    });
    hook.rerender();
    await act(async () => { await Promise.resolve(); });
    api.listAlibabaAsrModels.mockRejectedValueOnce(new Error("current catalog request failed"));
    await act(async () => { await hook.result.current.refreshAlibabaModels(); });
    expect(notices.error).toHaveBeenCalledExactlyOnceWith(expect.stringMatching(/^new-locale:\S/));
    expect(hook.result.current.alibabaAsrModelsLoading).toBe(false);
  });

  it("admits only one repeated region intent before React publishes the pending state", async () => {
    const { result } = await mountSettings();
    const mutation = deferred<typeof domestic>();
    api.setOnlineAsrEndpoint.mockReturnValueOnce(mutation.promise);
    let first!: Promise<void>;
    let duplicate!: Promise<void>;
    act(() => {
      first = result.current.handleOnlineAsrRegionChange("domestic");
      duplicate = result.current.handleOnlineAsrRegionChange("domestic");
    });
    await act(async () => { await duplicate; });
    await act(async () => { mutation.resolve(domestic); await first; });
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledExactlyOnceWith("domestic");
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrRegionLoading).toBe(false);
  });

  it("publishes loading without readiness or an invented error on the first public render", async () => {
    const engine = deferred<string>();
    const endpoint = deferred<typeof international>();
    const key = deferred<string>();
    const saved = deferred<{ model: string; models: string[] }>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    api.getAlibabaAsrConfig.mockReturnValueOnce(saved.promise);
    const views: { loading: boolean; error: boolean; endpointReady: boolean; keyReady: boolean; source: "live" | "fallback" }[] = [];
    const hook = renderHook(() => {
      const settings = useAsrEngineSettings({ engineLabel, retryModel });
      views.push({
        loading: settings.engineLoading, error: settings.asrConfigError,
        endpointReady: settings.onlineAsrEndpointReady, keyReady: settings.onlineAsrKeyReady,
        source: settings.alibabaAsrModelsSource,
      });
      return settings;
    });
    expect(views[0]).toEqual({ loading: true, error: false, endpointReady: false, keyReady: false, source: "fallback" });
    await act(async () => {
      engine.resolve("alibaba-asr");
      endpoint.resolve(international);
      key.resolve("");
      saved.resolve({ model: "saved-model", models: ["saved-model"] });
    });
    expect(hook.result.current.engineLoading).toBe(false);
    expect(hook.result.current.asrConfigError).toBe(false);
    expect(hook.result.current.onlineAsrKeyReady).toBe(true);
  });

  it("reports listener registration failure with useful context and the rejected error", async () => {
    const failure = new Error("synthetic listener registration unavailable");
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    listen.mockRejectedValueOnce(failure);
    await mountSettings();
    expect(error).toHaveBeenCalledExactlyOnceWith(expect.stringMatching(/\S/), failure);
  });
});
