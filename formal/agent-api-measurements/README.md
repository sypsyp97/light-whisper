# Native agent API measurements

This workspace measures the Windows desktop agent API through the actual native
host, application callers and focused security, transport and packaging suites.
It uses synthetic local fixtures. It does not qualify an installed application.
See the [CLI verification record](../evidence/agent-cli-180.md),
[native queries](../agent-api-app/README.md) and [host lifecycle](../agent-api-host/README.md).

## Scope and current status

[cohort.json](cohort.json) binds ten production source files, their affected
functions or spans, maintained tests and the trusted native fixture. Its stable
keys retain their names when accepted test counts change. Current source/test
pins and historical run inputs must remain distinct. Measurement helpers change
only owned disposable mirrors; they do not change shipping source or installed
files.

| Surface | Recorded result | Current boundary |
| --- | --- | --- |
| Preparation and profile-flush helpers | Earlier 26-test helper run passes | Helper safety evidence; full auxiliary-tool mutation runs are outside the accepted scope and do not establish application coverage |
| Native host | All 20 current cases and cleanup receipts pass | Synthetic IPC through the actual windowless host; normal GUI operation is outside this check |
| Security / transport | All 23 / 35 native tests pass; current LLVM collection is complete | Selected raw branch unions are 57/82 and 129/144; remaining error paths are open and full framework mutation execution is outside the accepted scope |
| Query callers | All 34 tests pass; two fixtures remain ignored | The controller span covers 52/56 raw branch outcomes; full framework mutation execution is outside the accepted scope |
| Controller admission | All 32 finite configurations match expected semantic outcomes | One positive, 16 safety controls, one progress control and 14 reachability witnesses; not a full lifecycle proof |
| Desktop setter | Actual Wry command and restored source-control runs pass | Current setter coverage is 9/10 raw branch outcomes; full framework mutation execution is outside the accepted scope |
| CLI selector | All eight tests and the restored guard-control run pass | Current selector coverage is 10/10 raw branch outcomes within the declared function |
| Entrypoint observer helper | The current 32-test baseline passes after independent safety review; selected coverage is 73/73 lines and 24/24 arcs | Complete helper mutations are outside the accepted scope. The supplemental native recipe stops at artifact admission after its first build; it establishes no native routing outcome |

The stable metadata keys `security22`, `transport34` and `query24` now identify
23, 35 and 34 current native tests respectively. Two native fixture bodies remain
ignored. The query cohort calls actual `controller::dispatch` and includes
lifecycle-wait recording/startup publication, cancellation ownership and saved
configuration revalidation. The controlled publication actors do not capture a
microphone or operate the normal GUI. They reject unsafe admission before native
effects; this does not claim an actual engine start/restart in those cases.

The supplementary admission model passes all 32 outcomes. Its positive run
generates 51,448 states, finds 20,476 distinct states and reaches depth 19 with
an empty queue. All 17 current source bindings remain unchanged. The earlier
28-case run, including its 37,376/14,092 positive counts, is historical. The
32-case result is independent of the later CLI input-validation change and is
retained without a redundant CLI replay.

The security addition calls actual private-file creation after an independently
observed restricted-token denial. An error-code control produces the intended
assertion failure, and restored production passes all 23 cases. The transport
addition checks twenty malformed failure partitions and an exact valid decline.
Two official negation controls and two separate guard relaxations produce
semantic failures; restored production passes all 35 cases. These tests are
integrated. Current LLVM collection uses these exact tests. The full framework
mutation run is outside the accepted scope and is not executed.

## Safety and resource bounds

Use owned external output, target, evidence and temporary directories. Keep paths
short for Windows tools. Do not reuse an active build or coverage target.
Use at most two Cargo build jobs, one test thread and one mutation worker.
Run heavy work serially, with offline, locked dependencies after mirror preparation.

Do not launch normal GUI startup, use real user profiles, access credentials,
activate a microphone, download models or make cloud/inference probes.
The desktop command fixture clears configured windows, disables the tray and
does not run the event loop. Tao can still create a hidden event HWND/helper thread;
the checks observe configuration and managed webview windows, not every native HWND.

The bridge checks source, test, application and fixture identities before launch.
It rejects changes to the early dispatch and window-clearing prefixes. Python
waits for assignment to an owned kill-on-close Windows Job; the native driver adds
its own job and exact process handles. Watchdogs and cleanup affect owned processes
and descendants only. Keep per-run identities, logs and cleanup receipts.

## Prepare a disposable mirror

Run from the source checkout. Replace each angle-bracket path before use:

```text
python -m unittest discover -s formal/agent-api-measurements -p "test_*.py" -v
python -m unittest discover -s formal/agent-api-measurements/entrypoint -p "test_*.py" -v
python formal/agent-api-measurements/prepare.py prepare --output <owned-new-output>
cargo metadata --offline --format-version 1 --filter-platform x86_64-pc-windows-msvc --manifest-path <owned-new-output>/repo/Cargo.toml
python formal/agent-api-measurements/prepare.py check-lock --output <owned-new-output>
```

Preparation validates frozen source and test hashes before creating output. It
copies tracked files and this measurement directory, rejecting links, junctions,
traversal, existing outputs and source/output overlap. Ignored archives, models,
environments, installed applications and old build directories are not copied.
The mirror receives the workspace manifest and lock union. Registry/Git package
name, version, source and checksum identities must match the original app lock;
local workspace members are allowed. Git identity is recorded separately from
frozen source identity. Preparation makes no commit or publication.

## Build and run maintained suites

In the prepared mirror, set command-local `LW_MEASUREMENT_PYTHON` to the project
Python 3.11.11, `LW_MEASUREMENT_FIXTURE` to the built, hash-bound tiny native fixture,
and `LW_MEASUREMENT_EVIDENCE` to a fresh external directory. Set
`CARGO_BUILD_JOBS=2`, `CARGO_NET_OFFLINE=true` and an owned `CARGO_TARGET_DIR`.

```text
cargo test --offline --locked -p light-whisper --lib --test agent_api_native_measurement --no-run -j2
cargo test --offline --locked -p light-whisper --test agent_api_native_measurement agent_api::tests::actual_native_host -j2 -- --test-threads=1 --nocapture
cargo test --offline --locked -p agent-api-security-verification -j2 -- --test-threads=1
cargo test --offline --locked -p agent-api-transport-verification -j2 -- --test-threads=1
cargo test --offline --locked -p light-whisper --lib agent_api::tests -j2 -- --test-threads=1
cargo test --offline --locked -p agent-api-measurement-selector -j2 -- --test-threads=1
```

The first command builds the actual application binary and bridge without running
them. For the bridge's semantic negative control, copy only the trusted fixture to
a new owned `negative/debug/light-whisper.exe` and set
`LW_MEASUREMENT_MARKER_NEGATIVE_EXE` to that copy. The bridge verifies its fixture
hash and runs only `test_01_compiled_protocol_marker`, which reads PE bytes and
must fail the missing-marker assertion. It never launches the negative PE.
Preserve that assertion result; remove the variable before the real baseline.

The [entrypoint observer](entrypoint/README.md) provides a separate restricted
recipe for reviewed routing controls. It preserves routing decisions and exits
before the normal GUI builder. Only exact reviewed source variants, arguments and
owned environments are admitted. An empty-data client probe requires an explicit
empty `LIGHT_WHISPER_DATA_DIR`; an absent variable is refused. Call
`verify_launch` immediately before every low-level probe. Helper tests do not
establish that a native baseline or predicate-inversion replay has executed.
The recipe also binds the observed source Git HEAD. On another checkpoint,
review the source/test changes before rebinding `entrypoint/reviewed.json`;
keep its 21 content pins and per-run Git provenance distinct.

## Collect coverage

Use a fresh owned target, command-local isolated `RUSTUP_HOME`,
`RUSTUP_TOOLCHAIN=nightly-2026-11-14` and cargo-llvm-cov 0.9.1. Obtain the branch
instrumentation environment once with `llvm-cov show-env --branch
--no-rustc-wrapper --manifest-path <mirror>/Cargo.toml`. Parse only the named
assignments; do not evaluate arbitrary output. Then run the accepted test
selections with ordinary pinned Cargo under that environment. Invoking
`llvm-cov test` again can duplicate flags and replace the custom profile path.

Run query tests with `--lib agent_api::tests`, and the native host with
`--test agent_api_native_measurement agent_api::tests::actual_native_host`.
Run the security, transport and selector packages separately. Keep offline,
locked dependencies, two build jobs and one test thread. Verify the actual
compiler flags and PE coverage sections.

Windows process exit can discard LLVM counters. The verification-only
[profile-flush helper](profile_flush.py) writes counters after awaited engine stop
in the real Exit callback, under `cfg(coverage)`. It requires matching source/test
identities, exclusive mirror files and an owned external target receipt. Set both
`CARGO_TARGET_DIR` and `CARGO_LLVM_COV_TARGET_DIR` to that same absolute target,
`LLVM_PROFILE_FILE` to `<target>/fresh-flush/flush-%p-%m.profraw`, and retain
`--cfg=coverage` and `-C instrument-coverage` in `RUSTFLAGS`. The profile directory
must be fresh and directly inside the target. Reparse points and hardlinked host
files fail closed. Keep backups, diffs, hashes and the dynamic line map.

```text
python formal/agent-api-measurements/profile_flush.py apply --mirror <owned-output>/repo --profiles <owned-short-target>/fresh-flush
python formal/agent-api-measurements/profile_flush.py check-logs --evidence <owned-native-run>
cargo-llvm-cov llvm-cov report --json --include-build-script --output-path <raw-llvm.json>
cargo-llvm-cov llvm-cov report --text --include-build-script --output-path <raw-llvm.txt>
python formal/agent-api-measurements/profile_flush.py restore --mirror <owned-output>/repo --profiles <owned-short-target>/fresh-flush
```

Apply before instrumented build/run; check logs and export afterward, then restore.
A flush-failure marker invalidates measurement. Restoration refuses later edits.
Merge only fresh run profiles and retain exact executable objects. Automatic
reporting with `--include-build-script` omitted the custom-path executions in
the current run. The qualified result uses the matching pinned LLVM tools:
`llvm-profdata merge -sparse -num-threads=2 -f <profile-list>`, then
`llvm-cov export` and `llvm-cov report` with that profile and the exact fresh
object list. Include regenerated build-script objects. Keep excluded attempts
and source aliases separate; do not combine unrelated collections.
The [LLVM runtime contract](https://clang.llvm.org/docs/SourceBasedCodeCoverage.html#using-the-profiling-runtime-without-static-initializers)
defines zero as write success and permits repeated writes to append. The ABI
control observes one hit after explicit flush/exit and two after flush/ordinary
return. Do not merge duplicate or unrelated collection attempts.

## Current measurement denominators

The current collection passes query34 (plus two ignored fixtures), native20,
security23, transport35 and selector8. It uses 1,084 custom-path profiles and
14 exact objects from the corrected instrumentation environment. The 801 initial
profiles and two initial objects are archived separately and excluded from this
qualification. All 20 native cleanup receipts pass; none of 62 stderr logs
contains a flush-failure marker. All 21 source/test pins match after restoration.

These are LLVM's canonical whole-file counts. The host includes 11 overlay lines.
Raw aliases remain separate and must not be summed.

| Source | Lines | Branches | Regions | Functions |
| --- | ---: | ---: | ---: | ---: |
| secure_files.rs | 275/355 | 42/78 | 261/360 | 22/33 |
| transport.rs | 430/534 | 80/142 | 527/712 | 51/70 |
| controller.rs | 233/256 | 48/56 | 259/303 | 19/25 |
| host.rs, transformed | 84/94 | 7/12 | 92/109 | 11/11 |
| lib.rs | 18/583 | 8/104 | 22/492 | 5/48 |
| commands/funasr.rs | 52/734 | 9/120 | 44/775 | 6/118 |
| services/funasr_service.rs | 406/1491 | 44/233 | 383/1615 | 38/152 |
| protocol.rs | 50/78 | 9/18 | 45/79 | 5/8 |
| build_support.rs | 31/64 | 7/20 | 28/58 | 6/9 |
| mod.rs | No executable mapping | No branches | No regions | No functions |

The derived original-coordinate inventory unions compiled aliases by source
position and branch outcome: private files 57/82, transport 129/144, controller
52/56, host 6/10, early entrypoint 8/12, setter 9/10, JSON bridge 2/2 and selector
10/10: **273/326 raw branch outcomes**, with **53 uncovered**. This union differs
from canonical whole-file counters and makes no feasibility adjustment. Host
line/region percentages in original coordinates remain unmeasured. Serde spans
have no branch inventory; no MC/DC or full correctness claim is made.

Seventeen of 20 qualified host PIDs retain profiles. Two early data-directory
rejections and one intentional hard kill retain none and remain unmeasured.
LLVM-inferred counters after an explicit flush do not prove later execution.
Normal merge, JSON export and text report succeed. Two profile-hash warnings
identify a dependency function, `rand::ThreadRng::next_u64`; they are retained.
A later optional diagnostic dump crashes and is preserved separately. It does
not supply the qualified counters. Independent receipt review reproduces the
selected counts and verifies the object, artifact and restoration identities.

## Mutation accounting and remaining limits

Use cargo-mutants 27.1.0 with the mirror's absolute root manifest and explicit
production paths. Confirm discovery resolves physical mirrored source without
outside-tree warnings. For example:

```text
cargo-mutants mutants --manifest-path <absolute-output>/repo/Cargo.toml --package agent-api-security-verification --file secure_files.rs --list --json
```

The current official generator discovery retains 412 raw entries: private files
121, transport 191, controller 58, host 6, command 10, JSON bridge 8, selector 13
and early entrypoint 5. The preserved execution plan selects 396 safe native
mutants across ten execution contexts. This full run is outside the accepted
scope and is not executed. Sixteen entries remain visible in the raw inventory: four unsafe/undefined or
null-DACL holds, six protected-prefix refusals and six entries outside the JSON
bridge function. None is an assertion kill or a waived raw denominator. The five
early-entrypoint entries belong to the protected-prefix refusals and have a
separate restricted observer recipe requiring five fresh builds.

The earlier 398-mutant affected inventory and its five additional entrypoint
items are historical. The current full framework run is not executed. Static list
discovery and accepted preparation do not establish test, coverage or mutation
success. The selective setter/selector source controls are sensitivity evidence,
not completed framework outcomes. Serde declarations have no generated affected
mutant; preserve that blind spot.

Use real callers, baseline-derived timeouts, one mutation worker and at most two
compiler tasks. Restore original source before mutation; never reuse coverage
overlays, stale outcomes or incremental-only results. Preserve every generated
status. Source-safety refusals, compile failures, timeouts and infrastructure
errors are separate from behavioral assertion kills.

Open gaps include OS/token/ACL and IO/thread errors, response validation,
shutdown/download races, file type/growth, owner-input alternatives, online
keyring paths, normal GUI callsites, early exits, hard kills and post-flush code.
A directory alternate-stream experiment did not establish a non-file witness;
the file-type guard remains open. The restricted-token case passes in the
actual security23 suite and is included in the current LLVM collection. Other
error paths remain uncovered.

Narrow contract assumptions remain separate from coverage. Safe parsed JSON
cannot supply nonfinite statistics. The reviewed native stop path normally
returns `Ok`; this does not close an unobserved error branch. For conforming
Windows APIs, the pinned ABI and no memory corruption, `secure_files.rs:166`
cannot report sizing-query success with a null, zero-byte TokenUser buffer;
line 169 cannot report a required size smaller than `TOKEN_USER`. Other token
errors remain open. See the
[GetTokenInformation contract](https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-gettokeninformation).
Missing profiles or LLVM-inferred post-flush counts do not prove execution.
These bounded results establish neither complete feasible-branch closure nor
full correctness. The 396-mutant full native run is outside the accepted scope;
no passing result or score is claimed. The restricted entrypoint recipe completes
one fresh library and binary build, then refuses the executable's shared file
identity before native launch. It remains unqualified: one of five builds, zero
of 15 lifecycle observations and zero of 18 launch admissions. It establishes
no semantic RED or native routing pass. Exact source restoration and owned-process
cleanup pass; the run is retained without a retry or helper change. The core
260-configuration application check and 250-configuration CLI replay pass.
Complete auxiliary-tool mutation runs are outside the accepted scope; no such
mutation score is claimed. The separate packaged CPU inference result in the
[CLI verification record](../evidence/agent-cli-180.md) does not supply these native
measurement results.
