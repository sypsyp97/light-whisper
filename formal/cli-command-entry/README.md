# CLI command entry verification

This cohort covers the installed console alias, frozen desktop discovery, and
the serialized `Invoke-CliCommandInstall` lifecycle. It adds no CLI actions.

`scripts/test_cli_command_discovery.py` calls the existing parser/main interface
and records the actual broker command at `EngineClient`. Its synthetic broker
does not prove execution of a packaged executable. The tests cover the bin
alias, root CLI compatibility, all existing actions, custom installation roots,
explicit desktop selection, fallback discovery, and refusing to select itself.

`scripts/test_cli_command_installation.py` starts Windows PowerShell and calls
the actual installer function with real disposable HKCU keys and files. It
does not call the script's real `HKCU\Environment` entrypoint. The cases cover
byte-identical copying, repeated install/removal, missing source, copy/write
errors, long PATH values, String/ExpandString, raw unrelated tokens, bin junction
rejection, foreign bin content and user-data preservation. Initial RED is against
the declaration-only installer scaffold, not a prior working installation.

Run from the repository root:

```powershell
python -m unittest scripts.test_cli_command_discovery scripts.test_cli_command_installation -q
```

## Finite lifecycle model

`CommandEntry.tla` explores two sequential install/uninstall calls. Calls choose
normal, missing-source, copy-error or registry-write-error outcomes. Initial
ownership is empty; the path can be ordinary or a preexisting reparse point.
Uninstall also admits or denies the caller's process-close check. A denial
represents either cancelling the prompt or failing to close the desktop process.
It must retain the alias and own PATH count captured at the start of that call.
Foreign PATH tokens retain an ordered three-token representative containing an
empty token and an expandable token. `ownCount` abstracts normalized own-path
identity; string normalization itself is exercised by PowerShell tests.

| Source obligation | Model binding | Behavioral test |
| --- | --- | --- |
| Decide process-close cancellation/failure before command cleanup | `Admit`, `DeniedPreservesCommand` | NSIS pre-uninstall hook source-order regression |
| Reject bin/alias reparse points before effects | `Validate`, `ForeignResourceUntouched` | bin-junction install/uninstall |
| Copy alias before PATH publication | `Copy` then `Publish`, `PublishedAfterCopy` | missing source, copy failure, byte identity |
| Preserve foreign tokens and one own entry | `Publish`/`Unpublish`, `PathIntegrity` | long raw tokens, quote/case/slash duplicates |
| Remove own PATH entry then own alias | `Unpublish` then `Cleanup`, `CleanupComplete` | repeat uninstall, foreign content, empty bin |
| Calls return success or visible failure | `OperationSettles` | missing/copy/write errors and subprocess deadline |

The source owner must verify final production/model correspondence and hashes.
In the inspected production function, reparse checks precede registry access,
`File.Copy` precedes `SetValue`, and uninstall `SetValue` precedes `File.Delete`.
This model represents registry-write failure with retained copied alias, matching
that ordering. It does not assert transactional rollback after partial failure.
The caller boundary binds to `CheckIfAppIsRunning` in `NSIS_HOOK_PREUNINSTALL`
before `LIGHT_WHISPER_COMMAND "-Uninstall"`. The pinned Tauri 2.10.0 macro
aborts on cancellation or process-close failure. `late-admission` instead runs
the command cleanup first, matching the defective hook/template ordering.

The positive config activates every listed safety property and progress.
Weak fairness assumes a synchronous operation's OS calls eventually return.
Terminal self-loops represent completed calls. No symmetry, state constraints,
disabled deadlock checks, or depth limits are used.

Run SANY, then every config with one worker and a 512 MiB heap. Use the existing
reviewed TLA+ 1.7.4 JAR; its SHA256 is
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.

```powershell
Set-Location formal/cli-command-entry
java -Xmx512m -cp $env:TLA2TOOLS_JAR tla2sany.SANY CommandEntry.tla
java -Xmx512m -cp $env:TLA2TOOLS_JAR tlc2.TLC -workers 1 -coverage 1 -metadir $env:MODEL_OUTPUT -config positive.cfg CommandEntry.tla
```

Give each subsequent run a fresh metadata directory and substitute its config:

| Config | Expected result |
| --- | --- |
| `positive.cfg` | complete, no errors |
| `early-publication-negative.cfg` | `PublishedAfterCopy` violation |
| `foreign-path-negative.cfg` | `PathIntegrity` violation |
| `duplicate-path-negative.cfg` | `PathIntegrity` violation |
| `cleanup-negative.cfg` | `CleanupComplete` violation |
| `reparse-negative.cfg` | `ForeignResourceUntouched` violation |
| `progress-negative.cfg` | temporal violation of configured `OperationSettles` |
| `lifecycle-witness.cfg` | `LifecycleNotReached` violation reaching install then removal |
| `admission-negative.cfg` | `DeniedPreservesCommand` violation after cleanup precedes denial |
| `admission-witness.cfg` | `DeniedNotReached` violation reaching denial with an installed command preserved |

Model bounds exclude concurrent external PATH/filesystem writers, crashes,
arbitrarily blocked OS calls, alias content/PE semantics, registry value kinds,
individual Unicode/path spellings, and installer broadcast/native shell wiring.
Admission is assumed to remain valid until cleanup; an external desktop restart
between the hook and the template's repeated process check is outside this model.
Those are explicit limits, not properties proved by TLC. The model does not
describe NSIS as a whole or claim a native execution proof.

## Bounded source measurements

`coverage.ini` includes only the actual Python CLI file named by the mandatory
`CLI_COMMAND_SOURCE` environment variable. Its exact absolute path excludes the
bootstrap tests' disposable source copies. Keep the whole-module raw
denominator, then derive the affected `app_path` function's lines and branch arcs
from coverage JSON. The reviewed current function spans lines 256-268; verify
these via AST after source changes before using the mutation line filter.

Use the existing task-local coverage.py and Cosmic Ray environment, with its
Python first in this process's PATH so Cosmic Ray's test command uses that same
interpreter. Do not alter the user's persistent PATH. Example commands in a
disposable source checkout after an unmutated GREEN baseline:

```powershell
$env:CLI_COMMAND_SOURCE = (Resolve-Path scripts/light_whisper_cli.py).Path
python -m coverage run --rcfile=formal/cli-command-entry/coverage.ini -m unittest -q scripts.test_cli_command_discovery scripts.test_cli_target scripts.test_cli_bootstrap
python -m coverage json --rcfile=formal/cli-command-entry/coverage.ini -o $env:COVERAGE_OUTPUT
cosmic-ray init formal/cli-command-entry/cosmic-ray.toml $env:MUTATION_SESSION
cr-filter-lines --config formal/cli-command-entry/cosmic-ray.toml $env:MUTATION_SESSION
cosmic-ray exec formal/cli-command-entry/cosmic-ray.toml $env:MUTATION_SESSION
```

The mutation cohort is only `app_path`, using seven new discovery tests plus two
existing discovery tests. Report all generated in-function mutations and every
survivor; out-of-function mutations skipped by the native filter are outside
this cohort. Preserve raw reports. No score is claimed before execution.

Python instrumentation does not measure PowerShell, NSIS, native argument/stdio
forwarding or environment broadcast. PowerShell branch coverage and source
mutation remain unmeasured with this bounded toolchain. Real PowerShell tests,
model negatives and native shell evidence are distinct from those missing
instrumentation results.

`scripts/test_installer_command.ps1` supplies separate native evidence on a fresh
GitHub-hosted Windows VM: install, plain-command discovery/doctor/help/error in
PowerShell 5, PowerShell 7 and cmd, repeated update, and silent uninstall. It
checks unrelated raw user PATH tokens, registry kind and synthetic user-data
preservation without loading models. The driver refuses existing installations
before effects; its JSON receipt contains only booleans, version and installer
hash. This smoke test does not execute the interactive cancellation prompt and
does not add a formal or native cancellation proof.
