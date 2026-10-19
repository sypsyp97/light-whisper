import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const tauriDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../src-tauri",
);

describe("Windows app icon contract", () => {
  it("provides a full-resolution first image and retains desktop DPI sizes", () => {
    const config = JSON.parse(
      readFileSync(resolve(tauriDirectory, "tauri.conf.json"), "utf8"),
    ) as { bundle: { icon: string[] } };
    const iconPath = config.bundle.icon.find((path) => path.endsWith(".ico"));
    expect(iconPath).toBeDefined();
    const icon = readFileSync(resolve(tauriDirectory, iconPath!));

    expect(icon.length).toBeGreaterThanOrEqual(6);
    expect([icon.readUInt16LE(0), icon.readUInt16LE(2)]).toEqual([0, 1]);
    const count = icon.readUInt16LE(4);
    expect(count).toBeGreaterThan(0);
    const directoryEnd = 6 + count * 16;
    expect(directoryEnd).toBeLessThanOrEqual(icon.length);

    const entries = Array.from({ length: count }, (_, index) => {
      const offset = 6 + index * 16;
      const width = icon[offset] || 256;
      const height = icon[offset + 1] || 256;
      const length = icon.readUInt32LE(offset + 8);
      const start = icon.readUInt32LE(offset + 12);
      expect(icon[offset + 3]).toBe(0);
      expect(length).toBeGreaterThan(0);
      expect(start).toBeGreaterThanOrEqual(directoryEnd);
      expect(start + length).toBeLessThanOrEqual(icon.length);

      const payload = icon.subarray(start, start + length);
      if (payload.subarray(0, 8).equals(Buffer.from("89504e470d0a1a0a", "hex"))) {
        expect(payload.length).toBeGreaterThanOrEqual(33);
        expect(payload.readUInt32BE(8)).toBe(13);
        expect(payload.toString("ascii", 12, 16)).toBe("IHDR");
        expect([payload.readUInt32BE(16), payload.readUInt32BE(20)]).toEqual([
          width,
          height,
        ]);
      }
      return { width, height };
    });

    expect(entries).toEqual(
      expect.arrayContaining(
        [16, 20, 24, 32, 48, 64, 256].map((size) => ({
          width: size,
          height: size,
        })),
      ),
    );
    // Tauri decodes the first ICO entry into the default window/taskbar icon.
    expect(entries[0].width, "taskbar source image width").toBeGreaterThanOrEqual(
      128,
    );
    expect(entries[0].height, "taskbar source image height").toBeGreaterThanOrEqual(
      128,
    );
    expect(entries[0].width).toBe(Math.max(...entries.map((entry) => entry.width)));
    expect(entries[0].height).toBe(Math.max(...entries.map((entry) => entry.height)));
  });
});
