import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

const repoRoot = path.resolve(__dirname, "../..");

function readRepoFile(relativePath: string): string {
  return readFileSync(path.join(repoRoot, relativePath), "utf8");
}

function readJsonVersion(relativePath: string): string {
  return JSON.parse(readRepoFile(relativePath)).version;
}

function readTomlString(relativePath: string, key: string): string {
  const content = readRepoFile(relativePath);
  const match = content.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m"));
  if (!match) {
    throw new Error(`${relativePath} is missing ${key}`);
  }
  return match[1];
}

function readPlistString(relativePath: string, key: string): string {
  const content = readRepoFile(relativePath);
  const match = content.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]+)</string>`));
  if (!match) {
    throw new Error(`${relativePath} is missing ${key}`);
  }
  return match[1];
}

describe("release metadata", () => {
  it("keeps project versions aligned across release manifests", () => {
    const versions = {
      "package.json": readJsonVersion("package.json"),
      "src-tauri/Cargo.toml": readTomlString("src-tauri/Cargo.toml", "version"),
      "src-tauri/tauri.conf.json": readJsonVersion("src-tauri/tauri.conf.json"),
      "native-macos/Bundle/Info.plist": readPlistString(
        "native-macos/Bundle/Info.plist",
        "CFBundleShortVersionString",
      ),
    };

    expect(new Set(Object.values(versions)).size, JSON.stringify(versions, null, 2)).toBe(1);
  });
});
