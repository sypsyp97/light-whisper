import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SPDX_ID = "GPL-3.0-only";

function read(path: string): string {
  return readFileSync(path, "utf8");
}

function readTomlString(path: string, key: string): string | undefined {
  const match = read(path).match(new RegExp(`^${key} = "([^"]+)"$`, "m"));
  return match?.[1];
}

describe("license metadata contract", () => {
  it("uses one software license identifier across project metadata", () => {
    const packageJson = JSON.parse(read("package.json"));
    const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"));

    expect(packageJson.license).toBe(SPDX_ID);
    expect(readTomlString("pyproject.toml", "license")).toBe(SPDX_ID);
    expect(readTomlString("src-tauri/Cargo.toml", "license")).toBe(SPDX_ID);
    expect(tauriConfig.bundle.license).toBe(SPDX_ID);
    expect(tauriConfig.bundle.licenseFile).toBe("../LICENSE");
  });

  it("keeps the README positioning and license badge consistent", () => {
    for (const path of ["README.md", "README.zh-CN.md"]) {
      const contents = read(path);
      expect(contents).toContain("GPL-3.0-only");
      expect(contents).toMatch(/open-source|开源/);
      expect(contents).not.toContain("PolyForm");
      expect(contents).not.toContain("CC BY-NC");
    }
  });

  it("preserves project and third-party notices in source and bundles", () => {
    const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"));
    const resources = tauriConfig.bundle.resources;
    const notice = read("NOTICE");
    const thirdPartyNotices = read("THIRD_PARTY_NOTICES.md");

    expect(read("LICENSE")).toContain("GNU GENERAL PUBLIC LICENSE");
    expect(read("LICENSE")).toContain("Version 3, 29 June 2007");
    expect(notice).toContain("GNU General Public License");
    expect(notice).toContain("THIRD_PARTY_NOTICES.md");
    expect(thirdPartyNotices).toContain("FireRedVAD ONNX model and CMVN data");
    expect(thirdPartyNotices).not.toContain("implementation and model assets");
    expect(resources["../NOTICE"]).toBe("NOTICE.txt");
    expect(resources["../THIRD_PARTY_NOTICES.md"]).toBe(
      "THIRD_PARTY_NOTICES.md",
    );
  });
});
