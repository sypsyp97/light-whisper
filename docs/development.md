# Development

Build on Windows 10/11 x64:

| Tool | Version | Purpose |
|:--|:--|:--|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2022 (MSVC 14.44 validated) | Rust and native ASR/VAD C++ toolchain |
| [Rust](https://www.rust-lang.org/tools/install) | 1.93.0 (CI) | Engine, CLI and Tauri desktop |
| [Node.js](https://nodejs.org/) | 22.14.0 (CI) | Frontend build |
| [pnpm](https://pnpm.io/) | 10.28.2 (CI) | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | 0.11.30 (CI) | Build-time tooling, locked native SDK wheels and reference tests |

The development tools support Python 3.11–3.12. `uv` selects 3.11 from `.python-version`.
The build scripts invoke Cargo and select native libraries. The tests also use
Python implementations of the engine and CLI as references. The installed
application runs the Rust executables without a Python interpreter.

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install --frozen-lockfile
uv sync --frozen
pnpm tauri dev
```

### Enable Git privacy checks

In each clone, put Python 3 on PATH and enable the versioned Git hooks:

```bash
git config --local core.hooksPath .githooks
git config --local user.name "YOUR_PUBLIC_GITHUB_USERNAME"
git config --local user.email "YOUR_GITHUB_NOREPLY_ADDRESS"
```

Copy your noreply address from GitHub email settings. The commit hook checks the
staged filenames and contents plus the active author and committer identities.
To flag an address anywhere in outgoing files, run
`git config --local --add privacy.privateEmail "YOUR_PRIVATE_ADDRESS"` once for
each private address. These entries stay in the clone's local configuration.
Before Git creates or replaces a local tag, the reference-transaction hook
checks its name and any annotated tag objects it points to. It rejects non-noreply
emails in tagger identities and notes, profile paths and recognized credentials.
Lightweight tags have no tagger or annotation. Existing tags keep their targets.
The push hook reads the destination's advertised refs and checks new Git objects,
including intermediate commits, messages, filenames, annotated tags and destination
ref names. The hooks check for local profile paths and common credential patterns.
Diagnostics give the category without printing matched values. The push hook runs
after Git connects to the destination, so configure a credential-free remote URL.

Before uploading an installer, run `python scripts/check_release_privacy.py <installer>`.
The Git hooks inspect Git objects and cannot see inside compressed installer payloads.

For Qwen 0.6B and R2T2 development, first build the shared native runtimes with the
commands below. Qwen and R2T2 support CPU and NVIDIA CUDA.
The CPU and CUDA runtimes live in `src-tauri/resources/r2t2-native/`.
The Windows distribution contains a Rust engine and a standalone Rust CLI.

Build the runtime packages and installer from the repository root:

```powershell
uv run --locked python scripts/build_r2t2_runtime.py --backend cpu
uv run --locked python scripts/build_r2t2_runtime.py --backend cuda --cuda-root "C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.9"
uv run --locked python scripts/build_engine.py
uv run --locked python scripts/build_cli.py
pnpm tauri build
```

The build scripts compile the `rust-runtime` workspace in release mode with
the locked Cargo dependencies. The engine build obtains the Microsoft.ML.OnnxRuntime
1.24.1 Windows x64 SDK from NuGet and checks its package SHA-256
(`5fca4421974b450711520f17a27e527fec60a78c2b6f9db3b05c159d9b5a346d`).
The SDK cache is `build/native-sdk/onnxruntime-1.24.1`. For an offline build,
set `LW_ORT_SDK` to an extracted copy of that package. The build reads the
kaldi-native-fbank 1.22.3 SDK and libsndfile from the locked Windows wheels.
`LW_KALDI_SDK` can point to a matching directory containing `include/` and
`lib/`. `LW_NATIVE_RESOURCES` can point to an external resources directory
containing both validated `r2t2-native` backend builds. `CARGO_TARGET_DIR`
controls the Cargo artifact cache.

The engine archive contains `engine.exe`, its native libraries and notices,
FireRedVAD assets, and backend file manifests. Its distribution manifest
records the size and SHA-256 of every payload file. Verification also checks
the executable's AMD64 PE header, release source fingerprint and image hash.
The CLI carries its own executable and source hashes. The build publishes each
artifact by atomic replacement after verification. To check existing artifacts:

```powershell
uv run --locked python scripts/build_engine.py --verify-archive src-tauri/resources/engine.tar.xz
uv run --locked python scripts/build_cli.py --verify
```

7-Zip compresses the engine tar with XZ and x86 BCJ filtering when available.
The NSIS installer is written to `src-tauri/target/release/bundle/nsis/`.
Installation registers `light-whisper` on the current user's PATH. Open a new
terminal after installation to use the command.

To prefetch a model from a source checkout, run the reference downloader:

```bash
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-0.6b
uv run python src-tauri/resources/download_models.py --engine confucius4-r2t2
```

For China mainland downloads, set `HF_ENDPOINT=https://hf-mirror.com` before prefetching.

## Agent debugging CLI

See the [CLI guide](cli.md) for commands, targets, sessions and exit codes.
The default target connects to the desktop instance. Independent mode starts
its own local engine with offline model access and a temporary data directory.
Independent mode reads the saved engine and model directory.
Use `--engine` to override the engine selection for that run.

```powershell
pnpm --silent cli doctor
pnpm --silent cli --target engine --threads 2 status
pnpm --silent cli --engine confucius4-r2t2 transcribe sample.wav --language en
python -m unittest discover -s scripts -p "test_*cli*.py" -v
```

Independent inference requires a native engine, its shared libraries and local model files.
Use `--engine-exe` to select a staged executable. `doctor` checks configuration and file presence.
It does not load a model. Capture command output, stderr and exit codes outside
runtime directories. Confirm that owned children exit after a test.
A separate engine run does not verify the desktop connection or the installed CLI.
The [native host verification](../formal/agent-api-host/README.md) runs the real
application control path without application windows. It uses an isolated profile
and a synthetic engine. Keep this lifecycle evidence separate from model inference.

The finite models, negative controls and correspondence boundaries are in
[CLI protocol verification](../formal/cli/README.md).

## Checks

Run from the repository root:

`pnpm check` starts with Git's whitespace check of staged and unstaged changes
against `HEAD`. Stage new files before running it so Git includes their added
lines. Editors that support EditorConfig apply the repository's whitespace rules;
Markdown line breaks, reviewed patches and the upstream license retain their
intentional whitespace. CI checks the committed range before installing dependencies.

```powershell
pnpm check
pnpm audit --prod --audit-level high
uv lock --check
uv run --no-sync python -m compileall -q scripts src-tauri/resources
uv run --no-sync python -m unittest discover -s src-tauri/resources -p "test_*.py"
uv run --no-sync python scripts/test_build_engine_atomicity.py
uv run --no-sync python scripts/test_cpu_cuda_backends.py
uv run --no-sync python scripts/test_native_source_checkout.py
uv run --no-sync python scripts/test_release_ci.py
uv run --no-sync python scripts/test_ci_fail_fast.py
uv run --no-sync python scripts/test_check_verus.py
uv run --no-sync python scripts/test_verus_ci.py
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --locked -- --skip services::qwen_hotword_service::tests::hotword_correction_p95_stays_below_one_millisecond
git diff --check
```

CI measures the skipped timing-sensitive hotword benchmark separately as advisory.
CI also runs the [TLA+ and Lean contracts](../formal/README.md), including verifier
regressions and intentional counterexamples, and uploads the proof logs. The
release gate requires this job to succeed for the exact candidate commit.
The application model runner uses one worker. `ContextRouting` has a 2048 MiB
Java heap limit; other models have a 512 MiB limit.
The separate Windows `Verus implementation proofs` job checks the actual capture
frame-count helper, hotkey ownership transitions and interim interval controller,
including deliberately faulty variants of each. It verifies the
download checksum and uses an isolated Rust 1.99.1 toolchain; the application
continues to build with Rust 1.93.0. This job is also required by the release gate.
For local execution and proof boundaries, see [Verus checks](../formal/verus/README.md).
Multi-command PowerShell steps stop on any native command failure. Rust caches
use OS, compiler, lockfile and source hashes; cache hits still run every check.

## Troubleshooting

**Hotkey not working**: the current default dictation hotkey is `F2`. Change it in Settings if another app owns it.

**GPU not detected**: run `nvidia-smi`. Run `light-whisper --target engine status`
to see which backend a separate local engine selects.

**Log locations**:

- App log: `%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- Desktop engine stderr: `%APPDATA%\com.light-whisper.app\funasr_stderr.log`

`LIGHT_WHISPER_DATA_DIR` overrides the ASR data directory. The Python reference
servers write `qwen3_asr_server.log` or `r2t2_asr_server.log` under its `logs`
directory, or under `%TEMP%\light_whisper_logs\` when it is unset.

**Automatic decisions**: `app.log` records the selected decision provider,
validated route/screen choices, probabilities and whether polishing was skipped.
`Decision model fallback` records HTTP status, transport, malformed-response or
timeout failures; these retain normal polishing and do not imply a `polish` decision.
For example, Vercel's HTTP 403 can indicate unavailable model access, while Liquid's
HTTP 429 can indicate temporary rate or capacity limits. Decision logs omit transcript
content, request bodies and credentials.

Auto-polish bypass and its automatic screen route use an 80% minimum choice
probability. A pass also requires a valid complete distribution and no explicit
screen, translation or edit requirement. Correction-rule deletion and meaning
audits keep their separate review thresholds.


## Documentation

Use [ASD-STE100 Issue 9](https://www.asd-ste100.org/assets/files/ASD-STE100_ISSUE9.pdf) for English documentation.
Use short sentences and consistent technical terms. Use the labels shown in the app.
Describe current features in the README files. Put version history in `CHANGELOG.md`.

## Repository map

| Path | Responsibility |
|:--|:--|
| `src/` | React UI, translations and frontend tests |
| `src-tauri/src/` | Rust capture, IPC, settings and service integrations |
| `rust-runtime/` | Rust speech engine, CLI and native inference bindings |
| `src-tauri/resources/` | Bundled assets and licenses, plus the Python reference engine and its tests |
| `scripts/` | Builds, release gates, benchmarks and pinned native patches |
| `formal/` | Versioned TLA+/Lean specifications, coverage registry and regression replay fixtures |
| `docs/` | Development, releases and R2T2 implementation details |
| `assets/`, `src-tauri/icons/` | Screenshots and application icons |

Generated native libraries, `engine.tar.xz`, model caches, `build/` and installers
are intentionally ignored. Rebuild them from the tracked scripts and pinned
dependencies; do not copy local caches or credentials into Git.

Keep `formal/` in Git: CI and regression tests load its models, configurations,
proofs and checked-in replay inputs. Per-run TLC logs, state databases, downloaded
verification tools and compiled Lean files are local artifacts. Write them under
`tmp/` (for example, `--logs tmp/formal-results`), not beside specifications.
The small evidence summaries and raw counterexample fixtures under `formal/`
remain versioned so another checkout can reproduce and validate the same inputs.

See [release procedure](releasing.md), [R2T2 backend](r2t2-native.md) and
[current API defaults](api-defaults.md).
