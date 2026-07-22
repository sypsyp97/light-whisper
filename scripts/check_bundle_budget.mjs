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

const budgets = {
  main_gzip_bytes: 130_000,
  settings_gzip_bytes: 30_000,
  subtitle_gzip_bytes: 10_000,
  selection_gzip_bytes: 145_000,
  core_js_gzip_bytes: 175_000,
  total_js_gzip_bytes: 310_000,
  largest_font_bytes: 14_500_000,
  total_font_bytes: 15_500_000,
  total_dist_bytes: 17_000_000,
};

console.log(`LIGHT_WHISPER_BUNDLE_METRICS ${JSON.stringify(metrics)}`);
const failures = Object.entries(budgets)
  .filter(([key, budget]) => typeof metrics[key] !== "number" || metrics[key] > budget)
  .map(([key, budget]) => `${key}: ${metrics[key] ?? "missing"} > ${budget}`);

if (failures.length > 0) {
  throw new Error(`Bundle budget exceeded:\n${failures.join("\n")}`);
}
