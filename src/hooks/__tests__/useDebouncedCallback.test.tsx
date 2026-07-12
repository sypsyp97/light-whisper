import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useDebouncedCallback } from "../useDebouncedCallback";

describe("useDebouncedCallback", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the returned controls stable across rerenders", () => {
    const { result, rerender } = renderHook(
      ({ value }) => useDebouncedCallback(() => {
        void value;
      }, 100),
      { initialProps: { value: "first" } },
    );

    const firstControls = result.current;
    rerender({ value: "second" });

    expect(result.current).toBe(firstControls);
  });

  it("uses the latest callback when a stable scheduled control fires", () => {
    vi.useFakeTimers();
    const seen: string[] = [];
    const { result, rerender } = renderHook(
      ({ value }) => useDebouncedCallback(() => {
        seen.push(value);
      }, 100),
      { initialProps: { value: "first" } },
    );

    const controls = result.current;
    rerender({ value: "second" });

    act(() => {
      controls.schedule();
      vi.advanceTimersByTime(100);
    });

    expect(seen).toEqual(["second"]);
  });

  it("flush resolves only after an async callback has finished", async () => {
    vi.useFakeTimers();
    let finishSave: (() => void) | undefined;
    let callbackFinished = false;
    const { result } = renderHook(() =>
      useDebouncedCallback(async () => {
        await new Promise<void>((resolve) => {
          finishSave = resolve;
        });
        callbackFinished = true;
      }, 100),
    );

    act(() => {
      result.current.schedule();
    });
    const flushed = result.current.flush();
    expect(flushed).toBeInstanceOf(Promise);
    await Promise.resolve();
    expect(callbackFinished).toBe(false);

    finishSave?.();
    await flushed;
    expect(callbackFinished).toBe(true);
  });

  it("flush waits for an async callback that the timer already started", async () => {
    vi.useFakeTimers();
    let finishSave: (() => void) | undefined;
    let callbackFinished = false;
    const callback = vi.fn(async () => {
      await new Promise<void>((resolve) => {
        finishSave = resolve;
      });
      callbackFinished = true;
    });
    const { result } = renderHook(() => useDebouncedCallback(callback, 100));

    act(() => {
      result.current.schedule();
      vi.advanceTimersByTime(100);
    });

    let flushFinished = false;
    const flushed = result.current.flush().then(() => {
      flushFinished = true;
    });
    await Promise.resolve();
    expect(flushFinished).toBe(false);
    expect(callbackFinished).toBe(false);

    finishSave?.();
    await flushed;

    expect(flushFinished).toBe(true);
    expect(callbackFinished).toBe(true);
    expect(callback).toHaveBeenCalledTimes(1);
  });

  it("serializes an older in-flight callback before a newer flushed value", async () => {
    vi.useFakeTimers();
    let finishFirst: (() => void) | undefined;
    const started: string[] = [];
    const finished: string[] = [];
    const callback = vi.fn(async (value: string) => {
      started.push(value);
      if (value === "old") {
        await new Promise<void>((resolve) => {
          finishFirst = resolve;
        });
      }
      finished.push(value);
    });
    const { result } = renderHook(() => useDebouncedCallback(callback, 100));

    act(() => {
      result.current.schedule("old");
      vi.advanceTimersByTime(100);
      result.current.schedule("new");
    });

    const flushed = result.current.flush();
    expect(started).toEqual(["old"]);
    expect(finished).toEqual([]);

    finishFirst?.();
    await flushed;

    expect(started).toEqual(["old", "new"]);
    expect(finished).toEqual(["old", "new"]);
  });
});
