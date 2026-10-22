import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ownershipTrace } from "@/test/asyncOwnershipTrace";
import { useModelDiscovery } from "../useModelDiscovery";

const translate = (key: string) => key;
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const payload = (id: string) => ({ models: [{ id }], sourceUrl: `https://${id}.example/models` });
type Role = "polish" | "assistant";
type Payload = ReturnType<typeof payload>;
afterEach(() => { vi.useRealTimers(); });

describe("TLC catalog request ownership replay", () => {
  it.each(["catalog-global-invalidation", "catalog-polish-independent"])("replays %s through the real hook", async (name) => {
    vi.useFakeTimers();
    const events = ownershipTrace(name);
    const fetchers = {
      polish: vi.fn<() => Promise<Payload>>(),
      assistant: vi.fn<() => Promise<Payload>>(),
    };
    const hook = renderHook(() => useModelDiscovery({
      polishModelsContext: "polish-A", polishHasAuth: true,
      polishMissingAuthMessage: "missing auth", fetchPolishModels: fetchers.polish,
      assistantModelsContext: "assistant-A", assistantHasAuth: true,
      assistantUseSeparateModel: true, assistantSharesPolishModels: false,
      fetchAssistantModels: fetchers.assistant,
    }));
    const requests = new Map<string, ReturnType<typeof deferred<Payload>>>();
    const completions = new Map<string, Promise<void>>();
    const expected = {
      polish: { owner: "", models: [] as string[], error: "", loading: false },
      assistant: { owner: "", models: [] as string[], error: "", loading: false },
    };
    const handled: string[] = [];
    for (const event of events) {
      const match = /^catalog:(polish|assistant):(begin|success|failure):([12])$/.exec(event);
      const invalidation = /^catalog:(polish|assistant):invalidate$/.exec(event);
      if (match) {
        const role = match[1] as Role;
        const key = `${role}-${match[3]}`;
        if (match[2] === "begin") {
          const request = deferred<Payload>();
          requests.set(key, request);
          fetchers[role].mockReturnValueOnce(request.promise);
          expected[role].owner = key;
          expected[role].loading = true;
          expected[role].error = "";
          act(() => {
            completions.set(key, role === "polish"
              ? hook.result.current.refreshAiModelsNow()
              : hook.result.current.refreshAssistantModelsNow());
          });
        } else {
          const request = requests.get(key);
          const completion = completions.get(key);
          if (!request || !completion) throw new Error(`Completion without request: ${event}`);
          if (expected[role].owner === key) {
            if (match[2] === "success") expected[role].models = [key];
            expected[role].error = match[2] === "failure" ? `${key} offline` : "";
            expected[role].loading = false;
          }
          await act(async () => {
            if (match[2] === "success") request.resolve(payload(key));
            else request.reject(new Error(`${key} offline`));
            await completion;
          });
        }
      } else if (invalidation) {
        const role = invalidation[1] as Role;
        expected[role] = { owner: "", models: [], error: "", loading: false };
        act(() => hook.result.current.invalidateModels({
          clearPolish: role === "polish", clearAssistant: role === "assistant",
        }));
      } else {
        throw new Error(`Unhandled TLC catalog event: ${event}`);
      }
      expect(hook.result.current.aiModels.map((model) => model.id), event).toEqual(expected.polish.models);
      expect(hook.result.current.aiModelsError, event).toBe(expected.polish.error);
      expect(hook.result.current.aiModelsLoading, event).toBe(expected.polish.loading);
      expect(hook.result.current.aiModelsSourceUrl, event).toBe(expected.polish.models.length
        ? payload(expected.polish.models[0]).sourceUrl : "");
      expect(hook.result.current.assistantModels.map((model) => model.id), event).toEqual(expected.assistant.models);
      expect(hook.result.current.assistantModelsError, event).toBe(expected.assistant.error);
      expect(hook.result.current.assistantModelsLoading, event).toBe(expected.assistant.loading);
      handled.push(event);
    }
    expect(handled).toEqual(events);
    const calls = [fetchers.polish.mock.calls.length, fetchers.assistant.mock.calls.length];
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });
    expect([fetchers.polish.mock.calls.length, fetchers.assistant.mock.calls.length]).toEqual(calls);
    hook.unmount();
  });
});
