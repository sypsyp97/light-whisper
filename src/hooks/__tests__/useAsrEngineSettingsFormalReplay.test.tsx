import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ownershipTrace } from "@/test/asyncOwnershipTrace";
import { useAsrEngineSettings } from "@/hooks/useAsrEngineSettings";

const api = vi.hoisted(() => ({
  getEngine: vi.fn(), getOnlineAsrApiKey: vi.fn(), getOnlineAsrEndpoint: vi.fn(),
  getAlibabaAsrConfig: vi.fn(), listAlibabaAsrModels: vi.fn(), setAlibabaAsrModel: vi.fn(),
  setEngine: vi.fn(), setOnlineAsrApiKey: vi.fn(), setOnlineAsrEndpoint: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const initialNames = ["online-asr-identity-initial", "online-asr-identity-recovery", "online-asr-identity-retry-owner"];
const engineName = (value: string) => value === "local" ? "qwen3-asr-0.6b" : `${value}-asr`;
const endpointFor = (engine: string) => engine === "alibaba"
  ? { region: "domestic", url: "https://domestic.example.test" }
  : { region: "international", url: "https://international.example.test" };
const slotFor = (engine: string) => engine === "alibaba" ? "alibaba-asr-cn-api-key" : "glm-asr-api-key";

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "qwen-audio-3.1-asr-flash", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
});
afterEach(() => { vi.useRealTimers(); });

describe("TLC online ASR identity replay", () => {
  it.each([
    "online-asr-identity-initial", "online-asr-identity-switch", "online-asr-identity-stale",
    "online-asr-identity-key-failure", "online-asr-identity-flush", "online-asr-identity-recovery",
    "online-asr-identity-retry-owner", "online-asr-identity-sibling",
  ])("replays %s through the real hook", async (name) => {
    const events = ownershipTrace(name);
    let backend = initialNames.includes(name) ? "alibaba" : "glm";
    let context = 0;
    let batch = 0;
    let keyVersion = 0;
    type Part = "engine" | "endpoint" | "key";
    const errors = { engine: false, endpoint: false, key: false };
    const reads = Array.from({ length: 3 }, () => ({
      engine: deferred<string>(), endpoint: deferred<ReturnType<typeof endpointFor>>(),
      key: deferred<string>(), backend: "", context: -1, keyVersion: 0,
      requested: { engine: false, endpoint: false, key: false },
      outcomes: { engine: "pending", endpoint: "pending", key: "pending" },
    }));
    reads[0].backend = backend;
    reads[0].context = context;
    const requested = (part: Part) => {
      reads[batch].requested[part] = true;
      reads[batch].keyVersion = keyVersion;
      return reads[batch];
    };
    api.getEngine.mockImplementation(() => requested("engine").engine.promise);
    api.getOnlineAsrEndpoint.mockImplementation(() => requested("endpoint").endpoint.promise);
    api.getOnlineAsrApiKey.mockImplementation(() => requested("key").key.promise);
    const mutation = deferred<string>();
    api.setEngine.mockReturnValue(mutation.promise);
    const saves: ReturnType<typeof deferred<void>>[] = [];
    api.setOnlineAsrApiKey.mockImplementation(() => {
      const save = deferred<void>();
      saves.push(save);
      return save.promise;
    });
    const hook = renderHook(() => useAsrEngineSettings({ engineLabel: (value) => value, retryModel: vi.fn() }));

    // Independent oracle: successful environmental responses establish identity;
    // displayed hook defaults and returned readiness flags never authorize a write.
    let engineKnown = false;
    let endpointKnown = false;
    let displayedEngine = "qwen3-asr-0.6b";
    let displayedRegion = "international";
    let displayedUrl = "";
    let busy = false;
    let draft: [string, string] | undefined;
    const expectedWrites: [string, string][] = [];
    let saveCursor = 0;
    let switching: Promise<void> | undefined;
    let retrying: Promise<void> | undefined;
    const ready = () => engineKnown && (backend === "glm" || (backend === "alibaba" && endpointKnown));
    const ledger = () => expect(api.setOnlineAsrApiKey.mock.calls).toEqual(expectedWrites);
    const dispatch = () => {
      if (draft) { expectedWrites.push(draft); draft = undefined; }
    };
    const handled: string[] = [];
    for (const event of events) {
      const [kind, idOrOutcome, outcome] = event.split(":");
      if (["engine", "endpoint", "key"].includes(kind)) {
        const id = Number(idOrOutcome);
        const request = reads[id];
        if (!request.requested[kind as Part]) throw new Error(`Completion without requested field: ${event}`);
        const ok = outcome === "success";
        request.outcomes[kind as Part] = outcome;
        await act(async () => {
          if (kind === "engine") {
            if (ok) request.engine.resolve(engineName(request.backend));
            else request.engine.reject(new Error("engine read failed"));
          } else if (kind === "endpoint") {
            if (ok) request.endpoint.resolve(endpointFor(request.backend));
            else request.endpoint.reject(new Error("endpoint read failed"));
          } else {
            if (ok) request.key.resolve("");
            else request.key.reject(new Error("key read failed"));
          }
        });
        if (request.context === context) {
          if (kind === "engine" && ok) {
            engineKnown = true;
            displayedEngine = engineName(request.backend);
          }
          if (kind === "endpoint" && ok) {
            endpointKnown = true;
            displayedRegion = endpointFor(request.backend).region;
            displayedUrl = endpointFor(request.backend).url;
          }
          if (!ok && (kind !== "key" || request.keyVersion === keyVersion)) errors[kind as Part] = true;
        }
      } else if (kind === "edit") {
        const value = idOrOutcome === "clear" ? "" : "FORMAL_FAKE_DRAFT";
        if (ready() && !busy) {
          draft = [value, slotFor(backend)];
          keyVersion += 1;
          errors.key = false;
        }
        act(() => { hook.result.current.handleOnlineAsrApiKeyChange(value); });
      } else if (kind === "timer") {
        const before = expectedWrites.length;
        await act(async () => { await vi.advanceTimersByTimeAsync(599); });
        expect(api.setOnlineAsrApiKey).toHaveBeenCalledTimes(before);
        dispatch();
        await act(async () => { await vi.advanceTimersByTimeAsync(1); });
        ledger();
      } else if (kind === "write") {
        const save = saves[saveCursor++];
        if (!save) throw new Error(`Completion without dispatched save: ${event}`);
        await act(async () => {
          if (idOrOutcome === "success") save.resolve();
          else save.reject(new Error("save failed"));
        });
        if (idOrOutcome === "failure" && busy) { busy = false; }
      } else if (event === "switch:begin") {
        busy = true;
        dispatch();
        await act(async () => { switching = hook.result.current.handleEngineSwitch("alibaba-asr"); });
        ledger();
        if (saveCursor < saves.length) expect(api.setEngine).not.toHaveBeenCalled();
      } else if (event === "switch:success") {
        expect(api.setEngine).toHaveBeenCalledTimes(1);
        backend = "alibaba";
        context += 1;
        batch += 1;
        reads[batch].backend = backend;
        reads[batch].context = context;
        engineKnown = true;
        endpointKnown = false;
        errors.engine = false;
        errors.endpoint = false;
        errors.key = false;
        displayedEngine = "alibaba-asr";
        displayedUrl = "";
        await act(async () => { mutation.resolve("alibaba-asr"); });
      } else if (event === "switch:failure") {
        await act(async () => { mutation.reject(new Error("switch failed")); await switching; });
        busy = false;
      } else if (event === "switch:settled") {
        // Check the visible release before awaiting the aggregate. Replacing
        // Promise.all with allSettled must fail here, not via a test timeout.
        expect(hook.result.current.engineLoading).toBe(false);
        await act(async () => { await switching; });
        busy = false;
      } else if (event === "retry:begin") {
        const writesBefore = api.setEngine.mock.calls.length;
        const retryParts = { ...errors };
        batch += 1;
        reads[batch].backend = backend;
        reads[batch].context = context;
        errors.engine = false;
        errors.endpoint = false;
        errors.key = false;
        await act(async () => { retrying = hook.result.current.retryAsrConfig(); });
        expect(api.setEngine).toHaveBeenCalledTimes(writesBefore);
        expect(reads[batch].requested).toEqual(retryParts);
      } else {
        throw new Error(`Unhandled TLC online ASR event: ${event}`);
      }
      expect(hook.result.current.engine).toBe(displayedEngine);
      expect(hook.result.current.onlineAsrRegion).toBe(displayedRegion);
      expect(hook.result.current.onlineAsrUrl).toBe(displayedUrl);
      expect(hook.result.current.onlineAsrKeyReady).toBe(ready());
      expect(hook.result.current.onlineAsrEndpointReady).toBe(engineKnown && endpointKnown);
      expect(hook.result.current.asrConfigError).toBe(Object.values(errors).some(Boolean));
      ledger();
      handled.push(event);
    }
    expect(handled).toEqual(events);
    if (name === "online-asr-identity-recovery" || name === "online-asr-identity-sibling") {
      expect(ready()).toBe(true);
      expect(expectedWrites).not.toEqual([]);
      expect(expectedWrites[expectedWrites.length - 1]?.[1]).toBe("alibaba-asr-cn-api-key");
    }
    // Unmount preserves the real debouncer's flush behavior. Outstanding reads
    // remain unresolved; cancellation is never used to make this replay pass.
    dispatch();
    hook.unmount();
    await act(async () => { for (const save of saves) save.resolve(); });
    ledger();
    void retrying;
  });
});
