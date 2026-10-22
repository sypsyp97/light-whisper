# Verus capture arithmetic pilot — 2026-10-22

This report preserves the initial pilot results. The subsequent
[integration and IPC replay repair](verus-integration-20261022.md) resolves the
failure recorded below and adds the CI/release gate.

The three capped audio mixers now call one ordinary Rust helper,
[`capped_frame_count`](../../src-tauri/src/services/audio_service/capture_bounds.rs).
Its result is exactly
`min(input_len / max(channels, 1), cap.saturating_sub(used))`.
Existing slicing, sample conversion and averaging are unchanged. No application
dependency, installer, global toolchain setting or CI gate was added or changed.
The local runner and reproduction instructions are in [the pilot guide](../verus/README.md).

## Results

| Check | Observed result |
| --- | --- |
| Original mixer bodies and existing tests, extracted without changes | 18 existing + 3 new golden tests passed before production extraction. |
| New helper tests | 3 passed, covering 5,356 explicit, small-domain and widened-arithmetic cases; also passed through the actual Cargo module wiring. |
| Actual audio module tests | 80 passed, 5 existing tests ignored. |
| Runner acceptance tests | 8 passed on Python 3.13.2; fake verifier output tests failure handling only. |
| Official Verus, final production body | 1 function verified, 0 errors, exit 0. |
| Three negative controls | Each typechecked with exit 0, then failed verification with exit 1 and one semantic error. |
| Existing formal inventory/provenance check | Passed: 128 commands, 19 event families, 16 lifecycle contracts, 14 protocols. No TLA+/Lean proof rerun was needed for unchanged models. |
| `cargo fmt --check` | Passed. |
| Full Rust library suite | **772 passed, 1 failed, 13 ignored**; not an all-green suite. |

The full-suite failure is
`services::funasr_service::ipc_recovery_tests::async_ownership_replay_tests::tlc_ipc_deadline_replays_write_progress_without_restarting_the_budget`,
at `ipc_recovery_tests.rs:379`. Its intermediate timeout assertion also failed in
one isolated rerun: 0 passed, 1 failed, 785 filtered out. That file and its owning
implementation are unchanged from the pilot baseline. The cause has not been
established or repaired in this bounded pilot; a separate investigation remains
necessary. This result does not justify claiming the full project regression
suite is green.

## TDD and review

The new API and runner first used ignored declaration scaffolds. Returning zero
caused three compiled helper assertion failures; an unconditional-success runner
caused 21 assertions to fail across seven tests. These are scaffold RED results,
not evidence that the original mixer arithmetic was faulty. The independent
reviewer accepted the frozen tests and proof specification before implementation.

Final review found that a valid-looking negative proof report followed by an
abnormal process exit could be accepted. A dedicated test-writer revision
reproduced two assertion failures for exits 2 and -9. Requiring the verifier's
observed semantic-failure exit 1 made the revised eight-test suite pass. Final
independent review accepted the pilot and retained the full-suite limitation.

## Proof and provenance

The runner inserts the production helper body verbatim into the reviewed Verus
template, with an explicit `x86_64-pc-windows-msvc` target and `--no-cheating`.
It proves exactness, input bounds, output capacity and arithmetic safety for
arbitrary `usize` inputs. The three separately generated faults remove the
zero-channel guard, remove the capacity limit, and replace saturating subtraction
with unchecked subtraction. Actual diagnostics were division by zero,
unsatisfied postcondition and possible underflow, respectively.

The trusted boundary includes Verus, its SMT solver, standard-library
specifications and the Rust compiler. Caller wiring is checked through review
and Rust tests. Mixing values, allocation, devices and concurrency are outside
this helper-body proof; it is not whole-program verification.

Final hashes (SHA-256):

- Production helper: `16947f35a1557e882e40765615f7ec39c3ae54cb970f8972d07f3e0cb9fd4a5e`.
- Proof template: `94b5374c6364351aabc39e82626e05af23e854e66f77e52bc6498cf2fa7633a0`.
- Runner: `9949096086d2d286af134860c02083fc1ce7644c9b59dca1cf40ec00bb3d7d0a`.
- Revised runner tests: `e85192a2bfe120d0b723eac9aad704daf3c6300d91ede0927893c45a7f7ecd16`.

The official Verus version was `0.2026.10.18.7d82994`, with isolated Rust 1.99.1.
The application tests used the existing Rust 1.93.0 toolchain. Heavy checks ran
serially at BelowNormal priority, two logical CPUs, one Cargo job and one Rust
test thread. All 752 baseline files were checked: only `formal/README.md`,
`capture.rs` and `audio_service/mod.rs` changed; earlier work was preserved.
The global Rust settings hash is unchanged and no Cargo config was created.

Ignored local evidence under `tmp/verus-pilot-20261022/`:

- `tests/`: baseline extraction, frozen test hashes, scaffold RED and abnormal-exit RED.
- `final-verus/summary.json`: exact commands, final source/template/tool hashes,
  four generated programs and all nine process results, with raw stdout/stderr.
- `python-final.log`, `cargo-tests.log`, `ipc-targeted-rerun.log`,
  `formal-inventory.log`, `rustfmt-final.log`, `preservation.json`.

No commit, push, release, installer rebuild or installed-app operation was performed.
