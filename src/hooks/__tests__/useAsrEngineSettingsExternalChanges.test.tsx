import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAsrEngineSettings } from "../useAsrEngineSettings";

const api = vi.hoisted(() => ({
  getEngine: vi.fn(), getOnlineAsrApiKey: vi.fn(), getOnlineAsrEndpoint: vi.fn(),
  getAlibabaAsrConfig: vi.fn(), listAlibabaAsrModels: vi.fn(), setAlibabaAsrModel: vi.fn(),
  setEngine: vi.fn(), setOnlineAsrApiKey: vi.fn(), setOnlineAsrEndpoint: vi.fn(),
}));
const listen = vi.hoisted(() => vi.fn());
const notices = vi.hoisted(() => ({ error: vi.fn(), warning: vi.fn(), success: vi.fn() }));
const translate = vi.hoisted(() => (key: string) => key);
vi.mock("@/api/tauri", () => api);
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("sonner", () => ({ toast: notices }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));

type Stop = () => void;
type Listener = { name: string; callback: (event: { payload: unknown }) => void; stop: ReturnType<typeof vi.fn> };
let listeners: Listener[];
let subscriptions: Promise<Stop>[];
const retryModel = vi.fn();
const engineLabel = (engine: string) => engine;
const initialEndpoint = { region: "international", url: "https://initial.example.test" };
const currentEndpoint = { region: "domestic", url: "https://current.example.test" };
const externalEndpoint = { region: "international", url: "https://external.example.test" };
type Settings = ReturnType<typeof useAsrEngineSettings>;
type Mutation = "engine" | "region";
const mutate = (settings: Settings, kind: Mutation) => kind === "engine"
  ? settings.handleEngineSwitch("glm-asr")
  : settings.handleOnlineAsrRegionChange("domestic");
const configView = (settings: Settings) => ({
  engine: settings.engine, loading: settings.engineLoading,
  region: settings.onlineAsrRegion, url: settings.onlineAsrUrl, key: settings.onlineAsrApiKey,
  endpointReady: settings.onlineAsrEndpointReady, keyReady: settings.onlineAsrKeyReady,
  regionLoading: settings.onlineAsrRegionLoading, error: settings.asrConfigError,
  retrying: settings.asrConfigRetrying, catalogLoading: settings.alibabaAsrModelsLoading,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  // A missing baseline listener leaves some boundary promises unrequested.
  // Keep that absence an assertion RED; hook-created rejection chains still surface.
  void promise.catch(() => undefined);
  return { promise, resolve, reject };
}

async function mountSettings(options: { reactStrictMode?: boolean } = {}) {
  const hook = renderHook(() => useAsrEngineSettings({ engineLabel, retryModel }), options);
  await act(async () => { await Promise.resolve(); });
  return hook;
}

// Invoke even retired callbacks: Tauri may already have queued an event at disposal.
async function emit(payload: unknown = { engine: "untrusted-event-engine" }, index = listeners.length - 1) {
  await act(async () => { listeners[index]?.callback({ payload }); });
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  listeners = [];
  subscriptions = [];
  listen.mockImplementation((name: string, callback: Listener["callback"]) => {
    const listener = { name, callback, stop: vi.fn() };
    listeners.push(listener);
    return subscriptions.shift() ?? Promise.resolve(listener.stop);
  });
  api.getEngine.mockResolvedValue("qwen3-asr-0.6b");
  api.getOnlineAsrApiKey.mockResolvedValue("");
  api.getOnlineAsrEndpoint.mockResolvedValue(initialEndpoint);
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "saved-model", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setEngine.mockImplementation(async (engine: string) => engine);
  api.setOnlineAsrApiKey.mockResolvedValue(undefined);
  api.setOnlineAsrEndpoint.mockResolvedValue(currentEndpoint);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("saved ASR engine changes from the native event boundary", () => {
  it("subscribes to invalidations and reads the saved engine instead of trusting the payload", async () => {
    const { result } = await mountSettings();
    api.getEngine.mockResolvedValueOnce("alibaba-asr");
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(currentEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_CURRENT_KEY");
    await emit({ engine: "glm-asr" });

    expect.soft(listen).toHaveBeenCalledWith("engine-config-changed", expect.any(Function));
    expect.soft(result.current.engine).toBe("alibaba-asr");
    expect.soft(result.current.onlineAsrRegion).toBe("domestic");
    expect.soft(result.current.onlineAsrUrl).toBe(currentEndpoint.url);
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_CURRENT_KEY");
    expect.soft(result.current.onlineAsrKeyReady).toBe(true);
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    expect(notices.success).not.toHaveBeenCalled();
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });

  it.each(["success", "failure"] as const)("retires initial engine, endpoint, key and learned-config %s when an event arrives", async (outcome) => {
    const initialEngine = deferred<string>();
    const initialKey = deferred<string>();
    const endpoint = deferred<typeof initialEndpoint>();
    const learned = deferred<{ model: string; models: string[] }>();
    api.getEngine.mockReturnValueOnce(initialEngine.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(initialKey.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getAlibabaAsrConfig.mockReturnValueOnce(learned.promise);
    const { result } = await mountSettings();
    api.getEngine.mockResolvedValueOnce("glm-asr");
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(currentEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_FRESH_KEY");
    await emit();
    const selectedModel = result.current.alibabaAsrModel;
    await act(async () => {
      if (outcome === "success") {
        initialEngine.resolve("alibaba-asr");
        endpoint.resolve(initialEndpoint);
        initialKey.resolve("SYNTHETIC_RETIRED_KEY");
        learned.resolve({ model: "retired-model", models: ["retired-model"] });
      } else {
        initialEngine.reject(new Error("retired engine read failed"));
        endpoint.reject(new Error("retired endpoint read failed"));
        initialKey.reject(new Error("retired key read failed"));
        learned.reject(new Error("retired learned config failed"));
      }
    });

    expect.soft(result.current.engine).toBe("glm-asr");
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_FRESH_KEY");
    expect.soft(result.current.onlineAsrUrl).toBe(currentEndpoint.url);
    expect.soft(result.current.alibabaAsrModel).toBe(selectedModel);
    expect.soft(result.current.asrConfigError).toBe(false);
    expect(result.current.alibabaAsrModels).not.toContain("retired-model");
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("keeps a newer engine read when two engine promises complete backwards", async () => {
    const { result } = await mountSettings();
    const oldEngine = deferred<string>();
    const latestEngine = deferred<string>();
    const latestEndpoint = deferred<typeof currentEndpoint>();
    const latestKey = deferred<string>();
    api.getEngine.mockReturnValueOnce(oldEngine.promise).mockReturnValueOnce(latestEngine.promise);
    api.getOnlineAsrEndpoint.mockReturnValue(latestEndpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValue(latestKey.promise);
    await emit({ engine: "glm-asr" });
    await emit({ engine: "alibaba-asr" });
    await act(async () => { latestEngine.resolve("alibaba-asr"); });
    expect.soft(retryModel).not.toHaveBeenCalled();
    await act(async () => {
      latestEndpoint.resolve(currentEndpoint);
      latestKey.resolve("SYNTHETIC_LATEST_KEY");
    });
    expect.soft(result.current.engine).toBe("alibaba-asr");
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    await act(async () => {
      oldEngine.resolve("glm-asr");
    });
    expect.soft(result.current.engine).toBe("alibaba-asr");
    expect.soft(result.current.onlineAsrUrl).toBe(currentEndpoint.url);
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_LATEST_KEY");
    expect.soft(result.current.engineLoading).toBe(false);
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it.each(["success", "failure"] as const)("keeps all newer fields when independent old config reads finish with %s", async (outcome) => {
    const { result } = await mountSettings();
    const oldEndpoint = deferred<typeof initialEndpoint>();
    const oldKey = deferred<string>();
    api.getEngine.mockResolvedValueOnce("glm-asr");
    api.getOnlineAsrEndpoint.mockReturnValueOnce(oldEndpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(oldKey.promise);
    await emit();
    const latestEngine = deferred<string>();
    const latestEndpoint = deferred<typeof currentEndpoint>();
    const latestKey = deferred<string>();
    api.getEngine.mockReturnValueOnce(latestEngine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(latestEndpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(latestKey.promise);
    await emit();
    await act(async () => { latestEngine.resolve("alibaba-asr"); });
    await act(async () => { latestEndpoint.resolve(currentEndpoint); latestKey.resolve("SYNTHETIC_LATEST_KEY"); });
    expect.soft(result.current.engine).toBe("alibaba-asr");
    expect.soft(result.current.onlineAsrUrl).toBe(currentEndpoint.url);
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_LATEST_KEY");
    expect.soft(result.current.onlineAsrEndpointReady).toBe(true);
    expect.soft(result.current.onlineAsrKeyReady).toBe(true);
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    const fresh = configView(result.current);
    await act(async () => {
      if (outcome === "success") { oldEndpoint.resolve(initialEndpoint); oldKey.resolve("SYNTHETIC_RETIRED_KEY"); }
      else { oldEndpoint.reject(new Error("retired endpoint failed")); oldKey.reject(new Error("retired key failed")); }
    });
    expect(configView(result.current)).toEqual(fresh);
    expect(retryModel).toHaveBeenCalledTimes(1);
    expect(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it.each(["glm-asr", "alibaba-asr"])("retires %s readiness immediately while the saved cloud identity is unknown", async (initialEngine) => {
    api.getEngine.mockResolvedValue(initialEngine);
    const { result } = await mountSettings();
    expect(result.current.onlineAsrKeyReady).toBe(true);
    const engine = deferred<string>();
    const endpoint = deferred<typeof currentEndpoint>();
    const key = deferred<string>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    await emit({ engine: "glm-asr" });
    expect.soft(result.current.onlineAsrEndpointReady).toBe(false);
    expect.soft(result.current.onlineAsrKeyReady).toBe(false);
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_UNCONFIRMED_EDIT"); });
    let regionPending!: Promise<void>;
    await act(async () => { regionPending = result.current.handleOnlineAsrRegionChange("domestic"); await vi.advanceTimersByTimeAsync(600); });
    expect.soft(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    expect.soft(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    await act(async () => { engine.resolve("alibaba-asr"); });
    expect.soft(result.current.onlineAsrKeyReady).toBe(false);
    await act(async () => { endpoint.resolve(currentEndpoint); key.resolve(""); await regionPending; });
    expect.soft(result.current.onlineAsrKeyReady).toBe(true);
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it.each(["success", "failure"] as const)("ignores an older GUI switch %s after a newer saved engine is read", async (outcome) => {
    const { result } = await mountSettings();
    const switching = deferred<string>();
    api.setEngine.mockReturnValueOnce(switching.promise);
    let pending!: Promise<void>;
    await act(async () => { pending = result.current.handleEngineSwitch("glm-asr"); });
    expect(api.setEngine).toHaveBeenCalledWith("glm-asr");
    api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
    await emit({ engine: "glm-asr" });
    await act(async () => {
      if (outcome === "success") switching.resolve("glm-asr");
      else switching.reject(new Error("retired GUI switch failed"));
      await pending;
    });
    expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect.soft(result.current.engineLoading).toBe(false);
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it("keeps the success toast and model refresh for a current GUI switch", async () => {
    const { result } = await mountSettings();
    await act(async () => { await result.current.handleEngineSwitch("confucius4-r2t2"); });
    expect(result.current.engine).toBe("confucius4-r2t2");
    expect(notices.success).toHaveBeenCalledTimes(1);
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("retires a cloud catalog response after the saved engine changes to local", async () => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    api.getOnlineAsrApiKey.mockResolvedValue("SYNTHETIC_STORED_KEY");
    const catalog = deferred<{ models: string[]; source: "live"; warning: string }>();
    api.listAlibabaAsrModels.mockReturnValueOnce(catalog.promise);
    const { result } = await mountSettings();
    expect(api.listAlibabaAsrModels).toHaveBeenCalledTimes(1);
    api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
    api.getOnlineAsrApiKey.mockResolvedValueOnce("");
    await emit();
    await act(async () => { catalog.resolve({ models: ["retired-cloud-model"], source: "live", warning: "Retired catalog warning" }); });
    expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect.soft(result.current.alibabaAsrModelsLoading).toBe(false);
    expect(result.current.alibabaAsrModels).not.toContain("retired-cloud-model");
    expect(notices.warning).not.toHaveBeenCalled();
  });

  it("preserves a key edit made after current engine and endpoint hydration", async () => {
    const { result } = await mountSettings();
    const key = deferred<string>();
    api.getEngine.mockResolvedValueOnce("alibaba-asr");
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(currentEndpoint);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    await emit();
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_USER_DRAFT"); });
    await act(async () => { key.resolve("SYNTHETIC_OLDER_READ"); });
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_USER_DRAFT");
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_USER_DRAFT", "alibaba-asr-cn-api-key");
  });

  it("reports an external read failure and lets the existing retry read the saved config", async () => {
    const { result } = await mountSettings();
    const endpoint = deferred<typeof currentEndpoint>();
    const key = deferred<string>();
    api.getEngine.mockRejectedValueOnce(new Error("synthetic local config read failed"));
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    await emit();
    expect.soft(result.current.asrConfigError).toBe(true);
    expect.soft(result.current.engineLoading).toBe(false);
    expect.soft(retryModel).not.toHaveBeenCalled();
    await act(async () => { endpoint.resolve(currentEndpoint); });
    expect.soft(retryModel).not.toHaveBeenCalled();
    await act(async () => { key.resolve(""); });
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
    await act(async () => { await result.current.retryAsrConfig(); });
    expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect(result.current.asrConfigError).toBe(false);
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it("keeps a queued credential save bound to the confirmed slot across an external engine change", async () => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    const { result } = await mountSettings();
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_QUEUED_DRAFT"); });
    api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
    await emit();
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_QUEUED_DRAFT", "alibaba-asr-intl-api-key");
  });

  it.each([
    ["engine", "event"], ["region", "event"], ["engine", "unmount"], ["region", "unmount"],
  ] as const)("prevents an obsolete %s persistence call after %s during credential flush", async (kind, retirement) => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    const { result, unmount } = await mountSettings();
    const save = deferred<void>();
    api.setOnlineAsrApiKey.mockReturnValueOnce(save.promise);
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_FLUSH_DRAFT"); });
    let pending!: Promise<void>;
    await act(async () => { pending = mutate(result.current, kind); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledExactlyOnceWith("SYNTHETIC_FLUSH_DRAFT", "alibaba-asr-intl-api-key");
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    if (retirement === "event") {
      api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
      await emit();
    } else unmount();
    await act(async () => { save.resolve(); await pending; });
    expect.soft(api.setEngine).not.toHaveBeenCalled();
    expect.soft(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    if (retirement === "event") expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect.soft(retryModel).toHaveBeenCalledTimes(retirement === "event" ? 1 : 0);
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it.each([
    ["engine", "event", "success"], ["engine", "event", "failure"],
    ["region", "event", "success"], ["region", "event", "failure"],
    ["engine", "unmount", "success"], ["engine", "unmount", "failure"],
    ["region", "unmount", "success"], ["region", "unmount", "failure"],
  ] as const)("retires %s mutation after %s despite native %s without disturbing fresh reads", async (kind, retirement, outcome) => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    const { result, unmount } = await mountSettings();
    const mutation = deferred<string | typeof currentEndpoint>();
    if (kind === "engine") api.setEngine.mockReturnValueOnce(mutation.promise);
    else api.setOnlineAsrEndpoint.mockReturnValueOnce(mutation.promise);
    let pending!: Promise<void>;
    await act(async () => { pending = mutate(result.current, kind); });
    const engine = deferred<string>();
    const endpoint = deferred<typeof externalEndpoint>();
    const key = deferred<string>();
    if (retirement === "event") {
      api.getEngine.mockReturnValueOnce(engine.promise);
      api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
      api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
      await emit();
      await act(async () => { engine.resolve("alibaba-asr"); });
    } else unmount();
    const fresh = configView(result.current);
    const reads = { key: api.getOnlineAsrApiKey.mock.calls.length, endpoint: api.getOnlineAsrEndpoint.mock.calls.length };
    await act(async () => {
      if (outcome === "success") mutation.resolve(kind === "engine" ? "glm-asr" : currentEndpoint);
      else mutation.reject(new Error("retired native mutation failed"));
    });
    expect.soft(configView(result.current)).toEqual(fresh);
    expect.soft(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(reads.key);
    expect.soft(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(reads.endpoint);
    expect.soft(retryModel).not.toHaveBeenCalled();
    if (retirement === "event") {
      await act(async () => { endpoint.resolve(externalEndpoint); key.resolve("SYNTHETIC_EXTERNAL_KEY"); await pending; });
      expect.soft(result.current.engine).toBe("alibaba-asr");
      expect.soft(result.current.onlineAsrUrl).toBe(externalEndpoint.url);
      expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_EXTERNAL_KEY");
      expect.soft(result.current.onlineAsrEndpointReady).toBe(true);
      expect.soft(result.current.onlineAsrKeyReady).toBe(true);
      expect.soft(result.current.asrConfigError).toBe(false);
      expect.soft(retryModel).toHaveBeenCalledTimes(1);
    } else await act(async () => { await pending; });
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it.each([
    ["engine", "success"], ["engine", "failure"],
    ["region", "success"], ["region", "failure"],
  ] as const)("keeps the current engine read loading through retired %s mutation %s", async (kind, outcome) => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    const { result } = await mountSettings();
    const mutation = deferred<string | typeof currentEndpoint>();
    if (kind === "engine") api.setEngine.mockReturnValueOnce(mutation.promise);
    else api.setOnlineAsrEndpoint.mockReturnValueOnce(mutation.promise);
    let pending!: Promise<void>;
    await act(async () => { pending = mutate(result.current, kind); });
    const engine = deferred<string>();
    const endpoint = deferred<typeof externalEndpoint>();
    const key = deferred<string>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(key.promise);
    await emit();
    expect.soft(result.current.engineLoading).toBe(true);
    const fresh = configView(result.current);
    const reads = { key: api.getOnlineAsrApiKey.mock.calls.length, endpoint: api.getOnlineAsrEndpoint.mock.calls.length };
    await act(async () => {
      if (outcome === "success") mutation.resolve(kind === "engine" ? "glm-asr" : currentEndpoint);
      else mutation.reject(new Error("retired mutation failed while engine read pending"));
    });
    expect.soft(result.current.engineLoading).toBe(true);
    expect.soft(configView(result.current)).toEqual(fresh);
    expect.soft(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(reads.key);
    expect.soft(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(reads.endpoint);
    expect.soft(retryModel).not.toHaveBeenCalled();
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect.soft(notices.error).not.toHaveBeenCalled();
    await act(async () => {
      engine.resolve("alibaba-asr");
      endpoint.resolve(externalEndpoint);
      key.resolve("SYNTHETIC_EXTERNAL_KEY");
      await pending;
    });
    expect.soft(result.current.engine).toBe("alibaba-asr");
    expect.soft(result.current.engineLoading).toBe(false);
    expect.soft(result.current.onlineAsrUrl).toBe(externalEndpoint.url);
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_EXTERNAL_KEY");
    expect.soft(result.current.onlineAsrEndpointReady).toBe(true);
    expect.soft(result.current.onlineAsrKeyReady).toBe(true);
    expect.soft(result.current.asrConfigError).toBe(false);
    expect(retryModel).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["engine", "event"], ["region", "event"], ["engine", "unmount"], ["region", "unmount"],
  ] as const)("retires %s post-mutation cloud reads after %s", async (kind, retirement) => {
    api.getEngine.mockResolvedValue("alibaba-asr");
    const { result, unmount } = await mountSettings();
    const oldKey = deferred<string>();
    const oldEndpoint = deferred<typeof currentEndpoint>();
    api.getOnlineAsrApiKey.mockReturnValueOnce(oldKey.promise);
    if (kind === "engine") api.getOnlineAsrEndpoint.mockReturnValueOnce(oldEndpoint.promise);
    let pending!: Promise<void>;
    await act(async () => { pending = mutate(result.current, kind); });
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(2);
    notices.success.mockClear();
    notices.error.mockClear();
    if (retirement === "event") {
      api.getEngine.mockResolvedValueOnce("alibaba-asr");
      api.getOnlineAsrEndpoint.mockResolvedValueOnce(externalEndpoint);
      api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_EXTERNAL_KEY");
      await emit();
    } else unmount();
    const fresh = configView(result.current);
    await act(async () => {
      oldKey.reject(new Error("retired post-mutation key read failed"));
      oldEndpoint.resolve(currentEndpoint);
      await pending;
    });
    expect.soft(configView(result.current)).toEqual(fresh);
    if (retirement === "event") {
      expect.soft(result.current.engine).toBe("alibaba-asr");
      expect.soft(result.current.onlineAsrUrl).toBe(externalEndpoint.url);
      expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_EXTERNAL_KEY");
    }
    expect.soft(retryModel).toHaveBeenCalledTimes(retirement === "event" ? 1 : 0);
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it("reports a rejected listener registration without inventing config or calling cloud APIs", async () => {
    const logError = vi.spyOn(console, "error").mockImplementation(() => {});
    const logWarning = vi.spyOn(console, "warn").mockImplementation(() => {});
    listen.mockRejectedValueOnce(new Error("synthetic event subscription failed"));
    const { result } = await mountSettings();
    expect.soft(logError.mock.calls.length + logWarning.mock.calls.length + notices.error.mock.calls.length + notices.warning.mock.calls.length).toBeGreaterThan(0);
    expect(result.current.engine).toBe("qwen3-asr-0.6b");
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
    expect(notices.success).not.toHaveBeenCalled();
  });

  it("disposes the exact late registration and ignores events queued after unmount", async () => {
    const registration = deferred<Stop>();
    subscriptions.push(registration.promise);
    const hook = await mountSettings();
    hook.unmount();
    const reads = api.getEngine.mock.calls.length;
    const stop = vi.fn();
    await act(async () => { registration.resolve(stop); });
    await emit();
    expect.soft(stop).toHaveBeenCalledTimes(1);
    expect(api.getEngine).toHaveBeenCalledTimes(reads);
    expect(retryModel).not.toHaveBeenCalled();
    expect(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
  });

  it("keeps StrictMode replay listeners separate when the old registration resolves late", async () => {
    const registration = deferred<Stop>();
    subscriptions.push(registration.promise);
    const { result, unmount } = await mountSettings({ reactStrictMode: true });
    expect(listeners).toHaveLength(2);
    const reads = api.getEngine.mock.calls.length;
    const oldStop = vi.fn();
    await act(async () => { registration.resolve(oldStop); });
    await emit({ engine: "alibaba-asr" }, 0);
    expect.soft(oldStop).toHaveBeenCalledTimes(1);
    expect.soft(api.getEngine).toHaveBeenCalledTimes(reads);
    api.getEngine.mockResolvedValueOnce("confucius4-r2t2");
    await emit({}, 1);
    expect.soft(result.current.engine).toBe("confucius4-r2t2");
    expect.soft(retryModel).toHaveBeenCalledTimes(1);
    unmount();
    expect(listeners[1]?.stop).toHaveBeenCalledTimes(1);
    expect(oldStop).toHaveBeenCalledTimes(1);
  });

  it("retires in-flight external reads and GUI completion on unmount", async () => {
    const { result, unmount } = await mountSettings();
    const switching = deferred<string>();
    api.setEngine.mockReturnValueOnce(switching.promise);
    let pending!: Promise<void>;
    await act(async () => { pending = result.current.handleEngineSwitch("glm-asr"); });
    const engine = deferred<string>();
    api.getEngine.mockReturnValueOnce(engine.promise);
    await emit();
    unmount();
    await act(async () => { engine.resolve("alibaba-asr"); switching.resolve("glm-asr"); await pending; });
    expect.soft(retryModel).not.toHaveBeenCalled();
    expect.soft(notices.success).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
    expect(api.listAlibabaAsrModels).not.toHaveBeenCalled();
  });
});
