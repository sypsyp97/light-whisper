import type { MouseEvent as ReactMouseEvent } from "react";
import { flushSync } from "react-dom";
import { act, fireEvent, render, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type SaveStatus = { revision: number; state: "saved" | "saving" | "error" };
type SaveHandler = (event: { payload: SaveStatus }) => void;
type ToastOptions = {
  id?: string | number;
  duration?: number;
  action?: { label: unknown; onClick: (event: ReactMouseEvent<HTMLButtonElement>) => unknown };
};

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
  getStatus: vi.fn<() => Promise<SaveStatus>>(),
  retry: vi.fn<() => Promise<void>>(),
  error: vi.fn<(message: unknown, options?: ToastOptions) => string | number>(),
  dismiss: vi.fn(),
  success: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/api/tauri", () => ({
  getProfileSaveStatus: mocks.getStatus,
  retryProfileSave: mocks.retry,
}));
vi.mock("sonner", () => ({
  toast: { error: mocks.error, dismiss: mocks.dismiss, success: mocks.success },
}));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));

import { useProfileSaveNotifications } from "../useProfileSaveNotifications";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

let handler: SaveHandler | undefined;
const unlisten = vi.fn();
const activeToasts = new Map<string | number, ToastOptions>();
let nextToastId = 1;

async function flush() {
  await act(async () => {
    await Promise.resolve();
  });
}

function emit(status: SaveStatus) {
  expect(handler).toEqual(expect.any(Function));
  act(() => handler!({ payload: status }));
}

function retryAction() {
  const options = mocks.error.mock.lastCall?.[1];
  expect(options?.duration).toBe(Infinity);
  expect(options?.action?.onClick).toEqual(expect.any(Function));
  return options!.action!.onClick;
}

function clickRetry() {
  const action = retryAction();
  const toastId = mocks.error.mock.results[mocks.error.mock.results.length - 1].value;
  let clickEvent: ReactMouseEvent<HTMLButtonElement> | undefined;
  let completion: unknown;
  const button = flushSync(() => render(
    <button onClick={(event) => {
      clickEvent = event;
      completion = action(event);
      // Sonner 1.7 invokes the action, then dismisses unless prevented synchronously.
      if (!event.defaultPrevented) activeToasts.delete(toastId);
    }}>Retry</button>,
  ));
  fireEvent.click(button.getByRole("button", { name: "Retry" }));
  button.unmount();
  expect(clickEvent?.cancelable).toBe(true);
  expect(clickEvent?.defaultPrevented).toBe(true);
  return completion;
}

beforeEach(() => {
  vi.resetAllMocks();
  handler = undefined;
  activeToasts.clear();
  nextToastId = 1;
  mocks.listen.mockImplementation(async (_event: string, callback: SaveHandler) => {
    handler = callback;
    return unlisten;
  });
  mocks.getStatus.mockResolvedValue({ revision: 0, state: "saved" });
  mocks.retry.mockResolvedValue(undefined);
  mocks.error.mockImplementation((_message, options) => {
    const id = options?.id ?? nextToastId++;
    activeToasts.set(id, options ?? {});
    return id;
  });
  mocks.dismiss.mockImplementation((id?: string | number) => {
    if (id === undefined) activeToasts.clear();
    else activeToasts.delete(id);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useProfileSaveNotifications", () => {
  it("subscribes before fetching the initial status and exposes an existing error", async () => {
    const subscription = deferred<() => void>();
    mocks.listen.mockReturnValueOnce(subscription.promise);
    mocks.getStatus.mockResolvedValue({ revision: 2, state: "error" });

    const hook = renderHook(() => useProfileSaveNotifications());
    expect(mocks.listen).toHaveBeenCalledWith("profile-save-status", expect.any(Function));
    expect(mocks.getStatus).not.toHaveBeenCalled();
    await act(async () => subscription.resolve(unlisten));

    expect(mocks.getStatus).toHaveBeenCalledOnce();
    expect(mocks.error).toHaveBeenCalledOnce();
    retryAction();
    expect(mocks.success).not.toHaveBeenCalled();
    hook.unmount();
    expect(unlisten).toHaveBeenCalledOnce();
  });

  it("keeps a failed retry actionable and dismisses only after a saved status", async () => {
    const firstRetry = deferred<void>();
    mocks.retry.mockReturnValueOnce(firstRetry.promise);
    const hook = renderHook(() => useProfileSaveNotifications());
    await flush();
    mocks.dismiss.mockClear();
    emit({ revision: 1, state: "error" });
    const toastId = mocks.error.mock.results[mocks.error.mock.results.length - 1].value;

    await act(async () => { void clickRetry(); });
    expect(mocks.retry).toHaveBeenCalledOnce();
    expect([...activeToasts.keys()]).toEqual([toastId]);
    emit({ revision: 2, state: "saving" });
    expect(mocks.dismiss).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
    await act(async () => firstRetry.reject(new Error("disk still unavailable")));
    emit({ revision: 3, state: "error" });
    expect(mocks.dismiss).not.toHaveBeenCalled();
    expect([...activeToasts.keys()]).toEqual([toastId]);
    expect(new Set(mocks.error.mock.results.map(({ value }) => value))).toEqual(new Set([toastId]));
    await act(async () => { await clickRetry(); });
    expect(mocks.retry).toHaveBeenCalledTimes(2);
    expect(mocks.dismiss).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
    expect([...activeToasts.keys()]).toEqual([toastId]);

    emit({ revision: 4, state: "saved" });
    expect(mocks.dismiss).toHaveBeenCalledWith(toastId);
    expect(activeToasts.size).toBe(0);
    hook.unmount();
  });

  it("does not resurrect an older retry error after a newer successful save while mounted", async () => {
    const retry = deferred<void>();
    mocks.retry.mockReturnValueOnce(retry.promise);
    const hook = renderHook(() => useProfileSaveNotifications());
    await flush();
    emit({ revision: 1, state: "error" });
    const toastId = mocks.error.mock.results[mocks.error.mock.results.length - 1].value;
    await act(async () => { void clickRetry(); });
    expect(mocks.retry).toHaveBeenCalledOnce();

    emit({ revision: 2, state: "saving" });
    emit({ revision: 3, state: "saved" });
    expect(mocks.dismiss).toHaveBeenCalledWith(toastId);
    expect(activeToasts.size).toBe(0);
    const notificationsAtSuccess = mocks.error.mock.calls.length;
    await act(async () => retry.reject(new Error("older write failed")));

    expect(mocks.error).toHaveBeenCalledTimes(notificationsAtSuccess);
    expect(activeToasts.size).toBe(0);
    expect(mocks.success).not.toHaveBeenCalled();
    hook.unmount();
  });

  it("ignores stale bootstrap and event snapshots after a newer failure", async () => {
    const bootstrap = deferred<SaveStatus>();
    mocks.getStatus.mockReturnValueOnce(bootstrap.promise);
    const hook = renderHook(() => useProfileSaveNotifications());
    await flush();
    expect(mocks.getStatus).toHaveBeenCalledOnce();

    emit({ revision: 5, state: "error" });
    expect(mocks.error).toHaveBeenCalledOnce();
    const toastId = mocks.error.mock.results[0].value;
    await act(async () => bootstrap.resolve({ revision: 1, state: "saved" }));
    emit({ revision: 4, state: "saved" });
    emit({ revision: 5, state: "error" });
    expect(mocks.dismiss).not.toHaveBeenCalled();
    expect(mocks.error).toHaveBeenCalledOnce();

    emit({ revision: 6, state: "saved" });
    expect(mocks.dismiss).toHaveBeenCalledWith(toastId);
    emit({ revision: 3, state: "error" });
    expect(mocks.error).toHaveBeenCalledOnce();
    hook.unmount();
  });

  it("does not resurrect an old bootstrap error after a newer successful save", async () => {
    const bootstrap = deferred<SaveStatus>();
    mocks.getStatus.mockReturnValueOnce(bootstrap.promise);
    const hook = renderHook(() => useProfileSaveNotifications());
    await flush();

    emit({ revision: 8, state: "saved" });
    await act(async () => bootstrap.resolve({ revision: 7, state: "error" }));

    expect(mocks.error).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
    hook.unmount();
  });

  it("releases a listener registered after unmount and ignores its retired callback", async () => {
    const subscription = deferred<() => void>();
    mocks.listen.mockImplementationOnce((_event: string, callback: SaveHandler) => {
      handler = callback;
      return subscription.promise;
    });
    const hook = renderHook(() => useProfileSaveNotifications());
    hook.unmount();
    await act(async () => subscription.resolve(unlisten));

    expect(unlisten).toHaveBeenCalledOnce();
    expect(mocks.getStatus).not.toHaveBeenCalled();
    emit({ revision: 9, state: "error" });
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("ignores pending bootstrap and retry results after unmount", async () => {
    const bootstrap = deferred<SaveStatus>();
    const retry = deferred<void>();
    mocks.getStatus.mockReturnValueOnce(bootstrap.promise);
    mocks.retry.mockReturnValueOnce(retry.promise);
    const hook = renderHook(() => useProfileSaveNotifications());
    await flush();
    emit({ revision: 1, state: "error" });
    await act(async () => { void clickRetry(); });
    hook.unmount();
    expect(unlisten).toHaveBeenCalledOnce();
    const notificationsAtUnmount = mocks.error.mock.calls.length;

    await act(async () => {
      bootstrap.resolve({ revision: 2, state: "error" });
      retry.reject(new Error("late write failure"));
    });
    emit({ revision: 3, state: "error" });

    expect(mocks.error).toHaveBeenCalledTimes(notificationsAtUnmount);
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.retry).toHaveBeenCalledOnce();
  });
});
