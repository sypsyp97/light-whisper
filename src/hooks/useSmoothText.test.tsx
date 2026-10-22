import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useSmoothText } from "./useSmoothText";

afterEach(() => vi.restoreAllMocks());

const segmenterPrototype = (Intl as typeof Intl & {
  Segmenter: { prototype: { segment(text: string): unknown } };
}).Segmenter.prototype;

function controlledFrames() {
  let nextId = 0;
  const pending = new Map<number, FrameRequestCallback>();
  const request = vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    pending.set(++nextId, callback);
    return nextId;
  });
  const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
    pending.delete(id);
  });

  return {
    request,
    cancel,
    pending,
    step(time: number) {
      const callbacks = [...pending.values()];
      pending.clear();
      act(() => callbacks.forEach((callback) => callback(time)));
    },
  };
}

it("does not segment unchanged text again on parent rerenders", () => {
  const frames = controlledFrames();
  const segment = vi.spyOn(segmenterPrototype, "segment");
  const text = "先👩‍💻e\u0301中";
  const { result, rerender } = renderHook(
    ({ source }) => useSmoothText(source, { charsPerFrame: 1, maxCatchup: 1 }),
    { initialProps: { source: text } },
  );
  expect(result.current).toBe(text);
  expect(frames.request).not.toHaveBeenCalled();

  segment.mockClear();
  for (let index = 0; index < 5; index += 1) rerender({ source: text });

  expect(result.current).toBe(text);
  expect(frames.request).not.toHaveBeenCalled();
  expect(segment).not.toHaveBeenCalled();
});

it("segments each new source once and reveals whole graphemes without segmenting frames", () => {
  const frames = controlledFrames();
  const segment = vi.spyOn(segmenterPrototype, "segment");
  const { result, rerender } = renderHook(
    ({ source }) => useSmoothText(source, { charsPerFrame: 1, maxCatchup: 1 }),
    { initialProps: { source: "先" } },
  );
  expect(result.current).toBe("先");

  segment.mockClear();
  rerender({ source: "先👩‍💻e\u0301中" });
  const sourceCalls = segment.mock.calls.length;
  segment.mockClear();
  expect(result.current).toBe("先");
  expect(frames.pending.size).toBe(1);

  frames.step(0);
  expect(result.current).toBe("先");
  frames.step(17);
  expect(result.current).toBe("先👩‍💻");
  frames.step(34);
  expect(result.current).toBe("先👩‍💻e\u0301");
  frames.step(51);
  expect(result.current).toBe("先👩‍💻e\u0301中");
  frames.step(68);
  expect(frames.pending.size).toBe(0);
  expect({ sourceCalls, frameCalls: segment.mock.calls.length }).toEqual({
    sourceCalls: 1,
    frameCalls: 0,
  });
});

it("cancels pending frames on divergence, clear, and unmount", () => {
  const frames = controlledFrames();
  const { result, rerender, unmount } = renderHook(
    ({ source }) => useSmoothText(source, { charsPerFrame: 1, maxCatchup: 1 }),
    { initialProps: { source: "先" } },
  );

  rerender({ source: "先继续" });
  const divergentFrame = [...frames.pending.keys()][0];
  rerender({ source: "另👩‍💻e\u0301" });
  expect(result.current).toBe("另👩‍💻e\u0301");
  expect(frames.cancel).toHaveBeenCalledWith(divergentFrame);
  expect(frames.pending.size).toBe(0);

  rerender({ source: "另👩‍💻e\u0301继续" });
  const clearedFrame = [...frames.pending.keys()][0];
  rerender({ source: "" });
  expect(result.current).toBe("");
  expect(frames.cancel).toHaveBeenCalledWith(clearedFrame);
  expect(frames.pending.size).toBe(0);

  rerender({ source: "重来" });
  expect(result.current).toBe("");
  expect(frames.pending.size).toBe(1);
  const finalFrame = [...frames.pending.keys()][0];
  unmount();
  expect(frames.cancel).toHaveBeenCalledWith(finalFrame);
  expect(frames.pending.size).toBe(0);
});

it("bypasses unused native animation and preserves text when final output enables it", () => {
  const raf = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
  const { result, rerender } = renderHook(
    ({ text, enabled }) => useSmoothText(text, { enabled }),
    { initialProps: { text: "", enabled: false } },
  );
  rerender({ text: "我们明天去上海", enabled: false });
  expect(result.current).toBe("我们明天去上海");
  rerender({ text: "我们明天去上班", enabled: false });
  expect(result.current).toBe("我们明天去上班");
  expect(raf).not.toHaveBeenCalled();
  rerender({ text: "我们明天去上班", enabled: true });
  expect(result.current).toBe("我们明天去上班");
  expect(raf).not.toHaveBeenCalled();
  rerender({ text: "我们明天去上班。", enabled: true });
  expect(raf).toHaveBeenCalledOnce();
});

it("cancels a pending reveal when native caption rendering takes over", () => {
  const cancel = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  vi.spyOn(window, "requestAnimationFrame").mockReturnValue(7);
  const { result, rerender, unmount } = renderHook(
    ({ text, enabled }) => useSmoothText(text, { enabled }),
    { initialProps: { text: "", enabled: true } },
  );
  rerender({ text: "Streaming", enabled: true });
  act(() => rerender({ text: "Native caption", enabled: false }));
  expect(cancel).toHaveBeenCalledWith(7);
  expect(result.current).toBe("Native caption");
  unmount();
});
