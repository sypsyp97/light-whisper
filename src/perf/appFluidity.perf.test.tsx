import { performance } from "node:perf_hooks";
import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import TranscriptionHistory from "@/components/TranscriptionHistory";
import type { HistoryItem } from "@/types";
import "@/i18n";

afterEach(cleanup);

function historyItems(count: number, suffix = ""): HistoryItem[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `perf-${index}`,
    text: `第 ${index + 1} 条语音转写结果 ${suffix}`,
    originalText: `第 ${index + 1} 条语音转写结果`,
    timestamp: Date.now() + index,
    timeDisplay: "12:00",
  }));
}

test("representative history mount and update stay inside the fluidity budget", () => {
  const mountStarted = performance.now();
  const view = render(
    <TranscriptionHistory
      history={historyItems(200)}
      currentResult=""
      copiedId={null}
      onCopy={() => undefined}
    />,
  );
  const historyMountMs = performance.now() - mountStarted;

  const updateStarted = performance.now();
  view.rerender(
    <TranscriptionHistory
      history={historyItems(200, "updated")}
      currentResult=""
      copiedId="perf-20"
      onCopy={() => undefined}
    />,
  );
  const historyUpdateMs = performance.now() - updateStarted;
  const compositeMs = historyMountMs + historyUpdateMs;

  console.log(`LIGHT_WHISPER_FLUIDITY_METRICS ${JSON.stringify({
    history_mount_ms: Number(historyMountMs.toFixed(2)),
    history_update_ms: Number(historyUpdateMs.toFixed(2)),
    composite_ms: Number(compositeMs.toFixed(2)),
  })}`);

  expect(compositeMs).toBeLessThan(500);
});
