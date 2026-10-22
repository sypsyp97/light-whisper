# Verus workflow integration and IPC replay repair — 2026-10-22

The [Verus pilot](verus-pilot-20261022.md) is now integrated into local development,
Windows CI and the release script. The pilot's failing IPC replay has been
repaired, and the final Rust suite passes. This work integrates Verus directly;
it does not install GitHub Next's PDD Kit.

## Repair and test-first evidence

The old IPC replay spent a one-second real deadline on serialization of an 8 MiB
command, subprocess I/O and fixed waits. It failed twice during the pilot and
passed once unchanged during this investigation. The production transport already
uses one timeout across lock acquisition, writing, flushing and reading; its
implementation did not need changing.

The repaired test retains the real subprocess, pipe and production future. It
pauses Tokio time after child startup and advances it only for the three ticks
from the existing TLC traces. Real I/O has a separate bounded watchdog. A measured
one-millisecond timer-wheel allowance is applied at the final tick. Every event
asserts the elapsed virtual time; intermediate pending state, lock ownership,
actual frame consumption, cancellation cleanup and fresh-server recovery remain
checked.

In an ignored snapshot of the actual implementation, all three unmodified trace
replays passed. Resetting the deadline after write progress, removing the write
deadline, and retaining a cancelled connection each compiled and then failed the
intended replay assertion. These are semantic failures, not build failures. The
canonical workspace was rebuilt after these mutation checks.

Before release-gate implementation, its five incumbent scenarios passed and four
new scenarios failed: missing, skipped, failed or cancelled Verus jobs incorrectly
allowed success. All nine now pass. The initially absent CI step was recorded as
`INTERFACE_PENDING`, not as a semantic test failure. The implemented PowerShell
step also passes checksum and native-command failure injection checks.

Full Clippy validation caught duplicate compilation of the capture helper by its
test module. The dedicated test-writer repair changed that module to import its
existing sibling. All three arithmetic test bodies remained byte-identical;
Clippy then passed without suppressing the warning.

## Integration

- `pnpm check:verus --verus <exe> --logs <directory>` runs the production-body
  checker locally. The [guide](../verus/README.md) documents isolated installation.
- `Verus implementation proofs` is an unconditional Windows job within the
  existing CI triggers. The official archive version and SHA-256 are pinned;
  verification precedes extraction. Rust 1.99.1 uses temporary `RUSTUP_HOME` and
  `CARGO_HOME`; application Rust remains 1.93.0.
- The job stops on native command failures and uploads verifier evidence even
  after failure. The release script requires this exact job to succeed for the
  candidate commit, alongside Frontend, Python, Rust and Application formal
  contracts. No runtime or Cargo dependency was added.

## Final validation

| Check | Observed result |
| --- | --- |
| Rust format and all-target Clippy with `-D warnings` | Passed. |
| Rust all-target suite, with advisory benchmark separated | 788 passed, 0 failed, 13 existing ignored; one benchmark filtered. |
| Separate hotword advisory benchmark | 1 passed; coverage 100%, false positives 0, p95 502 microseconds. |
| Python CI/release/atomicity tests | 56 passed: 41 existing engine atomicity, 2 existing CI failure handling, 9 release gate, 4 Verus integration. |
| Runner acceptance tests in the actual CI step | 8 passed on Python 3.11.11. These use fake verifier output and are not proof evidence. |
| Unmocked extracted CI PowerShell step | Exit 0; reused the official downloaded archive after checking its digest, extracted it, checked/installed the isolated toolchain, and ran the real verifier. |
| Actual Verus positive proof | 1 function verified, 0 errors. |
| Actual Verus negative controls | All three typechecked, then failed verification with exit 1 and one semantic error each. |
| Fresh IPC TLC model | Positive: 21 distinct states, no error. |
| Fresh IPC TLC negative controls | Deadline reset: 21 states and `OneTotalDeadline`; unbounded write: 22 and `OneTotalDeadline`; cancellation reuse: 10 and `InvalidFrameCannotBeReused`. Each exited 12. |
| Inventory and correspondence checks | Passed: 128 commands, 19 event families, 16 lifecycle contracts, 14 protocols; all stored trace provenance and request snapshots checked. |

The rest of the unchanged TLA+/Lean models were not rerun in this follow-up.
Heavy checks ran serially with BelowNormal priority, two logical CPUs, one Cargo
job and one Rust test thread. The checks do not use real microphones, model
inference or saved credentials.

The proof covers the actual `capped_frame_count` body for arbitrary Windows x64
`usize` inputs. It does not establish correctness of the entire app, mixer
arithmetic, device behavior or concurrent runtime. The IPC model has finite
bounds and three implementation replay schedules. Remote GitHub CI still needs
to execute after an authorized push; this report records local execution of its
actual step, not a remote workflow run or branch-protection change.

## Evidence and preservation

Ignored local evidence under `tmp/verus-integration-20261022/`:

- `ipc/audit.json`, trace baseline/mutation results, diffs and logs: actual
  production mutations, assertion failures and the frozen repaired replay.
- `ci-tests/red-evidence.md`, `gate2.md`, `ci-tests-green.log`: test-first release
  evidence, acceptance and final script tests.
- `clippy.log`: original duplicate-module failure; `rustfmt-final.log`,
  `clippy-final.log`, `rust-tests.log`, `hotword-advisory.log`: final canonical
  workspace validation.
- `ci-step.ps1`, `ci-step-real.log`, `ci-python-version.log`: exact extracted CI
  step and unmocked local execution. Extraction SHA-256:
  `d71a5c668fd7dd0c9147f5d0fb711bbbd7f4adc43be2f1e5b891ba48ddd2b670`.
- `formal/summary.json`, `formal-targeted.log`, `formal-inventory.log`: fresh
  model checking and correspondence checks.
- `source-before.json`, `preservation.json`: hashes of all 760 baseline files,
  the explicit change allowlist, frozen tests and unchanged global Rust settings.

The actual CI-step proof summary and all generated programs/diagnostics are in
`tmp/verus-pilot-20261022/verus-results/summary.json`. This run reused the pilot's
isolated tools directory. Its source, template, checker and verifier hashes match
the final workspace. The official archive SHA-256 is
`82b9558a3c4fa77673700507c066c711535d130c2789dcfcec30878168db20ec`.

Earlier uncommitted work is preserved. No commit, push, release, installer build,
installed-app operation or global configuration change was performed.
