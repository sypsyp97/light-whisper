# Verus implementation proofs

This check verifies three production domains. The app has no Verus runtime or
Cargo dependency; its normal Rust implementations remain the executable source.

| Domain | Executable functions | Contract |
| --- | --- | --- |
| [Capture bounds](../../src-tauri/src/services/audio_service/capture_bounds.rs) | `capped_frame_count` | Exact complete-frame count, input/output bounds and arithmetic safety for arbitrary `usize` inputs. |
| [Hotkey ownership](../../src-tauri/src/commands/hotkey/start_owner.rs) | `StartOwnership::{begin, bind, release, fail}` | Exact state transitions for arbitrary generations, active states and optional session IDs. Rejections preserve state; generation exhaustion cannot wrap. |
| [Interim interval](../../src-tauri/src/services/audio_service/interim.rs) | `adjust_interval` | Exact load-dependent adjustment and preservation of the 140–460 ms range, assuming the current interval is in that range. |

The hotkey mutex wrapper retains the same API and holds one lock across each
transition. Its exhaustion panic remains outside the total, optional-result
`StartOwnership::begin` method. The interval loop starts at 220 ms and both of
its updates call the verified controller; this caller invariant is checked by
source review and regression tests. All elapsed `u64` values are allowed.

## Capture contract

For arbitrary `usize` inputs on the Windows x86-64 target, the contract proves:

- Zero channels are treated as one.
- The result is exactly the smaller of complete input frames and remaining capacity.
- Consuming that many frames stays within the input slice.
- An already full or over-capacity output appends nothing; otherwise its new
  length remains within the cap.
- The helper has no division by zero or arithmetic overflow.

## Source binding and controls

The runner extracts production bodies verbatim into reviewed templates. It binds
the hotkey state structure and all seven interval constants to the actual source,
and rejects unsupported signatures, context, mutation anchors and local proof
trust escapes. The expected executable inventories are one capture function,
four state methods and one interval function. Each positive must verify its full
inventory; omitted functions and unrelated extra proof counts fail the gate.
The pinned verifier additionally counts the seven interval constant initializers,
giving exact positive counts of **1 / 4 / 8**. The interval template uses a nested
module to retain their `pub(crate)` declarations verbatim; constants-only output
of seven verified obligations is rejected.

Every generated program is typechecked before verification. Independent faulty
bodies must then fail actual verification: capture normalization/capacity/subtraction;
hotkey stale bind/duplicate bind/generation wrap/stale failure/release cleanup;
interval upper cap/lower floor/heavy threshold. Syntax errors, crashes, missing
diagnostics and empty or partial proof reports do not count as successful evidence.
Per-domain sources, commands, output and hashes are retained under the logs directory.

## Reproduce on Windows

Use the official [Verus release 0.2026.10.18.7d82994](https://github.com/verus-lang/verus/releases/tag/release/0.2026.10.18.7d82994)
and its required Rust **1.99.1** toolchain. The Windows archive
`verus-0.2026.10.18.7d82994-x86-win.zip` has SHA-256
`82b9558a3c4fa77673700507c066c711535d130c2789dcfcec30878168db20ec`.
Check the digest before extracting or executing it. Keep downloads and isolated
`RUSTUP_HOME` / `CARGO_HOME` directories under ignored `tmp/`; no global Rust
default or project toolchain file needs to change.

With that toolchain available in the current process:

```powershell
pnpm check:verus --verus C:/path/to/verus-x86-win/verus.exe --logs tmp/verus-results
python -m unittest discover -s scripts -p test_check_verus.py -v
python scripts/test_verus_ci.py
cargo test --manifest-path src-tauri/Cargo.toml --lib services::audio_service:: -- --test-threads=1
cargo test --manifest-path src-tauri/Cargo.toml --lib commands::hotkey::start_owner::tests -- --test-threads=1
```

The Python acceptance tests use a fake executable to test the runner's failure
handling. Their output is not mathematical proof evidence. The separate command
using the official Verus executable supplies that evidence.

## CI and release enforcement

The Windows `Verus implementation proofs` job runs for every existing CI trigger,
alongside the TLA+/Lean job. It checks the pinned archive digest before extraction,
uses `RUSTUP_HOME` and `CARGO_HOME` under the runner's temporary directory, and
fails immediately on acceptance-test, toolchain or proof failures. It always
uploads generated sources, commands, hashes and verifier output. The application
Rust toolchain and runtime dependencies are unaffected.

The release script requires this exact job to exist and succeed for the candidate
commit. Missing, skipped, cancelled and failed results all block publication.
The local entry point is separate from `pnpm check`, which also runs on Linux for
frontend validation. This workflow integration does not change remote branch
protection settings or extend proof coverage to unverified functions.

## Boundary

The proofs concern these executable arithmetic/state transitions, with no finite
input enumeration. They do not verify the compiled app, mutex implementation,
OS hooks, device behavior, scheduling latency or the asynchronous inference loop.
Rust regression tests and source review check callers and unchanged mixer sample
conversion. Averaging, floating-point accuracy, allocation and audio concurrency
remain outside the capture proof. The verifier, SMT solver, standard-library
specifications and Rust compiler remain trusted dependencies. Existing TLA+/Lean
state and protocol contracts continue to provide their separate evidence.

See the [core coverage results](../evidence/verus-core-20261022.md) for this expansion,
the [integration and IPC replay repair results](../evidence/verus-integration-20261022.md)
for the final validation, or the [original pilot results](../evidence/verus-pilot-20261022.md)
for the initial measurements.
