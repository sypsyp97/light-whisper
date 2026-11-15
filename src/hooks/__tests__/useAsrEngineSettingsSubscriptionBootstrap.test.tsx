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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const initial = {
  engine: "qwen3-asr-0.6b",
  endpoint: { region: "international", url: "https://initial.example.test" },
  key: "SYNTHETIC_INITIAL_KEY",
};
const replacement = {
  engine: "confucius4-r2t2",
  endpoint: { region: "domestic", url: "https://saved.example.test" },
  key: "SYNTHETIC_SAVED_KEY",
};
type SavedConfig = typeof initial;
type Settings = ReturnType<typeof useAsrEngineSettings>;
const retryModel = vi.fn();
const engineLabel = (engine: string) => engine;

// Tauri registers the native subscription asynchronously. Its native install
// can precede the JS promise acknowledgment; calling listen alone installs none.
let registration: ReturnType<typeof deferred<() => void>>;
let callback: (() => void) | undefined;
let nativeInstalled: boolean;
let acknowledged: boolean;
let saved: SavedConfig;
let holdInitialReads: boolean;
let heldConfig: SavedConfig | undefined;
let oldReads: (() => void)[];
let deliveredEvents: number;
let stop: ReturnType<typeof vi.fn<() => void>>;

function readSaved<K extends keyof SavedConfig>(field: K): Promise<SavedConfig[K]> {
  const snapshot = saved[field];
  if ((holdInitialReads && !acknowledged && saved === initial) || saved === heldConfig) {
    const pending = deferred<SavedConfig[K]>();
    oldReads.push(() => pending.resolve(snapshot));
    return pending.promise;
  }
  return Promise.resolve(snapshot);
}

function nativeSave(config: SavedConfig) {
  saved = config;
  if (nativeInstalled) {
    deliveredEvents += 1;
    callback?.();
  }
}

async function installAndAcknowledge() {
  await act(async () => {
    nativeInstalled = true;
    acknowledged = true;
    registration.resolve(stop);
  });
}

async function completeOldReads() {
  await act(async () => { oldReads.splice(0).forEach((complete) => complete()); });
}

async function mountSettings(observe?: (settings: Settings) => void) {
  const hook = renderHook(() => {
    const settings = useAsrEngineSettings({ engineLabel, retryModel });
    observe?.(settings);
    return settings;
  });
  await act(async () => { await Promise.resolve(); });
  expect(listen).toHaveBeenCalledWith("engine-config-changed", expect.any(Function));
  return hook;
}

function expectSavedConfig(settings: Settings, config: SavedConfig) {
  expect.soft(settings.engine).toBe(config.engine);
  expect.soft(settings.onlineAsrRegion).toBe(config.endpoint.region);
  expect.soft(settings.onlineAsrUrl).toBe(config.endpoint.url);
  expect.soft(settings.onlineAsrApiKey).toBe(config.key);
  expect.soft(settings.engineLoading).toBe(false);
  expect.soft(settings.onlineAsrEndpointReady).toBe(true);
  expect(settings.asrConfigError).toBe(false);
}

beforeEach(() => {
  vi.resetAllMocks();
  registration = deferred<() => void>();
  callback = undefined;
  nativeInstalled = false;
  acknowledged = false;
  saved = initial;
  holdInitialReads = true;
  heldConfig = undefined;
  oldReads = [];
  deliveredEvents = 0;
  stop = vi.fn(() => { nativeInstalled = false; });
  listen.mockImplementation((_event: string, handler: () => void) => {
    callback = handler;
    return registration.promise;
  });
  api.getEngine.mockImplementation(() => readSaved("engine"));
  api.getOnlineAsrEndpoint.mockImplementation(() => readSaved("endpoint"));
  api.getOnlineAsrApiKey.mockImplementation(() => readSaved("key"));
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "saved-model", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setEngine.mockImplementation(async (engine: string) => engine);
  api.setOnlineAsrEndpoint.mockResolvedValue(replacement.endpoint);
  api.setOnlineAsrApiKey.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("ASR invalidation subscription bootstrap", () => {
  it.each(["before native install", "after subscription acknowledgment"] as const)(
    "hydrates the saved engine when an invalidation was missed and the initial snapshot finishes %s",
    async (completion) => {
      const { result } = await mountSettings();
      await act(async () => { nativeSave(replacement); });
      expect(deliveredEvents).toBe(0);
      if (completion === "before native install") await completeOldReads();
      await installAndAcknowledge();
      if (completion === "after subscription acknowledgment") await completeOldReads();

      // No polling, second event, GUI interaction or explicit retry repairs this.
      expectSavedConfig(result.current, replacement);
      expect(api.setEngine).not.toHaveBeenCalled();
    },
  );

  it("keeps acknowledged hydration usable and reports a failed subscription", async () => {
    holdInitialReads = false;
    saved = replacement;
    const diagnostic = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const { result } = await mountSettings();
    const error = new Error("synthetic native subscription failure");
    await act(async () => { registration.reject(error); });

    expectSavedConfig(result.current, replacement);
    expect(diagnostic).toHaveBeenCalledWith(expect.any(String), error);
    expect(stop).not.toHaveBeenCalled();
  });

  it("releases a listener acknowledged after unmount without starting more configuration reads", async () => {
    const { unmount } = await mountSettings();
    unmount();
    const readsAtUnmount = [api.getEngine, api.getOnlineAsrEndpoint, api.getOnlineAsrApiKey]
      .map((read) => read.mock.calls.length);
    await installAndAcknowledge();
    await act(async () => { callback?.(); });
    await completeOldReads();

    expect(stop).toHaveBeenCalledTimes(1);
    expect([api.getEngine, api.getOnlineAsrEndpoint, api.getOnlineAsrApiKey]
      .map((read) => read.mock.calls.length)).toEqual(readsAtUnmount);
    expect(retryModel).not.toHaveBeenCalled();
  });

  it.each(["before subscription acknowledgment", "after subscription acknowledgment"] as const)(
    "preserves a native event delivered before acknowledgment when initial snapshots finish %s",
    async (completion) => {
      const { result } = await mountSettings();
      await act(async () => {
        nativeInstalled = true;
        nativeSave(replacement);
      });
      expect(deliveredEvents).toBe(1);
      expectSavedConfig(result.current, replacement);
      if (completion === "before subscription acknowledgment") await completeOldReads();
      await installAndAcknowledge();
      if (completion === "after subscription acknowledgment") await completeOldReads();

      expectSavedConfig(result.current, replacement);
      expect(notices.error).not.toHaveBeenCalled();
    },
  );

  it("does not regress a newer published snapshot when an older pre-ACK event read finishes", async () => {
    const publications: { engine: string; url: string }[] = [];
    const { result } = await mountSettings((settings) => {
      publications.push({ engine: settings.engine, url: settings.onlineAsrUrl });
    });
    heldConfig = replacement;
    await act(async () => {
      nativeInstalled = true;
      nativeSave(replacement);
    });
    const latest = {
      engine: "qwen3-asr-0.6b",
      endpoint: { region: "international", url: "https://second-save.example.test" },
      key: "SYNTHETIC_SECOND_SAVE_KEY",
    };
    // Native write and emit are separate: production awaits engine shutdown
    // between them. ACK may occur while the second invalidation is not emitted.
    saved = latest;
    await installAndAcknowledge();
    await completeOldReads();
    await act(async () => { nativeSave(latest); });

    expectSavedConfig(result.current, latest);
    const authoritative = publications.findIndex((view) => view.url === latest.endpoint.url);
    expect(authoritative).toBeGreaterThanOrEqual(0);
    expect(publications.slice(authoritative)).not.toContainEqual({
      engine: replacement.engine, url: replacement.endpoint.url,
    });
  });
});
