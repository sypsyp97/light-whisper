# Development

Build on Windows 10/11 x64:

| Tool | Version | Purpose |
|:--|:--|:--|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2022 (MSVC 14.44 validated) | Rust and shared ASR C++ toolchain |
| [Rust](https://www.rust-lang.org/tools/install) | 1.93.0 (CI) | Tauri backend |
| [Node.js](https://nodejs.org/) | 22.14.0 (CI) | Frontend build |
| [pnpm](https://pnpm.io/) | 10.28.2 (CI) | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | 0.11.30 (CI) | Python environment for local ASR |

Python 3.11 is selected by `.python-version`; the project supports Python 3.11–3.12. `uv` manages this environment.

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install --frozen-lockfile
uv sync --frozen
pnpm tauri dev
```

For Qwen 0.6B and R2T2 development, first build the shared native runtimes with the
commands below. Qwen and R2T2 support CPU and NVIDIA CUDA.
The development server loads DLLs beside its source script;
the installed application uses the self-contained engine archive.

Build a distributable installer. The bundled Python engine archive is intentionally
not stored in Git, so build it first:

```bash
uv run --locked python scripts/build_r2t2_runtime.py --backend cpu
uv run --locked python scripts/build_r2t2_runtime.py --backend cuda --cuda-root "C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA/v12.9"
uv run --locked python scripts/build_engine.py
pnpm tauri build
```

The NSIS installer is written to `src-tauri/target/release/bundle/nsis/`.
7-Zip uses a solid XZ archive, a 256 MiB dictionary and x86 BCJ filtering.
Files are ordered by size so large native libraries can share dictionary history.
The NSIS wrapper uses zlib around the precompressed engine.
The ONNX Runtime hook omits its standalone C API DLL while retaining the Python
binding and provider libraries. Normal dependency analysis still includes the C
API DLL if a collected binary imports it. Packaging also removes the nested Kaldi
core DLL only when its bytes match the copy at the bundle root.
The copied distribution excludes the retired transcribe.cpp packages and Vulkan
backend DLLs. Qwen 0.6B and R2T2 use only the shared audio.cpp runtime. CUDA/CRT
dependencies stay beside their native library unless an identical copy exists
at the bundle root; source native packages and their licenses stay intact.
`pnpm tauri build` rejects a missing, empty, or non-XZ engine archive instead of
producing an installer without local ASR. Reuse a verified archive only if its
Python code, native libraries and dependencies are unchanged. Distribution builds
compare the bundled Python sources and shared native build manifests with the
checkout, verify the native DLL sizes and hashes, and reject stale archives or
retired transcribe.cpp/Vulkan payloads;
rebuild the native packages and engine when this check fails.

Optional local-model prefetch:

```bash
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-0.6b
uv run python src-tauri/resources/download_models.py --engine confucius4-r2t2
```

For China mainland downloads, set `HF_ENDPOINT=https://hf-mirror.com` before prefetching.

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

**GPU not detected**: run `nvidia-smi`. Qwen3-ASR records its selected device/backend in `qwen3_asr_server.log`.

**Log locations**:

- App log: `%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- ASR logs: `%APPDATA%\com.light-whisper.app\logs\{qwen3_asr_server,r2t2_asr_server}.log`
- Python stderr fallback: `%APPDATA%\com.light-whisper.app\funasr_stderr.log`

`LIGHT_WHISPER_DATA_DIR` overrides the ASR data directory. Standalone Python
servers use `%TEMP%\light_whisper_logs\` when it is unset.

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


## Repository map

| Path | Responsibility |
|:--|:--|
| `src/` | React UI, translations and frontend tests |
| `src-tauri/src/` | Rust capture, IPC, settings and service integrations |
| `src-tauri/resources/` | Python ASR servers, bundled VAD and runtime licenses |
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
