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
});
