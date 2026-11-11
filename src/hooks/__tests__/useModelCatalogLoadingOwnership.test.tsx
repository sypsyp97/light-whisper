import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useModelCatalog } from "../useModelCatalog";
import type { AiModelListPayload } from "@/types";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});

function deferred() {
  let resolve!: (value: AiModelListPayload) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<AiModelListPayload>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const payload = (id: string): AiModelListPayload => ({ models: [{ id }], sourceUrl: `https://${id}.example/models` });
function settle(request: ReturnType<typeof deferred>, outcome: "resolve" | "reject", id: string) {
  if (outcome === "resolve") request.resolve(payload(id));
  else request.reject(new Error(`${id} unavailable`));
}
afterEach(cleanup);

describe("catalog loading ownership", () => {
  it.each(["resolve", "reject"] as const)("settles loading at context retirement before old %s", async (outcome) => {
    const old = deferred();
    const fetchModels = vi.fn().mockReturnValueOnce(old.promise);
    const hook = renderHook((context: string) => useModelCatalog({ context, fetchModels }), { initialProps: "A" });
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refresh(); });
    expect(hook.result.current.loading).toBe(true);

    hook.rerender("B");

    expect.soft(hook.result.current.loading).toBe(false);
    expect(hook.result.current.models).toEqual([]);
    await act(async () => { settle(old, outcome, "retired-A"); await pending; });
    expect.soft(hook.result.current.loading).toBe(false);
    expect(hook.result.current.models).toEqual([]);
    expect(hook.result.current.error).toBe("");
    expect(hook.result.current.sourceUrl).toBe("");
  });

  it.each([
    ["resolve", "resolve"], ["resolve", "reject"], ["reject", "resolve"], ["reject", "reject"],
  ] as const)("old %s cannot finish a new request, whose %s settles loading", async (oldOutcome, currentOutcome) => {
    const old = deferred();
    const current = deferred();
    const fetchModels = vi.fn().mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
    const hook = renderHook((context: string) => useModelCatalog({ context, fetchModels }), { initialProps: "A" });
    let oldPending!: Promise<void>;
    let currentPending!: Promise<void>;
    act(() => { oldPending = hook.result.current.refresh(); });
    hook.rerender("B");
    act(() => { currentPending = hook.result.current.refresh(); });
    expect(hook.result.current.loading).toBe(true);

    await act(async () => { settle(old, oldOutcome, "retired-A"); await oldPending; });

    expect(hook.result.current.loading).toBe(true);
    expect(hook.result.current.models).toEqual([]);
    expect(hook.result.current.error).toBe("");
    expect(hook.result.current.sourceUrl).toBe("");
    await act(async () => { settle(current, currentOutcome, "current-B"); await currentPending; });
    expect(hook.result.current.loading).toBe(false);
    if (currentOutcome === "resolve") {
      expect(hook.result.current.models).toEqual(payload("current-B").models);
      expect(hook.result.current.sourceUrl).toBe(payload("current-B").sourceUrl);
      expect(hook.result.current.error).toBe("");
    } else {
      expect(hook.result.current.models).toEqual([]);
      expect(hook.result.current.error).toBe("current-B unavailable");
    }
  });

  it.each(["resolve", "reject"] as const)("keeps an explicit reset settled after retired %s", async (outcome) => {
    const old = deferred();
    const fetchModels = vi.fn().mockReturnValueOnce(old.promise);
    const hook = renderHook(() => useModelCatalog({ context: "A", fetchModels }));
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refresh(); });
    act(() => { hook.result.current.reset("Missing credentials"); });
    expect(hook.result.current.loading).toBe(false);
    await act(async () => { settle(old, outcome, "retired-A"); await pending; });
    expect(hook.result.current.loading).toBe(false);
    expect(hook.result.current.models).toEqual([]);
    expect(hook.result.current.error).toBe("Missing credentials");
  });
});
