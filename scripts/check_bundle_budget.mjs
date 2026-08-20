import { readdirSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { gzipSync } from "node:zlib";

const distDir = join(process.cwd(), "dist");
const assetsDir = join(distDir, "assets");
const jsFiles = readdirSync(assetsDir).filter((file) => file.endsWith(".js"));

function listFiles(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    return entry.isDirectory() ? listFiles(path) : [{ path, bytes: statSync(path).size }];
  });
}

const metrics = {};
let totalGzipBytes = 0;
for (const file of jsFiles) {
  const gzipBytes = gzipSync(await readFile(join(assetsDir, file))).byteLength;
  totalGzipBytes += gzipBytes;
  const name = basename(file);
  if (name.startsWith("index-")) metrics.main_gzip_bytes = gzipBytes;
  if (name.startsWith("SettingsPage-")) metrics.settings_gzip_bytes = gzipBytes;
  if (name.startsWith("SubtitleOverlay-")) metrics.subtitle_gzip_bytes = gzipBytes;
  if (name.startsWith("SelectionOverlay-")) metrics.selection_gzip_bytes = gzipBytes;
}
metrics.total_js_gzip_bytes = totalGzipBytes;
metrics.core_js_gzip_bytes = totalGzipBytes - (metrics.selection_gzip_bytes ?? 0);
const distFiles = listFiles(distDir);
const fontFiles = distFiles.filter(({ path }) => /\.(?:woff2?|ttf|otf|ttc)$/i.test(path));
metrics.largest_font_bytes = Math.max(0, ...fontFiles.map(({ bytes }) => bytes));
metrics.total_font_bytes = fontFiles.reduce((total, { bytes }) => total + bytes, 0);
metrics.total_dist_bytes = distFiles.reduce((total, { bytes }) => total + bytes, 0);

// Guard user-visible entry points and meaningful aggregate regressions. The settings
// page is lazy-loaded and preloaded after startup, so its size remains observable in
// the metrics without an arbitrary per-page cap. Limits are rounded above the v1.5.8
// baseline to leave room for routine feature work while still catching material growth.
const budgets = {
  main_gzip_bytes: {
    limit: 140_000,
    rationale: "startup entry point",
  },
  subtitle_gzip_bytes: {
    limit: 10_000,
    rationale: "latency-sensitive subtitle window",
  },
  selection_gzip_bytes: {
    limit: 150_000,
    rationale: "latency-sensitive selection overlay",
  },
  core_js_gzip_bytes: {
    limit: 190_000,
    rationale: "all JavaScript except the selection overlay",
  },
  total_js_gzip_bytes: {
    limit: 340_000,
    rationale: "complete JavaScript payload",
  },
  largest_font_bytes: {
    limit: 80_000,
    rationale: "single font asset",
  },
  total_font_bytes: {
    limit: 1_200_000,
    rationale: "complete font payload",
  },
  total_dist_bytes: {
    limit: 2_500_000,
    rationale: "complete frontend distribution",
  },
};

console.log(`LIGHT_WHISPER_BUNDLE_METRICS ${JSON.stringify(metrics)}`);
const failures = Object.entries(budgets)
  .filter(([key, budget]) => typeof metrics[key] !== "number" || metrics[key] > budget.limit)
  .map(
    ([key, budget]) =>
      `${key}: ${metrics[key] ?? "missing"} > ${budget.limit} (${budget.rationale})`,
  );

if (failures.length > 0) {
  throw new Error(`Bundle budget exceeded:\n${failures.join("\n")}`);
}
