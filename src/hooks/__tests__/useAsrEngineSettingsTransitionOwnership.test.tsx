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

type Settings = ReturnType<typeof useAsrEngineSettings>;
type Endpoint = { region: string; url: string };
type LearnedConfig = { model: string; models: string[] };
const initialEndpoint = { region: "international", url: "https://initial.example.test" };
const externalEndpoint = { region: "international", url: "https://external.example.test" };
const guiEndpoint = { region: "domestic", url: "https://gui.example.test" };
const retryModel = vi.fn();
const engineLabel = (engine: string) => engine;
let listeners: (() => void)[];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  void promise.catch(() => undefined);
  return { promise, resolve, reject };
}

async function mountSettings(reactStrictMode = false) {
  const hook = renderHook(() => useAsrEngineSettings({ engineLabel, retryModel }), { reactStrictMode });
  await act(async () => { await Promise.resolve(); });
  return hook;
}

async function emit() {
  await act(async () => {
    const latest = listeners[listeners.length - 1];
    latest?.();
  });
}

const configView = (settings: Settings) => ({
  engine: settings.engine, engineLoading: settings.engineLoading,
  region: settings.onlineAsrRegion, url: settings.onlineAsrUrl, key: settings.onlineAsrApiKey,
  endpointReady: settings.onlineAsrEndpointReady, keyReady: settings.onlineAsrKeyReady,
  regionLoading: settings.onlineAsrRegionLoading, error: settings.asrConfigError,
  retrying: settings.asrConfigRetrying, model: settings.alibabaAsrModel,
});

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers();
  listeners = [];
  listen.mockImplementation((_name: string, callback: () => void) => {
    listeners.push(callback);
    return Promise.resolve(vi.fn());
  });
  api.getEngine.mockResolvedValue("alibaba-asr");
  api.getOnlineAsrEndpoint.mockResolvedValue(initialEndpoint);
  api.getOnlineAsrApiKey.mockResolvedValue("");
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "saved-model", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setEngine.mockResolvedValue(undefined);
  api.setOnlineAsrEndpoint.mockResolvedValue(guiEndpoint);
  api.setOnlineAsrApiKey.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("ASR transition ownership across admitted replacements", () => {
  it.each([
    ["success", "success"], ["failure", "success"],
    ["success", "failure"], ["failure", "failure"],
  ] as const)("keeps the second region transition busy and closed to edits after retired %s, then settles its own %s", async (oldOutcome, currentOutcome) => {
    const { result } = await mountSettings();
    const first = deferred<Endpoint>();
    const second = deferred<Endpoint>();
    api.setOnlineAsrEndpoint.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    let oldTransition!: Promise<void>;
    await act(async () => { oldTransition = result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledTimes(1);

    api.getOnlineAsrEndpoint.mockResolvedValueOnce(externalEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_EXTERNAL_KEY");
    await emit();
    expect(result.current.onlineAsrRegionLoading).toBe(false);
    expect(retryModel).toHaveBeenCalledTimes(1);
    let currentTransition!: Promise<void>;
    await act(async () => { currentTransition = result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect(result.current.onlineAsrRegionLoading).toBe(true);

    await act(async () => {
      if (oldOutcome === "success") first.resolve(guiEndpoint);
      else first.reject(new Error("retired synthetic region failure"));
      await oldTransition;
    });
    expect.soft(result.current.onlineAsrRegionLoading).toBe(true);
    expect.soft(result.current.onlineAsrUrl).toBe(externalEndpoint.url);
    expect.soft(notices.error).not.toHaveBeenCalled();
    await act(async () => {
      result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_BLOCKED_DRAFT");
      await result.current.handleEngineSwitch("glm-asr");
      await result.current.handleOnlineAsrRegionChange("domestic");
    });
    expect.soft(result.current.onlineAsrApiKey).toBe("SYNTHETIC_EXTERNAL_KEY");
    expect.soft(api.setEngine).not.toHaveBeenCalled();
    expect.soft(api.setOnlineAsrEndpoint).toHaveBeenCalledTimes(2);
    expect.soft(api.setOnlineAsrApiKey).not.toHaveBeenCalled();

    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_CURRENT_KEY");
    await act(async () => {
      if (currentOutcome === "success") second.resolve(guiEndpoint);
      else second.reject(new Error("current synthetic region failure"));
      await currentTransition;
    });
    expect.soft(result.current.onlineAsrRegionLoading).toBe(false);
    expect.soft(result.current.engineLoading).toBe(false);
    expect.soft(result.current.onlineAsrRegion).toBe(currentOutcome === "success" ? "domestic" : "international");
    expect.soft(result.current.onlineAsrUrl).toBe(currentOutcome === "success" ? guiEndpoint.url : externalEndpoint.url);
    expect.soft(result.current.onlineAsrApiKey).toBe(currentOutcome === "success" ? "SYNTHETIC_CURRENT_KEY" : "SYNTHETIC_EXTERNAL_KEY");
    expect(notices.error).toHaveBeenCalledTimes(currentOutcome === "failure" ? 1 : 0);
    act(() => { result.current.handleOnlineAsrApiKeyChange("SYNTHETIC_ACCEPTED_DRAFT"); });
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_ACCEPTED_DRAFT");
  });

  it.each(["success", "failure"] as const)("keeps the eager first-effect saved model and catalog retired through StrictMode replay despite late %s", async (outcome) => {
    const oldLearned = deferred<LearnedConfig>();
    api.getAlibabaAsrConfig.mockReturnValueOnce(oldLearned.promise)
      .mockResolvedValueOnce({ model: "current-model", models: ["current-model"] });
    api.getEngine.mockResolvedValue("glm-asr");
    api.getOnlineAsrEndpoint.mockResolvedValue(externalEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValue("SYNTHETIC_CURRENT_KEY");
    const { result } = await mountSettings(true);
    expect(listeners).toHaveLength(2);
    // Both listener ACKs run after StrictMode's first cleanup. Only the live
    // effect can start engine/endpoint/key reads; saved-model reads stay eager.
    expect(api.getAlibabaAsrConfig).toHaveBeenCalledTimes(2);
    expect(api.getEngine).toHaveBeenCalledTimes(1);
    expect(api.getOnlineAsrEndpoint).toHaveBeenCalledTimes(1);
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(1);
    expect(result.current.engine).toBe("glm-asr");
    expect(result.current.onlineAsrEndpointReady).toBe(true);
    expect(result.current.alibabaAsrModel).toBe("current-model");
    expect(result.current.alibabaAsrModels).toEqual(["current-model"]);
    const current = configView(result.current);
    await act(async () => {
      if (outcome === "success") {
        oldLearned.resolve({ model: "retired-model", models: ["retired-model"] });
      } else {
        oldLearned.reject(new Error("retired synthetic saved model read"));
      }
    });
    expect(configView(result.current)).toEqual(current);
    expect(result.current.alibabaAsrModels).toEqual(["current-model"]);
    expect(retryModel).not.toHaveBeenCalled();
    expect(notices.error).not.toHaveBeenCalled();
    expect(notices.success).not.toHaveBeenCalled();
  });

  it.each([
    ["engine", "success"], ["region", "success"],
    ["engine", "failure"], ["region", "failure"],
  ] as const)("keeps initial reads retired after external hydration and a GUI %s transition despite late %s", async (kind, outcome) => {
    const oldEngine = deferred<string>();
    const oldEndpoint = deferred<Endpoint>();
    const oldKey = deferred<string>();
    const oldLearned = deferred<LearnedConfig>();
    api.getEngine.mockReturnValueOnce(oldEngine.promise);
    api.getOnlineAsrEndpoint.mockReturnValueOnce(oldEndpoint.promise);
    api.getOnlineAsrApiKey.mockReturnValueOnce(oldKey.promise);
    api.getAlibabaAsrConfig.mockReturnValueOnce(oldLearned.promise);
    const { result } = await mountSettings();
    expect(result.current.engineLoading).toBe(true);
    api.getOnlineAsrEndpoint.mockResolvedValueOnce(externalEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_EXTERNAL_KEY");
    await emit();
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrEndpointReady).toBe(true);

    api.getOnlineAsrEndpoint.mockResolvedValueOnce(guiEndpoint);
    api.getOnlineAsrApiKey.mockResolvedValueOnce("SYNTHETIC_CURRENT_KEY");
    await act(async () => {
      if (kind === "engine") await result.current.handleEngineSwitch("glm-asr");
      else await result.current.handleOnlineAsrRegionChange("domestic");
    });
    expect(result.current.engine).toBe(kind === "engine" ? "glm-asr" : "alibaba-asr");
    expect(result.current.onlineAsrUrl).toBe(guiEndpoint.url);
    expect(result.current.onlineAsrApiKey).toBe("SYNTHETIC_CURRENT_KEY");
    const current = configView(result.current);
    const notifications = { errors: notices.error.mock.calls.length, successes: notices.success.mock.calls.length };
    await act(async () => {
      if (outcome === "success") {
        oldEngine.resolve("qwen3-asr-0.6b");
        oldEndpoint.resolve(initialEndpoint);
        oldKey.resolve("SYNTHETIC_RETIRED_KEY");
        oldLearned.resolve({ model: "retired-model", models: ["retired-model"] });
      } else {
        oldEngine.reject(new Error("retired synthetic initial engine"));
        oldEndpoint.reject(new Error("retired synthetic initial endpoint"));
        oldKey.reject(new Error("retired synthetic initial key"));
        oldLearned.reject(new Error("retired synthetic learned config"));
      }
    });
    expect(configView(result.current)).toEqual(current);
    expect(notices.error).toHaveBeenCalledTimes(notifications.errors);
    expect(notices.success).toHaveBeenCalledTimes(notifications.successes);
    expect(retryModel).toHaveBeenCalledTimes(kind === "engine" ? 2 : 1);
  });
});
