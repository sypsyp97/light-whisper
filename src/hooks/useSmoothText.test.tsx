import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useSmoothText } from "./useSmoothText";

afterEach(() => vi.restoreAllMocks());

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
