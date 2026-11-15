# Python CLI measurement cohort

This recipe replaces the old two-class CLI measurement selector. Keep raw logs,
coverage datasets, Cosmic Ray databases and generated mutant dumps in an external
task evidence directory. Run only in a disposable, frozen worktree, one mutation
worker at a time. Derive runtime budgets from the matching unmutated baseline.

## Frozen production denominator

| Source | Coverage and source mutation scope |
| --- | --- |
| `scripts/light_whisper_cli.py` | Entire module, including script entrypoint, bootstrap, configuration, session, routing, transport, deadlines and cleanup. |
| `scripts/build_cli.py` | Entire module, including script entrypoint, archive verification and staged build publication. |
| `src-tauri/resources/r2t2_native.py` | Whole-module coverage baseline; affected cohort is the complete `NativeRuntime.__init__`, including existing validation, DLL handles, native options, session allocation and exception cleanup. |
| `src-tauri/resources/qwen3_native.py` | Whole-module coverage baseline; affected cohort is the complete `QwenNativeRuntime.__init__`, including forwarding to the parent constructor. |

Other SDK functions remain outside the derived affected cohort. Preserve their
whole-module raw coverage instead of presenting constructor coverage as SDK-wide
coverage. Confirm source identity and the unchanged out-of-cohort functions from
the reviewed task baseline before reusing any receipt.

The application test modules are:

```text
scripts.test_light_whisper_cli
scripts.test_cli_bootstrap
scripts.test_cli_configuration
scripts.test_cli_session
scripts.test_cli_target
scripts.test_cli_native_threads
scripts.test_cli_commands
scripts.test_cli_json_numbers
scripts.test_build_cli
scripts.test_cli_measurement_gaps
```

Constructor regression selection additionally includes:

```text
test_native_thread_budget
test_native_constructor_gaps
test_qwen3_native.QwenNativeRuntimeTests
test_qwen_cpu_cuda_backends.CpuCudaAbiTests
test_r2t2_finish_context.FinishContextTests
test_r2t2_startup.NativeStartupTests
```

`scripts.test_cli_measurements` checks the verification runner and belongs in its
baseline, outside the application coverage denominator. The default runner selects
all the above tests with fail-fast enabled. Coverage also runs the complete
maintained `src-tauri/resources/test_*.py` suite to retain the SDK baseline.
`scripts.test_cli_ci` is excluded because CI wiring does not execute this Python
implementation. CI wiring has a separate mandatory test and release gate.

## Coverage

Use the actual project Python with its installed numpy and PyInstaller. Add the
task-local coverage installation to this process's `PYTHONPATH`; do not install
packages or alter a global `.pth` file. Set these task-local values before running:

```text
PATH: project Python's executable directory first
PYTHONPATH: formal/cli/coverage_startup; task-local tool site-packages; src-tauri/resources
COVERAGE_CORE: ctrace
COVERAGE_PROCESS_START: absolute formal/cli/coverage.ini path
COVERAGE_FILE: fresh external evidence directory/.coverage
```

Run the application modules plus `scripts.test_cli_measurements` through
`python -m unittest`, then `python -m unittest discover -s src-tauri/resources -p
test_*.py`. These direct Python invocations are instrumented by the startup hook,
as are inheriting fixture children. Then clear `COVERAGE_*` variables and remove
the startup directory from `PYTHONPATH`, keeping the tool installation available:

```text
python -m coverage combine --rcfile formal/cli/coverage.ini --data-file <evidence>/.coverage <evidence>
python -m coverage json --rcfile formal/cli/coverage.ini --data-file <evidence>/.coverage -o <evidence>/coverage.json
python -m coverage report --rcfile formal/cli/coverage.ini --data-file <evidence>/.coverage
```

The run include list instruments all four frozen production files. Do not use
`source = scripts`: the bootstrap tests execute byte-for-byte production copies
under temporary `source/scripts` layouts. The path alias combines only those
copies with the original scripts after execution. Retain pre-combine parallel
datasets and verify fixture copies read the frozen production bytes. Require all
four expected production files in the resulting JSON; a missing file is an OPEN
instrumentation gap, never an omitted denominator. Coverage 7.17 JSON includes
function regions; preserve whole-file summaries and separately report the exact
`NativeRuntime.__init__` and `QwenNativeRuntime.__init__` regions, with missing
lines/arcs. Record branch outcomes as well as executable statements.

## Source mutation

Use task-local Cosmic Ray 8.9 with the actual project interpreter. The committed
commands use `python`; no machine-specific Python path is stored in configuration.
On Windows, putting the project interpreter first on `PATH` is insufficient:
a child command can resolve `python` beside the base interpreter. Prepare an
external execution copy of each configuration with the absolute project interpreter
for both the wrapper and selector invocations. Verify that only those two command
arguments changed. Use that same resolved configuration for baseline, discovery
and execution. Clear inherited coverage instrumentation first.
The wrapper keeps `sys.executable` for its default child and supplies the SDK
resource import path after clearing the coverage bootstrap.

| Configuration | Tests and reachability |
| --- | --- |
| `cosmic-ray.toml` | Entire CLI; complete default CLI, packager, constructor and runner regression cohort. |
| `cosmic-ray-build.toml` | Entire packager; all artifact/build tests and runner regressions. The packager does not call SDK constructors or CLI command dispatch. |
| `cosmic-ray-sdk.toml` | Both SDK modules for complete generation; selected constructor worklist uses all maintained constructor tests, actual CLI-to-SDK thread tests, and runner regressions. |

For each configuration, create fresh baseline and session databases:

```text
cosmic-ray baseline <config> --session-file <evidence>/baseline.sqlite
cosmic-ray init <config> <evidence>/all-generated.sqlite
cosmic-ray dump <evidence>/all-generated.sqlite
cosmic-ray exec <config> <evidence>/session.sqlite
cosmic-ray dump <evidence>/session.sqlite
cr-report --all-mutations --no-show-output --no-show-diff <evidence>/session.sqlite
cr-rate <evidence>/session.sqlite
```

For CLI and packager, the execution session is the full generated worklist. SDK
generation covers both complete modules. Before SDK execution, retain that full
list, then copy every generated item whose mutation location lies inside the
AST-defined complete constructor into a fresh session using Cosmic Ray's
`WorkDB.add_work_items`. Resolve the exact class and `__init__` range from the
frozen Python AST; do not select every method named `__init__` in the Qwen module.
Retain selected and out-of-scope job IDs and reconcile their union against the
full generated set. `definition_name` is unqualified in Cosmic Ray 8.9, so it
cannot alone identify a constructor. The tested `select_sdk_mutants.py` helper
performs this AST-based partition and rejects duplicate IDs, unknown modules,
boundary-crossing mutations and work items mixing constructor/outside mutations:

```text
python formal/cli/select_sdk_mutants.py <evidence>/all-generated.sqlite <evidence>/session.sqlite <evidence>/partition.json
```

The output database and partition report must be fresh paths. No in-scope
operator, location or survivor may be filtered.

The wrapper reuses the frozen `formal/agent-api-host/run.py` `Job` implementation
through a repository-relative import. On Windows it assigns the acquired process
handle before waiting and closes the Job on normal completion, timeout and wait
errors. Actual synthetic-process tests check that root and remaining descendants
are reaped while arguments, environment and the live execution grace are retained.
No PID/name tree-kill fallback runs. The process-creation-to-Job-assignment interval
does not establish arbitrary OS/crash containment. POSIX uses a process group;
actual POSIX execution remains unverified. Do not count a wrapper/framework timeout or
cleanup failure as an assertion kill. Preserve every runner receipt and inspect
the logs before classifying detection, survival, invalid source, timeout, tool
error or pending work. Replay and explain survivors against the public contract.
Supplement framework operator blind spots with separately reported critical
guard/ordering negative controls; these never replace the generated denominator.

## Current capture

The current capture passes 551 tests in 88.501 seconds. The owned launcher exits
normally, reaps its root and reports no cleanup error. All three complete Cosmic
Ray baselines pass in the same maintained parent environment: SDK 53.843 seconds,
packager 56.594 seconds and CLI 53.579 seconds. The four production hashes match
the [current verification record](../evidence/agent-cli-180.md).

| Scope | Covered/total executable lines | Covered/total branch outcomes |
| --- | ---: | ---: |
| Complete CLI module | 397/397 | 156/156 |
| Complete packager module | 77/77 | 20/20 |
| Whole R2T2 SDK baseline | 181/186 | 66/72 |
| Whole Qwen SDK baseline | 294/306 | 80/90 |
| Derived complete R2T2 constructor | 59/59 | 22/22 |
| Derived complete Qwen constructor | 2/2 | 0/0 |

Constructor counts use the original captured AST coordinates. Whole SDK reports
retain 17 uncovered lines and 16 uncovered branch outcomes outside that cohort.
The context dataset SHA256 is
`286a1f2f4bf42e255239e9f6b21bc1f7a8aa451b3119b94a08f47c070b6e5010`.

Fresh generation produces 1,027 CLI items, 232 packager items and 979 SDK items.
The AST partition retains 181 constructor items and preserves all 798 other SDK
items outside the affected cohort. The in-scope denominator is 1,440. SDK execution
completes with 150 killed and 31 surviving items, all normal worker outcomes.
Packager execution completes with 209 killed and 23 surviving items, also all
normal worker outcomes. Survivor review and CLI execution remain open. No adjusted
mutation score or complete-detection claim is made.

Eight additional SDK tests now pass, extending the constructor suite from five
to thirteen cases. Each of the eighteen targeted frozen mutations produces an
intended assertion failure; all restored runs pass. Independent review accepts
this test-only slice. The four production hashes remain unchanged, but the final
cohort capture and framework run must include the updated tests.
Six packager regressions also pass, extending its module from 26 to 32 tests.
Each exact targeted mutation fails the intended assertion, with no test errors,
and every restoration passes. These tests cover malformed PE prefixes, input
changes in both directions, private filenames, failed staged builds, redirected
text encoding and retained archive-error causes. Independent review and assembled
tests pass. The final measurement cohort must include this test-only update too.

## Earlier capture before numeric admission

The earlier measurement cohort passed 544 tests: 205 CLI/packager/measurement tests
and 339 resource regressions. That capture took 85.571 seconds. Its owned
launcher exits normally, reaps the root and reports no cleanup failure. The four
production source hashes and effective environment bindings were recorded.
The numeric-admission fix changes the CLI source and adds a regression module.
These coverage figures and generated counts do not qualify the updated source.
Recapture coverage, rerun full baselines and regenerate every applicable mutant
before reporting current measurements. Do not preserve the old CLI count by force.
Source bytecode caching is disabled before imports so rapid same-size mutations
cannot reuse stale compiled source.

| Scope | Covered/total executable lines | Covered/total branch outcomes |
| --- | ---: | ---: |
| Complete CLI module | 393/393 | 156/156 |
| Complete packager module | 77/77 | 20/20 |
| Whole R2T2 SDK baseline | 181/186 | 66/72 |
| Whole Qwen SDK baseline | 294/306 | 80/90 |
| Derived complete R2T2 constructor | 59/59 | 22/22 |
| Derived complete Qwen constructor | 2/2 | 0/0 |

The whole SDK figures retain methods outside the constructor cohort. No missing
outcome is removed from those raw reports. The earlier context dataset SHA256 is
`604350a282938be322faa79f107ce864b09d52ed08076296a5010148a4a40055`.
Earlier partial captures and failed infrastructure baselines remain external
historical records; they establish no current mutant detection.

The resolved CLI, build and SDK framework baselines all pass the complete
544-test cohort in approximately 55, 55 and 54 seconds. Two cumulative-delay
controls also pass within the accepted wrapper/framework limits of 120/135
seconds. Keep those limits for this matching environment and remeasure after
binding changes. Timeouts require separate investigation and are not assertion
kills.

Cosmic Ray generates 1,025 complete-CLI items and 232 complete-packager items.
The two complete SDK modules generate 979 items. The tested AST partition retains
all 181 constructor items (170 R2T2, 11 Qwen), with 798 explicitly outside the
derived cohort. Selected and outside IDs are disjoint and their union matches
the full SDK generation. The earlier in-scope denominator was 1,438 items.

The conservative context selector projects 1,212 selective executions and 226
full fallbacks. Those are planned selections, not completed mutation outcomes.
Complete framework execution and per-mutant classification remain open. See the
[current verification record](../evidence/agent-cli-180.md) and
[context-selection contract](ContextSelection.md) for bindings and limits.
