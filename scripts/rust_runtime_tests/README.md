# Native runtime parity checks

Run the dependency-free engine and host drivers with a local Python interpreter. The packaged product runs independently of these test tools. Each driver accepts `--help` and writes its JSON receipt outside the source tree.

`engine_protocol.py` compares the native process with a reference from the same backend, source checkout, model cache and checksum-verified natural audio. Generate the reference by omitting `--candidate`, then pass its output as `--oracle` for the packaged native engine. On a CUDA machine the CPU scenario requires the CPU-only resource layout. The Qwen preview and audio-processing scenarios use the same two-pass pattern. The public download driver runs its own loopback HTTP server. Its resume case privately stages all but the last 4096 bytes of a real pinned weight.

`installed_roundtrip.py` accepts an extracted NSIS payload, an authentic reference receipt and a task-owned work directory with space for the model and unpacked engine. It runs the zero-window desktop host and a native CLI, checks both public transcription routes, and exits every owned process. `--compare-source` runs three warmed source/native pairs on the same backend. It alternates order and keeps only one model active for each measurement.

The following sample commands assume the authorized baseline checkout and private test evidence directory are supplied through environment variables. CPU inference uses verified offline model assets. Running the lightweight Cargo workspace alone does not replay these external reference vectors.

```powershell
$env:LW_REUSE_TEST_RESOURCES = Join-Path $env:LW_BASELINE 'src-tauri/resources'
$env:LW_REUSE_TEST_VECTORS = Join-Path $env:LW_TEST_OUTPUT 'reuse-vectors'
$env:LW_REUSE_TEST_MODEL = Join-Path $env:HF_HUB_CACHE 'models--nvidia--Real-time_RE-USE/snapshots/4c0c186272ca853cd18d832fc5c28777b62f32d1/model.safetensors'
& (Join-Path $env:LW_BASELINE '.venv/Scripts/python.exe') scripts/rust_runtime_tests/reuse_oracle.py --source-root $env:LW_BASELINE --model $env:LW_REUSE_TEST_MODEL --output $env:LW_REUSE_TEST_VECTORS
if ($LASTEXITCODE) { throw 'reference generation failed' }
cargo test --offline --manifest-path scripts/rust_runtime_tests/reuse_vectors/Cargo.toml --test reuse_vectors
```

The numerical test covers 28 observed operations. It requires exact sample counts at every block boundary, maximum absolute error at most 0.0001 and waveform RMS error at most 0.00002. The reference records the hashes of the source, model, CPU DLL, fixture and each float32 vector. Finite-model replay and the negative controls are documented in `formal/rust-runtime/product/README.md`.

For a fresh Windows workspace test, set `LW_ORT_SDK` to the extracted Microsoft.ML.OnnxRuntime 1.24.1 NuGet package and `LW_KALDI_SDK` to the kaldi-native-fbank 1.22.3 wheel SDK. Use a private Cargo output directory and copy the pinned DLLs beside the integration test executables before running Cargo. An installed System32 copy of ONNX Runtime may have a different ABI.

```powershell
$env:CARGO_TARGET_DIR = Join-Path $env:LW_TEST_OUTPUT 'workspace-target'
$deps = Join-Path $env:CARGO_TARGET_DIR 'debug/deps'
New-Item -ItemType Directory -Force $deps | Out-Null
Copy-Item (Join-Path $env:LW_ORT_SDK 'runtimes/win-x64/native/onnxruntime.dll') $deps
Copy-Item (Join-Path $env:LW_KALDI_SDK 'lib/kaldi-native-fbank-core.dll') $deps
cargo test --offline --locked --workspace --manifest-path rust-runtime/Cargo.toml
```

The default Windows workspace run checks the bundled VAD probability fixture. Tests explicitly marked as external-asset cases require their corresponding oracle, model or hardware variables from the individual test. Run those cases separately for release parity.
