# Releases

Build installers locally. GitHub Actions checks source code and can test an uploaded candidate installer. It does not build or upload installers.
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
bash scripts/release.sh 1.8.1 "Concise English release notes." --rebuild-engine
```

The script updates all six version files and runs the local checks.
It builds the engine, standalone CLI and installer, then validates the artifacts.
It prints the installer SHA-256.
It then pushes a candidate commit and waits for that exact commit's `Frontend`, `Python`, `Rust`, `Application formal contracts`, `Verus implementation proofs` and `CLI contracts` CI jobs.
Every job must exist and succeed; skipped jobs block publication.
After these checks pass and `origin/main` still matches, it publishes the annotated tag and GitHub Release.

Use `--reuse-engine` only when the packaged Python code, native libraries and dependencies are unchanged and the existing archive has been verified. Release builds reject missing or invalid engine archives. Development/debug builds can use a placeholder.
Both engine modes rebuild the CLI from the current checkout.
The CLI checks include its packaged executable and an isolated native host with no application windows. The host tests use a synthetic engine to verify the real application protocol and lifecycle. They do not run model inference.

For a manual release, follow the same gates: [local checks](development.md#checks), verified engine and installer, candidate commit, matching remote CI, then tag and Release. For multiline notes, use `gh release create --notes-file <file> --verify-tag`. Do not publish a tag for a failed candidate.

## Validate installer changes

Use the manual release path when command registration or installer hooks change. The release script does not run the following native installer check.

1. Build the installer and review the source, commit metadata and packaged files for private data. Record the installer SHA-256.
2. Push the reviewed candidate to `main`. Wait for all six CI jobs listed above to succeed for that exact commit.
3. Create a draft Release with the candidate commit as its target. Upload the reviewed installer to the draft. Keep the Release unpublished while testing.
4. Run the `Installed command smoke` workflow from `main`. Supply the uploaded `asset_id`, installer `sha256`, full `candidate_sha` and `expected_version`. The workflow checks out that commit and verifies the downloaded bytes before installation.
5. Require a successful workflow and summary for the same candidate and hash. Then push the annotated version tag and publish the draft. Check that the public asset has the recorded hash.

The workflow uses a fresh GitHub-hosted Windows VM. It checks new installation, repeated installation and silent uninstall. It verifies plain `light-whisper` commands in PowerShell 5, PowerShell 7 and Command Prompt. It also checks that unrelated PATH entries and synthetic user-data files remain intact. It does not load models or test the interactive cancellation prompt.

Only the summary JSON is uploaded as evidence. Raw diagnostics stay in the disposable runner. Do not upload local settings, recordings, logs, credentials or private filesystem paths with a Release.

The workflow downloads the unpublished asset in a separate job with the permission needed to read draft assets. That job does not check out source or run the installer. It passes only the verified installer through a temporary workflow artifact. The Windows job retains read-only repository permissions and checks the installer hash again before execution.

## Recovery and edits

- Before tagging: fix the failure and validate a new candidate.
- If the tag exists but Release creation failed: verify its commit before retrying `gh release create --verify-tag`. Do not move a published tag.
- Title and release-note corrections can use `gh release edit <tag> --notes-file <file>` without changing tags or assets. Preserve the facts of the original release.
- Asset replacement is a separate release operation requiring explicit authorization; immutable releases prohibit it.

Building or publishing an installer does not update the application currently installed on the development machine.
