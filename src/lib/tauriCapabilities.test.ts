import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface Capability {
  identifier: string;
  windows: string[];
  permissions: string[];
}

interface TauriConfig {
  app: {
    security: {
      capabilities: string[];
    };
  };
}

function readJson<T>(relativePath: string): T {
  return JSON.parse(readFileSync(resolve(relativePath), "utf8")) as T;
}

describe("Tauri window capability boundaries", () => {
  it("keeps the subtitle window event-only", () => {
    const subtitle = readJson<Capability>(
      "src-tauri/capabilities/subtitle.json",
    );

    expect(subtitle.identifier).toBe("subtitle");
    expect(subtitle.windows).toEqual(["subtitle"]);
    expect(subtitle.permissions).toEqual(["core:event:default"]);
  });

  it("grants the main window every autostart operation used by settings", () => {
    const main = readJson<Capability>("src-tauri/capabilities/main.json");

    expect(main.identifier).toBe("main");
    expect(main.windows).toEqual(["main"]);
    expect(main.permissions).toEqual(
      expect.arrayContaining([
        "autostart:allow-enable",
        "autostart:allow-disable",
        "autostart:allow-is-enabled",
      ]),
    );
  });

  it("keeps the selection toolbar isolated while allowing its explicit window-hide fallback", () => {
    const selection = readJson<Capability>(
      "src-tauri/capabilities/selection.json",
    );

    expect(selection.identifier).toBe("selection");
    expect(selection.windows).toEqual(["selection-toolbar"]);
    expect(selection.permissions).toEqual(
      expect.arrayContaining([
        "core:window:default",
        "core:window:allow-hide",
      ]),
    );
    expect(selection.permissions).not.toEqual(
      expect.arrayContaining([
        "clipboard-manager:default",
        "keyring:default",
      ]),
    );
  });

  it("loads every window capability from the desktop configuration", () => {
    const config = readJson<TauriConfig>("src-tauri/tauri.conf.json");

    expect(config.app.security.capabilities).toEqual([
      "main",
      "subtitle",
      "selection",
    ]);
  });
});
