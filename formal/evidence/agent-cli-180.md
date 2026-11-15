# CLI 1.8.0 verification record

Status: local product checks completed on 2026-11-15. This record does not
authorize a release. Core formal checks, product regressions, artifact checks and
CPU inference pass. The supplemental entrypoint check remains unqualified, as
recorded below. Publication checks and CI apply to the actual PR commit.
Raw logs and measurement databases stay outside the source tree.

## Supported contract

The CLI connects to the desktop instance or starts an independent local engine.
The default target is `app`. Explicit engine, runtime or data-directory options
select `engine`. Invalid option combinations fail before a child starts.

Desktop commands provide engine selection, start, restart, status, statistics,
model checks, device enumeration and file transcription. Saved selection and
observed runtime identity are separate fields. The interface excludes arbitrary
application commands, credentials, microphone activation and desktop termination.

Independent runs use the saved local engine unless an explicit option overrides
it. They use offline model access and owned runtime state. Both targets support
serial JSONL sessions and integer request IDs. A lost connection or an unknown
operation outcome ends the session without retry. Closing a desktop CLI
connection leaves the desktop application in operation.

See the [CLI guide](../../docs/cli.md) for commands, limits and exit codes.

## Test-first findings

- Saved-selection tests exposed the incorrect Qwen default for a saved R2T2
  selection. Tests also cover explicit overrides and invalid target combinations.
- Request tests exposed silent replacement of escaped unpaired Unicode
  characters. The CLI now escapes request strings before native transport.
- Desktop input tests reproduce rejected oversized or invalid Unicode requests.
  The CLI checks scalar text and the complete encoded message before connection
  acquisition or transmission. Session input errors remain recoverable.
- Response tests reject nonfinite numbers and distinguish recoverable operation
  failures from transport failures that require session shutdown.
- Settings tests exercise listener acknowledgement, failed registration, late
  cleanup, overlapping reads, external changes and StrictMode replay.
- Native admission tests reproduce recording or configuration changes while an
  engine-control request waits. The controller checks state after lifecycle
  acquisition and retains ownership through the protected operation.
- Private-file and transport tests cover authentication, bounded messages,
  deadlines, atomic publication, admission, shutdown and owned audio cleanup.

## Current verification

| Check | Result | Boundary |
| --- | --- | --- |
| Application formal contracts | All 260 configured outcomes pass, with the recorded inventory, bindings, 67-action reachability and Lean 4.34.1 checks. | 79 unchanged positive proofs are reused by exact identity; one positive and all 180 negative/witness cases run fresh. |
| CLI companion models | All 250 configurations in the affected nine-driver replay pass on current CLI source. | Other unchanged matrices retain their original bounded evidence; native admission is reported separately. |
| Combined settings transitions | The oversized combined model is outside the accepted verification scope. | Hook regression tests and the separate settings and subscription models are retained. No combined formal qualification is claimed. |
| Mounted settings model | All 18 configurations pass. | One positive, 12 safety controls, one progress control and four witnesses. |
| Verus implementation checks | Three positive programs verify 13 obligations. Eleven negative controls fail as expected. | Six functions and seven constant initializers; this does not verify the whole application. |
| Public settings hooks | All 135 tests pass. Eleven recorded source controls give the expected outcomes. TypeScript and ESLint pass. | Current application callers and lifecycle assumptions; two equivalent controls are not assertion kills. |
| Frontend build | All 976 regression tests, the build, bundle budgets and separate performance test pass. | The final regression uses the main integration checkout and its normal dependencies. |
| Formal-checker changes | All 40 tests pass. All 42 identified gap controls produce their intended assertion failures. Selected coverage is 78/78 statements and 40/40 arcs. | The complete 443-mutant auxiliary-tool run is outside the accepted scope and was not executed. Earlier 30-test mutation results remain historical. |
| Python CLI and packaging | All 606 current cohort tests pass with zero skips, errors or failures. CLI coverage is 411/411 executable lines and 162/162 branches; packaging coverage is 77/77 lines and 20/20 branches. | Coverage and the completed 1,531-mutant run retain their original 599-test binding. Three mutation baselines and the original final 599-test regression pass. Later test-only additions are checked separately. |
| Native engine-control queries | All 34 tests pass; two fixtures remain ignored. | Actual windowless Wry application state; no microphone or normal GUI interaction. |
| Native admission model | All 32 configurations produce their expected semantic outcomes. | One positive, 16 safety controls, one progress control and 14 witnesses; witnesses are not mutation kills. |
| Native measurements | All five current suites pass. LLVM collection records 273/326 raw branch outcomes in the declared spans; independent receipt review accepts the counts and identities. | 53 raw outcomes remain uncovered. The 396-mutant full native run is outside the accepted scope and is not executed. The supplemental entrypoint recipe stops at artifact admission before native launch. Auxiliary helper mutations are outside the accepted scope. |
| Engine, frozen CLI, desktop and installer | Builds, source/version checks, archive integrity and the current installer identity/privacy audit pass. | The artifact identities and inspection bounds are recorded below; no installed-app qualification. |
| Actual packaged CPU inference | Qwen and R2T2 pass through both the standalone frozen CLI and windowless Wry desktop host: four exact synthetic transcriptions. | No CUDA inference, installed-app interaction or hard-kill parent-watch claim. |

The frontend regression found missing open-source wording in the English README
and a tray test tied to the previous cleanup location. The README now states its
license clearly. The repaired tray contract checks the unified exit callback:
drain admitted agent work, then stop the engine. Six negative controls detect
broken cleanup ordering or missing calls. The final regression passes.

The whole-hook V8 report records 205/205 lines, 248/248 statements, 154/154
branches and 41/41 functions. The complete Stryker report records 457 mutants:
424 killed, 32 survived and one timeout. Its official score is 93.00.
The raw report SHA256 is
`88bd868f2aac42fa60344dbec40b9968d284928b07f97f1619b54a708b422e79`.

Independent review accepts bounded equivalence arguments for the 33 unresolved
outcomes. Two survivors also produce TypeScript errors. Compiler rejection and
behavioral equivalence remain separate observations. The timed-out variant
passes all 135 tests when replayed. This does not change its raw timeout status
or count it as an assertion kill. No adjusted mutation score is reported.

The affected SDK constructors cover 61/61 executable lines and 22/22 branches.
The whole SDK files retain separate coverage gaps. Across all four Python source
files, the raw totals are 964/980 executable lines and 330/344 branches. The fresh
generators produce 2,329 mutants: 979 SDK mutants, 232 packaging mutants and
1,118 CLI mutants. The frozen affected scope selects 181 SDK constructor mutants,
all 232 packaging mutants and all 1,118 CLI mutants. The other 798 SDK mutants
are outside that scope; they are not executed or counted as detected.

The closed SDK phase records 169 raw framework kills and 12 survivors. Review
separates 142 assertion detections, seven constructor exceptions on valid
80/2000 ms inputs, and 20 invalid generated syntax variants that execute no
tests. The invalid variants are not behavioral detections. All 12 survivors are
bounded equivalents for the current two engine families, canonical modes,
ordinary input types and CPython 3.11.11 runtime. This excludes arbitrary
metaclasses, modified OS facades and private partial-object observation. No
adjusted mutation score is reported.

The closed packaging phase records 215 raw framework kills and 17 survivors.
All 17 are bounded equivalents for the inspected Windows build commands,
nonoptimized Python runtime, fixed checkout paths and import names. Review
separates 62 assertion detections, 70 builder initialization errors, 80 builder
runtime errors, two missing-output detections and one incidental timing error.
That incidental run failed before it reached the builder. An unchanged existing
builder test then passes on original source and fails at the exact mutated
initialization expression. Both receipts are retained; the original framework
result is not rewritten. No adjusted score is reported.

The closed CLI phase records 1,059 raw framework kills and 59 survivors.
Independent review identifies 54 bounded equivalents. Three other variants expose
test gaps for explicit desktop target arguments and the independent engine's
larger context. Three added tests detect those exact variants by assertion
failure and pass on the unchanged implementation. The original seven admission
tests and all their helpers remain unchanged. Two daemon variants are detected
by existing process-exit tests that the context-selected subset omitted. Their
separate baseline and mutant replays pass and fail as expected. All five remain
SURVIVED in the original framework report; no adjusted score is reported.

The raw CLI logs contain 659 assertion-only failures, one mixed failure/error,
260 error-only runs, 59 successful runs and 139 runs without a unittest summary.
Those 139 comprise 21 watchdog timeouts, 56 invalid generated syntax variants,
57 product import/initialization errors and five mutated entry guards that exit
before tests. Invalid syntax, early exits and watchdogs are not assertion kills.
All 21 watchdog variants have separate causal evidence: 20 typed runtime-error
detections and one fake-clock assertion detection. Original timeout statuses
remain unchanged. All 1,531 owned mutation runs have clean process teardown.

The final full regression collects 606 unique tests: the original 599, three new
admission tests and four existing formal-registration tests outside the frozen
mutation cohort. All 606 pass in 60.010 seconds. The first assembled attempt
stops at a mocked download-cache test because the long temporary path prevents
Windows from creating a nested file. The unchanged test reproduces that failure
and passes with a short owned temporary directory; the full short-path replay
then passes. Both attempts are retained. No test, assertion or product change
is used to remove that environment failure. Python product and model sources remain
unchanged after the original mutation run; no fresh 606-test mutation run is
claimed.

The initial Python collector, official baselines and early SDK mutations used
two native threads but did not inherit the intended OS affinity and priority.
The owned launcher and descendants were then set to two logical CPUs and
BelowNormal priority; newly created children were observed to inherit both.
Earlier receipts retain their actual conditions. These functional results are
not a uniform performance benchmark.

The CI compatibility repairs normalize expected fixture paths and mocked tool
identities, preserving
the single-read, ownership and cleanup assertions. Actual Windows short-path
aliases reproduce the old failures and pass after repair. Rust changes are
limited to test callback types, a test lock scope, isolated test directories and a
narrowly expected unused function in the archive test module. Normal-build execution paths stay unchanged.
The assembled checks pass again: 278 CLI tests, the full 606-test cohort,
26 measurement-helper tests with canonical TEMP, 32 entrypoint-helper tests and
three checkout tests.
The six required CLI CI unit commands also pass with actual Windows short-path
TEMP: 278 CLI, 32 entrypoint, two admission-runner, 15 numeric-runner, 21 release-CI
and two fail-fast tests. Mocked runner replies are not SANY or TLC evidence.
The refreshed local Rust suites pass 23 security, 35 transport, eight selector
and six archive tests. Required whole-application Rust checks run in PR CI.

A later parallel CI run reports three native query test failures. Its fixture
used only a process ID and timestamp for directory names and accepted existing
directories. Actual Windows clock samples repeat that candidate. Exact-source
probes with a fixed clock fail uniqueness, independent cleanup and preservation
of an existing candidate before repair; all three pass after repair. The fixture
now uses a process-local counter and exclusive directory creation before taking
cleanup ownership. All 257 assertions, 34 cases and two ignored fixture bodies
remain unchanged. The original CI collision remains an inference because that
run did not record allocated root identities. Full native results require the
new commit's CI; the allocator probes are not a full application run.

## Recorded artifacts and actual inference

These artifact and LLVM receipts retain their inputs from before the test-only
Rust CI repairs. The CLI implementation stays unchanged. No installer is rebuilt
or requalified for the later source snapshot by these earlier receipts.

| Artifact | SHA256 |
| --- | --- |
| Built desktop application | `3cfd491c349e2f06ae836a26fa33756e3a05f08a4d1aaa3a145c6ea71fdcc64f` |
| Desktop application extracted from the installer | `b31817fd7f38046e5514cc4dcae73cdc82b8d6d0744028772e9541b98bfaefe9` |
| NSIS installer | `7b4f0621fe5e4c52d4b3242a2eeab769aa76e43b6afe2925e4e785ba89f67f94` |
| Frozen CLI | `e7fedc622ba8fb48d66d6e80892309e73f75e6b6873a0f22e760a15f42a82acf` |
| Engine archive | `bea2ecd61949d0228044e103a8636d6f40eab8b5ef2d3d1333b29e9a35d55417` |

The built and extracted applications differ only in the expected NSIS marker;
all other bytes match. The recorded audit verifies all 19 configured resources,
11 build/source pins, x64 architecture, freshness, archive integrity and extracted
CLI source/version. It freshly scans 28 raw files and 35 text files, including
18 frontend files, with zero configured-private-prefix hits or scan errors.
All 77 frontend asset keys are present. This is a scoped privacy check, not a
general proof that arbitrary secrets cannot occur.

The earlier deep inspection of 33,740 compressed Python code objects is retained
only through exact unchanged CLI, engine-archive and resource identities. The
recorded audit walks zero new code objects and does not decode CUDA fatbins.
Earlier failed artifact receipts remain historical; they do not qualify the
current application or installer.

The second actual-artifact probe uses the CLI, engine archive and extracted
application from this same installer payload. All four CPU transcriptions match
the expected synthetic text, with 28 responses. The standalone R2T2 case omits
`--engine` and observes the isolated saved R2T2 selection. All six owned process
handles are dead after natural cleanup, the runtime directory is removed and
there are no cleanup failures. No model download or normal GUI interaction is
part of this result.

The recorded admission positive generates 51,448 states, finds 20,476 distinct
states and completes at depth 19 with an empty queue. The later test-only Rust
repairs refresh three of the 17 binding hashes; state guards and model sources
stay unchanged. The earlier 28-case receipt remains historical. The 32-case result
does not depend on the changed Python CLI source.

## PR checks and remaining limits

The supplemental entrypoint recipe completes its first fresh library and binary
build. Its artifact gate then refuses the executable's shared file identity
before native launch. The result is unqualified: one of five builds, zero of
15 lifecycle observations and zero of 18 launch admissions. No semantic RED or
native pass is claimed. The run restores all 21 source pins in each of the source,
mirror and ownership records, removes its targets and leaves no owned processes.
This auxiliary refusal is retained without another build or tool repair.

The PR must retain the source-only privacy review and the exact commit's required
CI results. Those remote statuses are reported by the PR rather than embedded
in a later source commit.

The accepted scope excludes the oversized combined settings model, the unstarted
396-mutant full native run and complete auxiliary-tool mutation runs. Their plans and raw
evidence are preserved outside the source tree. They are not passing results.
The current entrypoint helper passes all 32 tests; selected coverage is 73/73
lines and 24/24 arcs. No complete auxiliary mutation score is claimed.

An extra measurement-helper run with Windows short-path TEMP reports nine failed
subtests and three errors in LLVM profile-template path checks. Its earlier
26-test canonical-path result remains separate. The short-path result is not a
pass and is outside the accepted auxiliary-tool repair scope. These helpers are
not part of the required CLI CI unit step.

No model-file listing or successful `doctor` report counts as inference.
An interrupted model check, timeout, build failure or uncertain cleanup does
not count as a successful verification result.

## Reproduction and limits

Use the maintained commands in [development](../../docs/development.md),
[CLI verification](../cli/README.md),
[native measurements](../agent-api-measurements/README.md) and
[formal verification](../README.md). The native cohort manifest records source
and test identities. Each measurement must retain its actual tool versions,
inputs, raw results, environment and owned-process cleanup receipt.

Finite models establish their stated invariants and progress properties within
their recorded domains. Source correspondence checks connect these models to
the implementation; they do not establish complete refinement. Branch coverage
does not establish all condition combinations. Witness configurations establish
reachability and are not mutation kills. Raw framework errors, assertion kills,
survivors, refusals and timeouts must remain distinct.
