# Releases

Build installers locally. GitHub Actions checks source code; it does not build or upload installers.
Write documentation and release notes in concise English. Keep only `README.zh-CN.md` as a Chinese companion.

## Prepare

- Commit reviewed changes to `main` and synchronize it with `origin/main`.
- Keep the working tree clean. Confirm the target version and tag do not exist.
- Install the [development tools](development.md), GitHub CLI, Git Bash and 7-Zip.
- Authenticate with `gh auth status`.
- If the R2T2 source, patch or native dependencies changed, rebuild both native packages first; see [R2T2](r2t2-native.md).

## Publish

From the repository root:

```bash
bash scripts/release.sh 1.6.0 "Concise English release notes." --rebuild-engine
```

The script updates all six version files, runs the local CI checks, builds the engine and installer, validates both archives, and prints the installer SHA-256. It then pushes a candidate commit and waits for that exact commit's `Frontend`, `Python` and `Rust` CI jobs. Only after all succeed and `origin/main` still matches does it publish the annotated tag and GitHub Release.

Use `--reuse-engine` only when the packaged Python code, native libraries and dependencies are unchanged and the existing archive has been verified. Release builds reject missing or invalid engine archives. Development/debug builds can use a placeholder.

For a manual release, follow the same gates: [local checks](development.md#checks), verified engine and installer, candidate commit, matching remote CI, then tag and Release. For multiline notes, use `gh release create --notes-file <file> --verify-tag`. Do not publish a tag for a failed candidate.

## Recovery and edits

- Before tagging: fix the failure and validate a new candidate.
- If the tag exists but Release creation failed: verify its commit before retrying `gh release create --verify-tag`. Do not move a published tag.
- Title and release-note corrections can use `gh release edit <tag> --notes-file <file>` without changing tags or assets. Preserve the facts of the original release.
- Asset replacement is a separate release operation requiring explicit authorization; immutable releases prohibit it.

Building or publishing an installer does not update the application currently installed on the development machine.
