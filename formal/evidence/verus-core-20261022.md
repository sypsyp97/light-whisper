# Core Verus coverage — 2026-10-22

Two core domains now join the existing capture proof in the default local and CI
command. The existing release gate therefore requires all three domains. The
application has no new runtime dependency or toolchain requirement.

| Domain | Production behavior proved | Explicit boundary |
| --- | --- | --- |
| Capture bounds | Exact complete-frame count, slice/cap bounds and arithmetic safety. | Sample conversion, mixing and devices remain outside this proof. |
| Hotkey ownership | `begin`, `bind`, `release`, `fail` preserve exact state transitions; stale/duplicate requests cannot take another session; generation exhaustion cannot wrap. | Arbitrary initial state is accepted. Mutex internals, OS hooks and asynchronous scheduling are not proved. |
| Interim interval | Exact adjustment for idle, light, middle and heavy load; output remains in 140–460 ms and moves in the specified direction. | Requires the current interval in that range; all `u64` elapsed values are accepted. It does not guarantee real-time latency or inference throughput. |

The interval caller starts at 220 ms and both subsequent assignments pass through
`adjust_interval`. This establishes the source-reviewed induction boundary. The
function body and its seven production constants are unchanged. Hotkey state
changes moved into methods on the existing `StartOwnership`; the outer API keeps
one mutex guard for each operation and retains its existing exhaustion panic.

## Test-first development

The original nine focused Rust tests passed before changes. Eight additional
tests fixed the incumbent behavior before refactoring: arbitrary hotkey states,
rejected operations, duplicate actions and exhaustion; and 7,704 interval inputs
against independent widened arithmetic and directional/range properties. All
17 passed on the original implementation and after the final refactor.

Two isolated snapshots of the actual source passed. Five separately compiled
faults then failed their intended assertions: stale binding, duplicate binding,
generation wrap, removed upper cap and removed lower floor. These are regression
sensitivity checks, not a claim that the baseline had those defects. The hotkey
snapshot uses the real cached `parking_lot` dependency, with no lock stub.

The runner's new default-coverage test first failed against the unchanged CLI:
it returned success while checking only capture. New template/method interfaces
were separately recorded as `INTERFACE_PENDING`. Independent review accepted the
frozen Rust and runner tests before implementation. Local review was used rather
than sending repository source to an external review service.

Pinned Verus cannot use the original `pub(crate)` constants at proof-crate root.
A nested module preserves every declaration verbatim and verifies them, counting
seven initializer obligations in addition to the controller. The revised test
first rejected this valid eight-obligation output against the old count of one;
review then accepted exact counts **1 / 4 / 8**, with seven-only and extra-count
outputs rejected. No assumption or visibility rewrite was introduced.

Clippy also caught an early-return match that could use `?`. A small `if let`
retained standard Rust, verifier support and the frozen behavior without a lint
suppression. Final formatting, Clippy and the focused tests passed.

## Actual proof results

`pnpm check:verus` now runs all domains by default. Fourteen generated programs
were typechecked and verified using the pinned official Verus executable:

- Positive capture: **1 verified, 0 errors**.
- Positive hotkey ownership: **4 verified, 0 errors**.
- Positive interval: **8 verified, 0 errors** (one function, seven initializers).
- **11 negative controls:** each typechecked with exit 0, then failed verification
  with exit 1 and one semantic error. Controls cover capture normalization/cap/
  subtraction, stale/duplicate hotkey bind, generation wrap, stale failure, release
  cleanup, and interval cap/floor/heavy threshold.

The checker binds actual function bodies, state structure, constants, source and
template hashes. It checks the executable inventory separately from initializer
counts; rejects unsupported context, aliases, conditional definitions, trust
escapes and mutation drift; and retains every command and diagnostic. All source,
generated-program, runner and executable hashes are rechecked during the run.
Independent review reconstructed all fourteen programs and their mutations.

The verifier, compiler, SMT solver and pinned standard-library specifications
remain trusted. The reviewed [Option specifications](https://github.com/verus-lang/verus/blob/release/0.2026.10.18.7d82994/source/vstd/std_specs/option.rs)
and [integer specifications](https://github.com/verus-lang/verus/blob/release/0.2026.10.18.7d82994/source/vstd/std_specs/num.rs)
provide the contracts for `take`, `checked_add` and the negative wrapping control.
This is executable-body verification for the stated domains, not whole-app or
compiler-refinement proof.

## Final regression gates

| Check | Observed result |
| --- | --- |
| Rust format and all-target Clippy with `-D warnings` | Passed on the final source. |
| Rust all-target suite, advisory timing test separated | 796 passed, 0 failed, 13 existing ignored; one advisory test filtered. |
| Separate hotword advisory | 1 passed; coverage 100%, false positives 0, p95 501 microseconds. Total Rust passed: **797**. |
| Expanded verifier acceptance tests | 13 passed on Python 3.11; fake verifier output tests runner behavior only. |
| Release/CI regression tests | 9 release gate + 2 native failure handling + 4 Verus CI tests passed. Total scripts passed: **28**. |
| Fresh `HotkeyPendingStart` TLC check | Positive: 25 distinct states, no error, including the configured progress property. |
| Fresh legacy hotkey negative | 15 states; exit 12 with `ReleasedOrSupersededStartCannotBind` violated. |
| Formal inventory/provenance | Passed: 128 commands, 19 event families, 16 lifecycle contracts, 14 protocols; stored trace provenance and request snapshots checked. |

Other unchanged TLA+/Lean models were not rerun. Checks ran serially at
BelowNormal priority, two logical CPUs, one Cargo job and one Rust test thread.
Earlier uncommitted work is preserved: the 762-file baseline has exactly seven
authorized modified files and three additions, with no missing or unexpected
changes. Frozen test hashes, final proof inputs and global Rust settings match
their recorded values; no global Cargo configuration was created.

## Evidence

Ignored artifacts are under `tmp/verus-core-20261022/`:

- `contract.md`, `gates.md`, `source-before.json`: scope, reviewed revisions and
  the 762-file workspace baseline.
- `behavior-tests/README.md`, `snapshot-controls.json`, `artifact-sha256.json`:
  baseline/expanded Cargo runs and five compiled mutation failures.
- `runner-tests/`: original coverage RED, the narrow count-revision RED, frozen
  test hashes, fixtures and exact process records.
- `implementation/result.json`: production/template hashes, before/after focused
  tests, Clippy RED/GREEN and initial positive proof diagnostics.
- `final-verus/summary.json`, `verus-final.log`: final three-domain run, all source
  hashes, fourteen generated programs and 29 verifier invocations.
- `runner-final.log`, `test_release_ci.log`, `test_ci_fail_fast.log`,
  `test_verus_ci.log`: final 28 script tests.
- `rust-final.log`, `hotword-advisory.log`: full Rust regression and separate
  timing advisory.
- `formal/summary.json`, `formal-final.log`, `formal-inventory.log`: fresh hotkey
  positive/negative model runs and inventory/correspondence checks.
- `preservation.json`: the baseline comparison, final file hashes, frozen tests,
  proof input/program hashes and global-configuration audit.

The default command remains the entry point used by the existing Windows CI job;
no selector can silently reduce its domain coverage. Remote GitHub CI has not
been run for these local changes. No commit, push, release, installer operation,
live microphone/model use or global configuration change was performed.
