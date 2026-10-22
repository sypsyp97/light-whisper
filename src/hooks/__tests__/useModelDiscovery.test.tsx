import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useModelDiscovery } from "../useModelDiscovery";

const translate = (key: string) => key;
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const payload = (id: string) => ({ models: [{ id }], sourceUrl: `https://${id}.example/models` });
type Payload = ReturnType<typeof payload>;

afterEach(() => { vi.useRealTimers(); });

function setup(assistantSharesPolishModels = false) {
  vi.useFakeTimers();
  const fetchPolishModels = vi.fn<() => Promise<Payload>>().mockResolvedValue(payload("initial"));
  const fetchAssistantModels = vi.fn<() => Promise<Payload>>().mockResolvedValue(payload("assistant"));
  const options = {
    polishModelsContext: "A", polishHasAuth: true, polishMissingAuthMessage: "missing auth",
    fetchPolishModels, assistantModelsContext: "assistant-A", assistantHasAuth: true,
    assistantUseSeparateModel: true, assistantSharesPolishModels, fetchAssistantModels,
  };
  const hook = renderHook((props) => useModelDiscovery(props), { initialProps: options });
  return { ...hook, options, fetchPolishModels, fetchAssistantModels };
}

describe("model discovery publication contracts", () => {
  it("keeps the newer catalog when an older fetch completes later", async () => {
    const hook = setup();
    const old = deferred<Payload>();
    hook.fetchPolishModels.mockReturnValueOnce(old.promise);
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refreshAiModelsNow(); });
    hook.fetchPolishModels.mockResolvedValueOnce(payload("new"));
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    await act(async () => { old.resolve(payload("old")); await pending; });
    expect(hook.result.current.aiModels.map((model) => model.id)).toEqual(["new"]);
    expect(hook.result.current.aiModelsSourceUrl).toContain("new.example");
    expect(hook.result.current.aiModelsLoading).toBe(false);
  });

  it("invalidates old provider and auth responses before publication", async () => {
    const hook = setup();
    const old = deferred<Payload>();
    hook.fetchPolishModels.mockReturnValueOnce(old.promise);
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refreshAiModelsNow(); });
    hook.rerender({ ...hook.options, polishModelsContext: "B", polishHasAuth: false });
    await act(async () => { old.resolve(payload("old-A")); await pending; });
    expect(hook.result.current.aiModels).toEqual([]);
    expect(hook.result.current.aiModelsSourceUrl).toBe("");
    expect(hook.result.current.aiModelsLoading).toBe(false);
  });

  it("retains a same-provider catalog on refresh failure and clears it on context change", async () => {
    const hook = setup();
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    hook.fetchPolishModels.mockRejectedValueOnce(new Error("offline"));
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    expect(hook.result.current.aiModels.map((model) => model.id)).toEqual(["initial"]);
    expect(hook.result.current.aiModelsError).toBe("offline");
    hook.rerender({ ...hook.options, polishModelsContext: "B" });
    expect(hook.result.current.aiModels).toEqual([]);
  });

  it("cancels queued discovery on unmount", async () => {
    const hook = setup();
    hook.unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(hook.fetchPolishModels).not.toHaveBeenCalled();
    expect(hook.fetchAssistantModels).not.toHaveBeenCalled();
  });

  it.each(["polish", "assistant"] as const)("preserves an in-flight %s request when only the other catalog is invalidated", async (catalog) => {
    const hook = setup();
    const response = deferred<Payload>();
    const fetchModels = catalog === "polish" ? hook.fetchPolishModels : hook.fetchAssistantModels;
    const modelsKey = catalog === "polish" ? "aiModels" : "assistantModels";
    const loadingKey = catalog === "polish" ? "aiModelsLoading" : "assistantModelsLoading";
    fetchModels.mockReturnValueOnce(response.promise);
    let pending!: Promise<void>;
    act(() => {
      pending = catalog === "polish"
        ? hook.result.current.refreshAiModelsNow()
        : hook.result.current.refreshAssistantModelsNow();
    });
    act(() => {
      hook.result.current.invalidateModels({
        clearPolish: catalog === "assistant",
        clearAssistant: catalog === "polish",
      });
    });
    expect(hook.result.current[loadingKey]).toBe(true);

    await act(async () => { response.resolve(payload("preserved")); await pending; });
    expect.soft(hook.result.current[modelsKey].map((model) => model.id)).toEqual(["preserved"]);
    expect.soft(hook.result.current[loadingKey]).toBe(false);
    if (catalog === "polish") {
      expect(hook.result.current.aiModelsSourceUrl).toBe(payload("preserved").sourceUrl);
    }
  });

  it.each(["polish", "assistant"] as const)("clears invalidated %s state and rejects its retired response", async (catalog) => {
    const hook = setup();
    const fetchModels = catalog === "polish" ? hook.fetchPolishModels : hook.fetchAssistantModels;
    const modelsKey = catalog === "polish" ? "aiModels" : "assistantModels";
    const loadingKey = catalog === "polish" ? "aiModelsLoading" : "assistantModelsLoading";
    const errorKey = catalog === "polish" ? "aiModelsError" : "assistantModelsError";
    const refresh = () => catalog === "polish"
      ? hook.result.current.refreshAiModelsNow()
      : hook.result.current.refreshAssistantModelsNow();
    const invalidate = () => hook.result.current.invalidateModels({
      clearPolish: catalog === "polish",
      clearAssistant: catalog === "assistant",
    });
    await act(async () => { await refresh(); });
    fetchModels.mockRejectedValueOnce(new Error("catalog offline"));
    await act(async () => { await refresh(); });
    expect(hook.result.current[modelsKey]).toHaveLength(1);
    expect(hook.result.current[errorKey]).toBe("catalog offline");

    act(invalidate);
    expect.soft(hook.result.current[modelsKey]).toEqual([]);
    expect.soft(hook.result.current[errorKey]).toBe("");
    expect.soft(hook.result.current[loadingKey]).toBe(false);
    if (catalog === "polish") {
      expect(hook.result.current.aiModelsSourceUrl).toBe("");
    }

    const retired = deferred<Payload>();
    fetchModels.mockReturnValueOnce(retired.promise);
    let pending!: Promise<void>;
    act(() => { pending = refresh(); });
    expect(hook.result.current[loadingKey]).toBe(true);
    act(invalidate);
    expect.soft(hook.result.current[loadingKey]).toBe(false);
    await act(async () => { retired.resolve(payload("retired")); await pending; });
    expect.soft(hook.result.current[modelsKey]).toEqual([]);
    expect.soft(hook.result.current[errorKey]).toBe("");
    expect(hook.result.current[loadingKey]).toBe(false);
  });

  it("publishes the original independent assistant response without refetching after polish publication", async () => {
    const hook = setup();
    const assistant = deferred<Payload>();
    hook.fetchAssistantModels.mockReturnValueOnce(assistant.promise);
    let pending!: Promise<void>;
    // The immediate refresh cancels the assistant's initially queued 700ms fetch.
    act(() => { pending = hook.result.current.refreshAssistantModelsNow(); });
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    expect(hook.fetchAssistantModels).toHaveBeenCalledTimes(1);
    expect(hook.result.current.assistantModelsLoading).toBe(true);

    await act(async () => { assistant.resolve(payload("original-assistant")); await pending; });
    expect.soft(hook.result.current.assistantModels.map((model) => model.id)).toEqual(["original-assistant"]);
    expect.soft(hook.result.current.assistantModelsLoading).toBe(false);
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });
    expect(hook.fetchAssistantModels).toHaveBeenCalledTimes(1);
  });

  it("keeps shared assistant models synchronized without an independent assistant fetch", async () => {
    const hook = setup(true);
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    await act(async () => { await hook.result.current.refreshAssistantModelsNow(); });
    expect(hook.result.current.assistantModels).toEqual(payload("initial").models);

    hook.fetchPolishModels.mockResolvedValueOnce(payload("updated-polish"));
    await act(async () => { await hook.result.current.refreshAiModelsNow(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });
    expect(hook.result.current.assistantModels).toEqual(payload("updated-polish").models);
    expect(hook.result.current.assistantModelsLoading).toBe(false);
    expect(hook.result.current.assistantModelsError).toBe("");
    expect(hook.fetchAssistantModels).not.toHaveBeenCalled();
  });

  it("keeps the newer independent assistant catalog when an older request completes later", async () => {
    const hook = setup();
    const old = deferred<Payload>();
    hook.fetchAssistantModels.mockReturnValueOnce(old.promise);
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refreshAssistantModelsNow(); });
    hook.fetchAssistantModels.mockResolvedValueOnce(payload("new-assistant"));
    await act(async () => { await hook.result.current.refreshAssistantModelsNow(); });
    await act(async () => { old.resolve(payload("old-assistant")); await pending; });
    expect(hook.result.current.assistantModels.map((model) => model.id)).toEqual(["new-assistant"]);
    expect(hook.result.current.assistantModelsLoading).toBe(false);
  });
});
