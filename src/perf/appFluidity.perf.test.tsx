import { performance } from "node:perf_hooks";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, render, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

import TranscriptionHistory from "@/components/TranscriptionHistory";
import { segmentGraphemes, useSmoothText } from "@/hooks/useSmoothText";
import type { HistoryItem } from "@/types";
import "@/i18n";

const FRAME_BUDGET_MS = 1000 / 60;
const HISTORY_ITEM_COUNT = 20;
const WARMUP_RUNS = 3;

afterEach(cleanup);

function round(value: number): number {
  return Number(value.toFixed(3));
}

function median(values: number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.floor(sorted.length / 2)];
}

function percentile(values: number[], ratio: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * ratio))];
}

function historyItems(count: number, suffix = ""): HistoryItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `perf-${index}`,
    text: `第 ${index + 1} 条语音转写结果 ${suffix}`,
    originalText: `第 ${index + 1} 条语音转写结果`,
    timestamp: 1_700_000_000_000 + index,
    timeDisplay: "12:00",
  }));
}

function cssRule(source: string, selector: string): string {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return source.match(new RegExp(`${escapedSelector}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
}

function assertCssMotionContracts(): void {
  const themeCss = readFileSync(resolve("src/styles/theme.css"), "utf8");
  const pagesCss = readFileSync(resolve("src/styles/pages.css"), "utf8");
  const selectionCss = readFileSync(resolve("src/styles/selection.css"), "utf8");
  const subtitleCss = readFileSync(resolve("src/styles/subtitle.css"), "utf8");
  const historyRule = cssRule(pagesCss, ".history-item");

  expect(themeCss).toContain("@keyframes fade-in");
  expect(themeCss).toMatch(/@keyframes fade-in\s*\{[\s\S]*?opacity:[\s\S]*?transform:/);
  expect.soft(historyRule).not.toMatch(/animation(?:-delay)?:/);
  expect(historyRule).toMatch(/transition:/);

  const pageCss = themeCss + "\n" + pagesCss;
  expect.soft(pageCss).toMatch(/--dur-2:\s*(?:140ms|0\.14s)\s*;/);
  for (const direction of ["left", "right"]) {
    expect.soft(cssRule(pageCss, `.page-enter-${direction}`))
      .toMatch(/animation:\s*[a-z-]+\s+(?:var\(--dur-2\)|140ms|0\.14s)/);
  }
  expect.soft(pageCss).not.toMatch(/\.page-exit-(?:left|right)\b/);
  expect(pageCss).toMatch(/@keyframes page-enter-from-right\s*\{[\s\S]*?opacity:[\s\S]*?translate/);

  const ringRule = cssRule(pagesCss, ".recording-pulse-ring");
  expect.soft(ringRule).toMatch(/animation:\s*[a-z-]+\s+(?:[1-9][0-9]{1,2}ms|0\.\d+s|var\(--dur-[1-4]\))/);
  expect.soft(ringRule).not.toMatch(/infinite/);
  expect.soft(pagesCss).not.toContain(".recording-pulse-ring-outer");

  const waveformBarRule = cssRule(subtitleCss, ".subtitle-waveform-indicator-bar");
  expect.soft(waveformBarRule).toMatch(/(?:^|[;{\n])\s*height:\s*16px;/);
  expect.soft(waveformBarRule).toMatch(/transition:\s*transform\b/);
  expect.soft(subtitleCss).not.toMatch(/\.stream-char\b/);
  expect(cssRule(selectionCss, ".selection-loading svg"))
    .toMatch(/animation:\s*selection-spin\s+900ms/);
}

function installRafController() {
  type RafCallback = (time: number) => void;
  const originalRequest = globalThis.requestAnimationFrame;
  const originalCancel = globalThis.cancelAnimationFrame;
  let nextId = 1;
  const callbacks = new Map<number, RafCallback>();

  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    writable: true,
    value: (callback: RafCallback) => {
      const id = nextId;
      nextId += 1;
      callbacks.set(id, callback);
      return id;
    },
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    writable: true,
    value: (id: number) => callbacks.delete(id),
  });

  return {
    frame(time: number): number {
      const pending = [...callbacks.values()];
      callbacks.clear();
      for (const callback of pending) callback(time);
      return pending.length;
    },
    restore(): void {
      Object.defineProperty(globalThis, "requestAnimationFrame", {
        configurable: true,
        writable: true,
        value: originalRequest,
      });
      Object.defineProperty(globalThis, "cancelAnimationFrame", {
        configurable: true,
        writable: true,
        value: originalCancel,
      });
    },
  };
}

function installRegularMotionPreference(): () => void {
  const originalMatchMedia = window.matchMedia;
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockReturnValue({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  });
  return () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: originalMatchMedia,
    });
  };
}

function measureHistoryRendering() {
  const mountDurations: number[] = [];
  const updateDurations: number[] = [];
  let motionContractChecked = false;

  for (let run = 0; run < 13; run += 1) {
    const mountStarted = performance.now();
    const view = render(
      <TranscriptionHistory
        history={historyItems(HISTORY_ITEM_COUNT)}
        currentResult=""
        copiedId={null}
        onCopy={() => undefined}
      />,
    );
    const mountDuration = performance.now() - mountStarted;

    if (!motionContractChecked) {
      const rows = [...view.container.querySelectorAll<HTMLElement>(".history-item")];
      expect(rows).toHaveLength(HISTORY_ITEM_COUNT);
      expect.soft(rows.every((row) => row.style.animationDelay === "")).toBe(true);
      motionContractChecked = true;
    }

    const updateStarted = performance.now();
    view.rerender(
      <TranscriptionHistory
        history={historyItems(HISTORY_ITEM_COUNT, "updated")}
        currentResult=""
        copiedId="perf-10"
        onCopy={() => undefined}
      />,
    );
    const updateDuration = performance.now() - updateStarted;
    view.unmount();

    if (run >= WARMUP_RUNS) {
      mountDurations.push(mountDuration);
      updateDurations.push(updateDuration);
    }
  }

  return {
    history_mount_median_ms: round(median(mountDurations)),
    history_update_median_ms: round(median(updateDurations)),
  };
}

async function measureSmoothTextMotion() {
  const source = (
    "流式字幕应该逐帧稳定出现，emoji 👩‍💻 和 mixed scripts 也不能跳变。 "
  ).repeat(3);
  const updateDurations: number[] = [];
  const totalFrameDurations: number[] = [];
  const allFrameDurations: number[] = [];
  const frameCounts: number[] = [];
  const intermediateStateCounts: number[] = [];

  for (let run = 0; run < 9; run += 1) {
    const raf = installRafController();
    try {
      const { result, rerender, unmount } = renderHook(
        ({ text }) => useSmoothText(text),
        { initialProps: { text: "" } },
      );

      const updateStarted = performance.now();
      await act(async () => rerender({ text: source }));
      const updateDuration = performance.now() - updateStarted;

      let frames = 0;
      let previous = result.current;
      const intermediateStates = new Set<string>();
      const frameDurations: number[] = [];
      while (result.current !== source && frames < 120) {
        const frameStarted = performance.now();
        await act(async () => {
          raf.frame(frames * FRAME_BUDGET_MS);
        });
        frameDurations.push(performance.now() - frameStarted);
        frames += 1;
        if (result.current !== previous && result.current !== source) {
          intermediateStates.add(result.current);
        }
        previous = result.current;
      }

      expect(result.current).toBe(source);
      expect(frames).toBeGreaterThanOrEqual(8);
      expect(frames).toBeLessThan(120);
      expect(intermediateStates.size).toBeGreaterThanOrEqual(6);
      unmount();

      if (run >= WARMUP_RUNS) {
        updateDurations.push(updateDuration);
        totalFrameDurations.push(frameDurations.reduce((total, value) => total + value, 0));
        allFrameDurations.push(...frameDurations);
        frameCounts.push(frames);
        intermediateStateCounts.push(intermediateStates.size);
      }
    } finally {
      raf.restore();
    }
  }

  return {
    smooth_text_source_update_median_ms: round(median(updateDurations)),
    smooth_text_frame_cpu_total_median_ms: round(median(totalFrameDurations)),
    smooth_text_frame_cpu_p95_ms: round(percentile(allFrameDurations, 0.95)),
    smooth_text_frames: median(frameCounts),
    smooth_text_intermediate_states: median(intermediateStateCounts),
  };
}

function measureGraphemeSegmentation() {
  const corpus = (
    "中文语音转文字需要稳定处理标点、emoji 👩‍💻🚀、Latin words, and mixed scripts. "
  ).repeat(12);
  const durations: number[] = [];
  let checksum = 0;

  for (let run = 0; run < 18; run += 1) {
    const startedAt = performance.now();
    for (let iteration = 0; iteration < 300; iteration += 1) {
      checksum += segmentGraphemes(`${corpus}${iteration}`).length;
    }
    if (run >= WARMUP_RUNS) durations.push(performance.now() - startedAt);
  }

  expect(checksum).toBeGreaterThan(0);
  return {
    grapheme_segment_batch_median_ms: round(median(durations)),
  };
}

test("representative work stays fast without removing motion", async () => {
  const restoreMotionPreference = installRegularMotionPreference();
  try {
    assertCssMotionContracts();
    const history = measureHistoryRendering();
    const smoothText = await measureSmoothTextMotion();
    const segmentation = measureGraphemeSegmentation();
    const compositeMs = round(
      history.history_mount_median_ms
        + history.history_update_median_ms
        + smoothText.smooth_text_source_update_median_ms
        + smoothText.smooth_text_frame_cpu_total_median_ms
        + segmentation.grapheme_segment_batch_median_ms,
    );

    console.log(`LIGHT_WHISPER_FLUIDITY_METRICS ${JSON.stringify({
      lower_is_better: [
        "history_mount_median_ms",
        "history_update_median_ms",
        "smooth_text_source_update_median_ms",
        "smooth_text_frame_cpu_total_median_ms",
        "smooth_text_frame_cpu_p95_ms",
        "grapheme_segment_batch_median_ms",
        "composite_ms",
      ],
      motion_fidelity_pass: true,
      ...history,
      ...smoothText,
      ...segmentation,
      composite_ms: compositeMs,
    })}`);

    expect(history.history_mount_median_ms + history.history_update_median_ms).toBeLessThan(100);
    expect(smoothText.smooth_text_frame_cpu_p95_ms).toBeLessThan(FRAME_BUDGET_MS);
    expect(segmentation.grapheme_segment_batch_median_ms).toBeLessThan(180);
    expect(compositeMs).toBeLessThan(300);
  } finally {
    restoreMotionPreference();
  }
});
