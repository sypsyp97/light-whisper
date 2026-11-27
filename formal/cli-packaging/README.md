# CLI artifact delivery verification

The PyInstaller and CArchive results below describe the 1.8.0 CLI. Since 1.8.2,
the build packages a Rust executable. See the [development guide](../../docs/development.md)
for the current build and verification commands.

The implemented `scripts/build_cli.py` verifies embedded CLI code and metadata,
then publishes a completed build atomically. `build_support::select_cli_executable`
provides the Cargo resource selection contract. Their initial declaration-only
RED was interface evidence, not a pre-existing packaging defect.

Owned cohort: `verify_cli`, `build_cli`, `main`, and
`select_cli_executable`, plus their existing Cargo/Tauri, distribution and release
entrypoints. No engine packaging, CLI transport, CI, installed GUI, models,
provider calls, dependency installation, commit or publication is exercised.

## Hidden state and checked promises

| Resource / transition | Actual public seam and deterministic schedule | Model / configured property |
| --- | --- | --- |
| Default/custom artifact, actual CArchive and embedded metadata | `verify_cli(path=None)` reads real `CArchiveWriter` output through the installed reader; missing/empty/non-PE/corrupt/missing members, stale code, optimize 1/2, exact metadata and version | `Finish`, `Verify`, `OnlyVerifiedPublication`; verification negative removes admission guard |
| Nested compiled-code filenames | real writer fixture changes root/nested `co_filename`; actual verifier must reject both | `Verify` abstracts all artifact integrity predicates; byte/code/filename details remain Python obligations |
| Old artifact, owned staging directory, unrelated sibling | `build_cli()` invokes a controlled PyInstaller subprocess boundary that writes a real CArchive; observe actual verifier and actual `os.replace` | `Start`, `Finish`, `Publish`, `OnlyVerifiedPublication`; early-publication negative |
| Original input bytes versus current inputs | source/package changes during subprocess, including a staged archive matching the new inputs; source changes after actual verification | `ChangeInputs`, `InputFailure`, `OriginalInputsRequired`; stale-guard negative |
| Build/verify/replace failures and cleanup | subprocess rejection, incomplete archive and denied atomic replacement preserve old bytes and sibling state; owned stage disappears | `BuildFailure`, `Verify`, `ReplaceFailure`, `Cleanup`, `UnrelatedSurvives`, `SettledOwnStageReleased`; separate cleanup/sibling negatives |
| Debug/release selection and filesystem shape | std-only harness imports actual `build_support.rs`; missing/empty/full/directory, create I/O failure, engine sibling preserved | `Select`, `ReleaseRequiresNonempty`, `ExistingArtifactPreserved`, `SelectionTruth`; release/debug negatives |
| Metadata failure before debug/release fallback | actual `fs::metadata` on an owned NUL-containing child path returns Windows `InvalidInput`; exact returned metadata context and preserved CLI/sibling bytes in both modes | `Select`, `MetadataFailurePreserved`; `MetadataNegative` bypasses the NotFound-only guard; `MetadataWitness` reaches rejection |
| Finite local completion | eventual return and stage release under stated local-step fairness | `EventuallySettled`; fairness-removal progress negative |
| Public build/verify command | real `python scripts/build_cli.py --verify PATH` subprocess, module `main([])` build boundary, stdout JSON and error exit/stderr | interface/output details outside the finite lifecycle model |
| Distribution and bundle integration | execute the actual `package.json` distribution command in Bash with external commands controlled; CLI verification failure prevents frontend build; source/config assertions supplement Cargo/Tauri/release binding | selector/source correspondence must be reviewed on the final production tree; no NSIS/runtime claim |

Synthetic fixtures contain a real PyInstaller CArchive after a synthetic PE
header. They prove archive acceptance/rejection and build ownership contracts;
they cannot run as executables. The success verifier is never mocked. Build tests
replace only the external subprocess boundary; the post-verification mutation
wrapper first calls the actual verifier. A separate real PyInstaller build and
frozen CLI recipe belongs to the parent integration work.

## Finite model bounds and assumptions

One build owner, two input revisions, good/bad/partial/no staged artifact, two
selector modes and five initial artifact classes: the original missing, empty,
full and directory classes plus `metadata-nul-invalid-input`: an embedded-NUL
path rejected by Windows path conversion before WinAPI. Input changes may occur
during build, after subprocess completion and after verification. Publication is an
atomic `os.replace`; failure preserves the old destination. The model assumes
source changes stop at publication, successful filesystem cleanup, and weak
fairness for finite local transitions. It does not prove crash durability,
arbitrary concurrent builders, arbitrary post-check external writers, antivirus
interference, Python/native refinement or installer placement at runtime.

The positive configuration checks every listed safety invariant plus
`EventuallySettled`, including the fifth input class in both selector modes.
The original seven safety negatives and five witnesses remain; a metadata guard
negative and metadata rejection witness extend the matrix from fourteen to
sixteen configurations. The progress negative still removes local fairness.
Witnesses are reachability evidence and are not counted as safety mutation kills.

`Select` records the returned error stage and cause. For the NUL-containing
path, metadata fails with `InvalidInput`; selection returns
`metadata/invalid-input` before either fallback, keeps the artifact unchanged
and preserves sibling state. `MetadataFailurePreserved` checks that
classification and both resource facts as well as rejection. Its negative
bypasses the real `failure.kind() == NotFound` guard: debug reaches
`create/invalid-input`, while release reaches `selection/missing-or-empty`.
The four original input classes retain their prior success and artifact effects;
all build bounds and transitions remain unchanged.

The new Rust fixture is a synthetic invalid path beneath its owned Scratch,
not a file containing a NUL. It reads the actual standard-library metadata
failure and derives the exact expected public error context from it; fixture
CLI and sibling bytes survive both calls. The fifth model input represents
only this NUL `InvalidInput` partition. Other non-NotFound production metadata
errors also return through the unconditional error arm, but their possible
debug fallback behavior is outside this bounded partition. The model abstracts
OS error text and assumes no external filesystem writer. `SkipMetadataGuard`
is bound to the `failure.kind() == ErrorKind::NotFound` match guard in
`src-tauri/build_support.rs`: the following error arm returns the metadata
failure, while bypassing that guard reaches `OpenOptions::open` in debug or
the missing/empty return in release. `selectorError` classifies those source
return paths; it is checked against the original input, not rejection alone.
The standard-library metadata and open paths share NUL conversion rejection
before WinAPI, so their debug diagnostics can coincide. Only the release
missing/empty diagnostic distinguishes the generated guard control through
the public error result. Exact Windows behavior, compilation, source-control
RED/GREEN and the extended TLC outcomes remain pending until the parent grants
their bounded validation runs.

## Historical RED and model evidence

From this slice's test worktree, with the project's existing Python 3.11.11 and
PyInstaller 6.19.0:

```powershell
python -m py_compile scripts/test_build_cli.py
python scripts/test_build_cli.py
rustc --edition=2021 --test formal/cli-packaging/selector_tests.rs -o ../selector-tests.exe
../selector-tests.exe --test-threads=1
python formal/cli-packaging/run_models.py --java $java8 --jar $reviewedTlaJar --output ../formal-red-20261114
```

Python compile passed. Python RED: 26 tests, 30 assertion failures (subtests
included), zero errors; the independent real archive fixture passes. Rust
compiled with only three unused existing engine-selector warnings; seven tests
fail against the declared pending selector. All are assertion/panic failures
after invoking the available public interfaces, not syntax/collection failures.

SANY passed. All fourteen TLC configurations produced the expected outcome.
Positive: 824 generated states, 464 distinct states, exhaustive completion and
checked liveness. The seven safety controls fail the intended invariant; the
progress control fails the temporal property; all five witnesses reach their
target. Parent-local `formal-red-20261114/receipt.json` binds exact source,
model, config, runner and pinned JAR hashes. `python-red.log` retains Python RED.

The runner requires the reviewed TLA+ JAR SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`,
uses the explicitly supplied Java executable, 512 MiB and one worker, and writes
logs plus receipt beneath a new caller-owned output directory. The receipt's
production identity in that historical receipt is the declaration scaffold.
Current packager tests, real frozen CLI execution, source bindings and remaining
coverage/mutation/installer gates are recorded in the
[1.8.0 verification record](../evidence/agent-cli-180.md).
