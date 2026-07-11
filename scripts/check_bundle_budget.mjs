import { readdirSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { basename, join } from "node:path";
import { gzipSync } from "node:zlib";

const assetsDir = join(process.cwd(), "dist", "assets");
const jsFiles = readdirSync(assetsDir).filter((file) => file.endsWith(".js"));

const metrics = {};
let totalGzipBytes = 0;
for (const file of jsFiles) {
  const gzipBytes = gzipSync(await readFile(join(assetsDir, file))).byteLength;
  totalGzipBytes += gzipBytes;
  const name = basename(file);
  if (name.startsWith("index-")) metrics.main_gzip_bytes = gzipBytes;
  if (name.startsWith("SettingsPage-")) metrics.settings_gzip_bytes = gzipBytes;
  if (name.startsWith("SubtitleOverlay-")) metrics.subtitle_gzip_bytes = gzipBytes;
}
metrics.total_js_gzip_bytes = totalGzipBytes;

const budgets = {
  main_gzip_bytes: 130_000,
  settings_gzip_bytes: 30_000,
  subtitle_gzip_bytes: 10_000,
  total_js_gzip_bytes: 175_000,
};

console.log(`LIGHT_WHISPER_BUNDLE_METRICS ${JSON.stringify(metrics)}`);
const failures = Object.entries(budgets)
  .filter(([key, budget]) => typeof metrics[key] !== "number" || metrics[key] > budget)
  .map(([key, budget]) => `${key}: ${metrics[key] ?? "missing"} > ${budget}`);

if (failures.length > 0) {
  throw new Error(`Bundle budget exceeded:\n${failures.join("\n")}`);
}
