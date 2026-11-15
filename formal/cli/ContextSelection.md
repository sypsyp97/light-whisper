# Conservative context selection contract

This is a measurement-only optimization for the unchanged generated mutation
worklists. It changes test selection, not application source, mutant generation,
the SDK constructor partition, or outcome classification. Final execution still
accounts for all 1,438 in-scope framework items and ends with a full regression.

## Pure selector API

`select_context_tests.select_tests(dataset, current_sources, current_bindings,
full_test_ids)` returns `tests`, `full` and a nonempty `reason`. `tests` preserves
the current complete cohort order, with no duplicates. `full` distinguishes a
fallback from a trusted context selection. An empty or duplicated current cohort
raises `ValueError`; it must never become a passing empty run.

The schema-1 dataset contains:

- `complete`: true only after successful full collection and test execution;
- `tests`: the exact ordered complete observed test IDs;
- `sources`: all four frozen production paths, each with original source text,
  its SHA256, and executable-line-to-context-ID lists;
- `bindings`: all non-mutated test, configuration, helper and module-dependency
  file hashes, plus interpreter, OS, tool/dependency versions and environment
  fingerprints.

Selective execution requires all bindings and complete ordered IDs to match,
valid baseline source hashes, exactly the four expected source files, exactly
one changed file, and exactly one same-line-count single-line replacement. The
changed original line must lie wholly in a function body, with valid original
and current Python syntax. Function signatures/defaults, module/class-level
initialization, imports, insertion/deletion, multiline or multiple hunks use the
full cohort. This includes nested class initialization, declaration decorators,
continuation-line defaults, imports inside functions and a one-line function
containing both its header and body. Use the innermost lexical class/function
owner so an outer callable does not make a nested class body eligible. Validate the baseline
SHA256 of every source, including unchanged files. Missing, empty, unknown or outside-test contexts use the full cohort.
Otherwise select every covering ID, once, in complete-cohort order.

## Planned collector and runtime wiring

Use coverage.py's existing current measurement instance and an owned unittest
result hook. Before each test, switch the parent coverage context to its real
`test.id()` and set `LIGHT_WHISPER_COVERAGE_CONTEXT` for inheriting children. After
the test, switch to `<outside-test>` and restore the prior environment value.
The existing owned startup hook calls `coverage.process_startup()`, then switches
the child to its inherited identity or `<outside-test>`. Module/class fixtures
and other unattributed execution therefore force conservative full fallback.

Collect the complete maintained CLI/packager/measurement suite, including the new
context contracts, plus every maintained resource test. Exclude only the static
CI-wiring tests already outside this Python implementation cohort. Derive real
IDs and counts from collection, reject duplicate IDs and loader errors, and keep
all tests observed in this run. The existing wrapper's smaller default selector
does not define the context run's fallback cohort. Do not edit its accepted
assertions; the new test-command runner loads the complete observed IDs.

Fingerprint all local Python callers, helpers, test files and module dependencies
used by the complete suites, not just the four mutation targets. A small tracked
input inventory can cover `scripts/**/*.py`, `src-tauri/resources/**/*.py`, owning
formal/verification helpers, required JSON fixtures/configuration, project package
metadata and dependency locks. Exclude ignored multi-gigabyte native binaries
that these synthetic Python tests do not load. Recompute these input fingerprints
when selecting tests; a dataset file's own hash is not evidence that its claimed
inputs still match. Record actual project Python, coverage/Cosmic
Ray and application dependency versions from the existing local environments.
Bind effective runtime environment values using hashes, without recording
credentials or other raw private environment values. Normalize only owned
measurement/output/context settings that necessarily differ between collection
and mutation execution; bind their effective application import environment.
Check fingerprints before and after collection. A changed, incomplete or failing
run cannot create a trusted dataset. Preserve original fixture-copy coverage
datasets and existing source-path aliases; verify copies contain frozen bytes.

The selection command records the raw dataset-file SHA256, actual current source
hashes, complete/selected test IDs, selection reason and exact equivalent unittest
command before executing tests. Each framework invocation keeps that record in
the wrapper's existing disk logs. Current test/config/helper/dependency drift or
an unavailable/untrusted dataset chooses the complete current collected cohort.
Context data is never silently reused across a changed binding.

Cosmic Ray 8.9's local distributor exposes no job-ID environment variable. Inspect
current source against the recorded baseline bytes; do not invent a job-ID seam,
custom mutation generator or replacement distributor. Reuse the existing wrapper
`--` command interface and exact Job containment. Configuration changes only the
test command after acceptance and a fresh full baseline. Per-invocation timeouts,
tool errors, invalid source and assertion failures remain distinct outcomes.

The implemented selector/startup and collector safety contracts pass 35 lightweight
tests. Four disposable helper controls expose changed-source-only hashing, missing
boundary checks, first-covering-ID-only selection and ignored input bindings.
These controls are separate from the untouched 1,438 framework work items. Actual
discovery found 536 unique IDs: 197 CLI/packager/measurement cases and 339 resource
cases. The ordered list is retained in the external authoring evidence; collection
does not run any test or model inference. Full context data, real collector command
execution, full mutation baselines and framework execution remain pending their
resource slot. Existing tests, wrapper behavior and application source are unchanged.

## Reproducible commands after the heavy slot is granted

Run from the frozen disposable checkout. Set `$projectPython` to the existing
project Python 3.11 executable, `$toolScripts` to the task-local verification
environment's `Scripts` directory, and `$toolSite` to its `Lib/site-packages`.
Choose a fresh absolute `$evidence` path outside the checkout; do not pre-create
that final directory. No tool or dependency installation is needed.
Collect and execute from the same frozen checkout and effective environment;
moving the run changes its binding and must conservatively fall back or recollect.
The collector owns creation of this fresh directory before constructing or starting
parent coverage. It rejects any existing file/directory and any already-active
parent coverage instance. The startup hook stays on `PYTHONPATH`, but the caller
must disable parent startup coverage; the collector enables it for children before
loading the cohort. Failed collection stops the owned coverage instance and cannot
write a trusted dataset or success receipt. Retain failed evidence and choose a new
output path for a later attempt.

```powershell
$root = (Get-Location).Path
$env:PYTHONDONTWRITEBYTECODE = '1'
$env:PATH = "$(Split-Path $projectPython);$env:PATH"
$env:LIGHT_WHISPER_VERIFICATION_SITE_PACKAGES = $toolSite
$env:PYTHONPATH = "$(Join-Path $root 'formal/cli/coverage_startup');$toolSite;$(Join-Path $root 'src-tauri/resources')"
Remove-Item Env:COVERAGE_PROCESS_START, Env:COVERAGE_FILE -ErrorAction SilentlyContinue
$env:COVERAGE_CORE = 'ctrace'
$env:OMP_NUM_THREADS = '2'
$env:MKL_NUM_THREADS = '2'
$env:OPENBLAS_NUM_THREADS = '2'
$env:HF_HUB_OFFLINE = '1'
$env:TRANSFORMERS_OFFLINE = '1'
$env:HF_HUB_DISABLE_SYMLINKS_WARNING = '1'
$env:HF_HUB_ETAG_TIMEOUT = '10'
& $projectPython formal/cli/context_measurements.py $evidence
```

After creating the directory, the collector constructs coverage with the absolute
`formal/cli/coverage.ini` and owned `$evidence/.coverage` paths, starts coverage and
switches to `<outside-test>`. It then sets the absolute child
`COVERAGE_PROCESS_START` and owned `COVERAGE_FILE` before loading any suite or child.
It keeps all parallel datasets and combines
the established source-copy aliases, and writes `unittest.log`, `coverage.json`,
`coverage.txt`, `context-dataset.json` and `receipt.json`. It refuses a trusted
dataset on test failure, source/input drift, duplicate/empty IDs, missing files
or missing branch/context instrumentation. All four whole-module denominators
remain in the reports. Derive constructor-only counts separately.

After inspecting the successful receipt and actual source/coverage identities,
bind the accepted raw dataset hash and clear startup instrumentation:

```powershell
$env:LIGHT_WHISPER_CONTEXT_DATASET = Join-Path $evidence 'context-dataset.json'
$env:LIGHT_WHISPER_CONTEXT_SHA256 = (Get-FileHash $env:LIGHT_WHISPER_CONTEXT_DATASET -Algorithm SHA256).Hash.ToLowerInvariant()
Remove-Item Env:COVERAGE_PROCESS_START, Env:COVERAGE_CORE, Env:COVERAGE_FILE -ErrorAction SilentlyContinue
$env:PYTHONPATH = Join-Path $root 'src-tauri/resources'
& (Join-Path $toolScripts 'cosmic-ray.exe') baseline formal/cli/cosmic-ray.toml --session-file "$evidence/cli-baseline.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') baseline formal/cli/cosmic-ray-build.toml --session-file "$evidence/build-baseline.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') baseline formal/cli/cosmic-ray-sdk.toml --session-file "$evidence/sdk-baseline.sqlite"
```

All three unmutated baselines deliberately run the complete observed cohort.
Only after their review, generate fresh non-incremental worklists and retain
complete dumps. Before SDK execution, partition the whole generated SDK session
with the existing `select_sdk_mutants.py`: 181 constructor items are in scope and
798 remain explicitly outside, assuming the unchanged generation identities.
CLI and build sessions retain every generated item.

```powershell
& (Join-Path $toolScripts 'cosmic-ray.exe') init formal/cli/cosmic-ray.toml "$evidence/cli-session.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') init formal/cli/cosmic-ray-build.toml "$evidence/build-session.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') init formal/cli/cosmic-ray-sdk.toml "$evidence/sdk-all.sqlite"
& (Join-Path $toolScripts 'python.exe') formal/cli/select_sdk_mutants.py "$evidence/sdk-all.sqlite" "$evidence/sdk-session.sqlite" "$evidence/sdk-partition.json"
& (Join-Path $toolScripts 'cosmic-ray.exe') exec formal/cli/cosmic-ray.toml "$evidence/cli-session.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') exec formal/cli/cosmic-ray-build.toml "$evidence/build-session.sqlite"
& (Join-Path $toolScripts 'cosmic-ray.exe') exec formal/cli/cosmic-ray-sdk.toml "$evidence/sdk-session.sqlite"
```

Do not execute these framework commands before that phase is granted. Raw wrapper
receipts/stdout/stderr now go outside the checkout, under its parent
`python-mutation-logs/{cli,build,sdk}`. Each test-command log starts with selected
and complete ordered IDs, source hashes, accepted dataset hash, reason and exact
equivalent unittest command. Inspect assertion failures separately from loader,
source-invalid, timeout and tooling errors.

The provisional outer collector budget is 180 seconds, one process and two native
library threads. Prior instrumented application/resource suites took about 73
seconds combined; the new helper checks add little execution time but fingerprint
and collection overhead must be measured. All mutation selections use a 120-second
wrapper budget and 135-second framework budget because any item may conservatively
fall back to the complete cohort. These are provisional caps, not measured fresh
baselines or a promised total mutation runtime. Estimate total work from the fresh
per-test timings and each framework item's selected IDs before full execution.

For valid cumulative-delay controls, keep all application source bytes unchanged
and use an external test-only runner that delays `unittest.TextTestResult.startTest`
by 0.01 and 0.04 seconds before calling the original method, then runs
`select_context_tests.py` with `runpy.run_path(..., run_name='__main__')`. Invoke it
through the unchanged bounded wrapper's `--` interface. On unmutated source each
variant runs every observed ID; 536 cases add roughly 5.4 and 21.4 seconds before
minor nested-result overhead. No application deadline has started at that boundary.
Require all tests to remain GREEN within the measured budget; preserve exact delay
script hash, IDs, elapsed time and wrapper receipts. These timing probes are not
framework assertion kills. No runtime inference is part of this recipe.

Set `$delayControl` to the external authoring artifact `cumulative_delay.py`, whose
hash is retained with the evidence. After the heavy slot is granted, use:

```powershell
& $projectPython formal/cli/run_mutation_tests.py --timeout 120 --logs "$evidence/delay-001" -- $projectPython $delayControl 0.01 (Join-Path $root 'formal/cli/select_context_tests.py')
& $projectPython formal/cli/run_mutation_tests.py --timeout 120 --logs "$evidence/delay-004" -- $projectPython $delayControl 0.04 (Join-Path $root 'formal/cli/select_context_tests.py')
```

The control was compiled during authoring; neither delayed full run has yet been
executed. Their actual GREEN results and timings remain required evidence.
