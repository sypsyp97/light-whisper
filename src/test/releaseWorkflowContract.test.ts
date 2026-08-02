import {
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { describe, expect, it } from "vitest";

function read(path: string): string {
  return readFileSync(path, "utf8");
}

function expectCallsInOrder(body: string, calls: string[]): void {
  let previousIndex = -1;
  for (const call of calls) {
    const index = body.indexOf(call);
    expect(index, `${call} should be present`).toBeGreaterThan(previousIndex);
    previousIndex = index;
  }
}

describe("release workflow contract", () => {
  it("runs local gates and packaging before the candidate commit, then waits for its CI before release", () => {
    const script = read("scripts/release.sh");
    const main = script.match(/main\(\) \{([\s\S]*?)\n\}\n\nmain "\$@"/);

    expect(main).not.toBeNull();
    expectCallsInOrder(main![1], [
      "preflight",
      "update_versions",
      "run_local_checks",
      "prepare_engine",
      "verify_engine_archive",
      "build_installer",
      "verify_installer",
      "commit_candidate",
      'candidate_sha="$(git rev-parse HEAD)"',
      'wait_for_ci "$candidate_sha"',
      'publish_release "$candidate_sha"',
    ]);
  });

  it("keeps the full frontend, Python, Rust and diff checks as hard local gates", () => {
    const script = read("scripts/release.sh");

    for (const required of [
      "pnpm check",
      "pnpm audit --prod --audit-level high",
      "uv lock --check",
      "python -m unittest discover",
      "scripts/test_build_engine_atomicity.py",
      "cargo fmt",
      "cargo clippy",
      "cargo test",
      "git diff --check",
      "find_seven_zip >/dev/null",
      "git diff --name-only -z HEAD",
      "git ls-files --others --exclude-standard -z",
      "git diff --cached --quiet",
    ]) {
      expect(script).toContain(required);
    }
  });

  it("binds CI approval to the exact candidate SHA and all required jobs", () => {
    const script = read("scripts/release.sh");
    const main = script.match(/main\(\) \{([\s\S]*?)\n\}\n\nmain "\$@"/);

    expect(script).toContain("set -euo pipefail");
    expect(script).toContain('--commit "$candidate_sha"');
    expect(script).toContain('gh run watch "$run_id" --exit-status');
    expect(script).toContain('run_head" == "$candidate_sha"');
    expect(script).toContain("for required_job in Frontend Python Rust");
    expect(script).toContain('git rev-parse "origin/$RELEASE_BRANCH"');
    expect(script).toContain("git ls-files --others --exclude-standard");
    expect(main).not.toBeNull();
    expect(main![1].indexOf('wait_for_ci "$candidate_sha"')).toBeLessThan(
      main![1].indexOf('publish_release "$candidate_sha"'),
    );
    expect(script).toMatch(
      /publish_release\(\)[\s\S]*git tag -a "\$TAG"[\s\S]*gh release create/,
    );
  });

  it("does not package the application in ordinary CI", () => {
    const workflow = read(".github/workflows/ci.yml");

    expect(workflow).not.toContain("pnpm tauri build");
    expect(workflow).not.toContain("gh release create");
  });

  it("makes Tauri distribution builds verify the engine archive first", () => {
    const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"));
    const packageJson = JSON.parse(read("package.json"));

    expect(tauriConfig.build.beforeBuildCommand).toBe(
      "pnpm build:distribution",
    );
    expect(packageJson.scripts["build:distribution"]).toContain(
      "verify_engine_archive.mjs",
    );
  });

  it("rejects missing, empty and non-XZ-header engine archives before packaging", () => {
    const directory = mkdtempSync(join(tmpdir(), "light-whisper-engine-check-"));
    const archive = join(directory, "engine.tar.xz");
    const verify = () =>
      spawnSync(
        process.execPath,
        ["scripts/verify_engine_archive.mjs", archive],
        { encoding: "utf8" },
      );

    try {
      expect(verify().status).not.toBe(0);

      writeFileSync(archive, Buffer.alloc(0));
      expect(verify().status).not.toBe(0);

      writeFileSync(archive, "not an xz archive");
      expect(verify().status).not.toBe(0);

      writeFileSync(
        archive,
        Buffer.from([0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]),
      );
      expect(verify().status).toBe(0);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
