# CLI CI and release gate

This cohort protects publication of the exact release candidate after all six
required jobs succeed. It adds `CLI contracts` to the five existing names:
`Frontend`, `Python`, `Rust`, `Application formal contracts`, and
`Verus implementation proofs`. Aggregate workflow success never replaces a
successful conclusion for each named job. A missing, skipped, failed or
cancelled CLI job blocks publication. A successful run for another SHA also
blocks publication.

`scripts/test_release_ci.py` extracts and executes the real `wait_for_ci` shell
function. Its local `gh` boundary provides known run/job observations; it never
calls the network, GitHub write commands, tag creation or release publication.
All six required CI job names remain covered.
The earlier release gate produced four semantic
assertion failures: bad CLI conclusions return exit code 0 instead of blocking.
The same test module executes the actual `publish_release` function through
local Git/GitHub boundaries. Its fetch changes HEAD, the dirty state or remote
HEAD between real function observations. The unchanged baseline incorrectly
tags and releases after the first two changes; both are semantic RED. Stable
publication must tag the explicit frozen candidate before pushing/releasing,
while a failed fetch or changed remote must have no publication effects.

`scripts/test_cli_ci.py` supplements executable cohorts with workflow wiring
assertions. It executes the actual PowerShell native-command groups named
`Run CLI unit contracts` and `Run native CLI contracts` at every injected failure
position through the existing `test_ci_fail_fast` boundary. Other checks inspect
invocations and configuration; they do not claim to execute live CI, native Wry,
model inference, installed software or packaged artifacts. Those real surfaces
remain required commands in the Windows CLI job. No new production orchestrator
is required. Every required execution step must be unconditional and propagate
failure; only evidence upload may use `if: always()`. The UI model script must
be integrated and invoked via a fresh `pwsh -NoProfile -File` process, so its
expected TLC negative exits are not interrupted by the parent's native-command
failure preference.

## State and source correspondence

| Obligation | Owning source | Model action/property | Test or evidence |
| --- | --- | --- | --- |
| Failed local checks cannot create a candidate | `scripts/release.sh`: `main`, `run_local_checks`, fail-fast shell | `LocalResult`, `FreezeCandidate`, `LocalSafety` | Local CLI test/harness wiring checks; source review of main sequencing |
| All five old jobs and the CLI job must succeed, even after aggregate success | `wait_for_ci` named job loop | `DecideCI`, `ExistingSafety`, `CliSafety`, `AggregateSafety` | Actual extracted shell with each failed/missing/skipped/cancelled job observation |
| CI must belong to the frozen candidate | `wait_for_ci` `headSha` equality | `DecideCI`, `ShaSafety` | Actual shell with six successes and wrong SHA |
| Candidate cannot be tagged before CI acceptance | `main`: `wait_for_ci` before `publish_release` | `SkipWait`, `WaitSafety` | Sequencing source review, wait-negative sensitivity |
| HEAD and clean state are reobserved after fetch, alongside remote identity, before the explicit candidate tag | `publish_release` post-fetch guards | `Fetch`, `ObserveRemote`, `ObserveHead`, `ObserveClean`, `Tag`, `FinalIdentitySafety` | Actual function with fetch changing HEAD/dirty/remote; independent skipped-observation controls |
| Release creation follows successful tag creation/push | `publish_release` fail-fast command sequence | `Publish`, `TagSafety` | Tag-order control; source review |
| Settled checks and CI eventually settle the decision | External command return plus shell progress | Weak fairness, `EventuallyDecided` | Positive liveness and disabled-release progress negative |

The five old job results are abstracted by their conjunction. They are symmetric
inputs to the same required-name loop; the shell tests exercise the individual
names. There are two distinct SHA representatives (`A`, `B`), five terminal CLI
conclusions plus pending, local check success/failure, aggregate success/failure,
and one optional change during fetch (HEAD, remote HEAD or dirty worktree), with
an initially clean candidate. Initial HEAD/clean reads, fetch, each post-fetch
identity read, tagging and release are separate actions. Head/clean negatives
omit the post-fetch read/rejection and retain stale pre-fetch observations,
matching the actual baseline. The remote negative omits its real read/rejection.
The finite lifecycle has one candidate; retries/new releases need a new run.

This model explicitly assumes an exclusive publisher and no external writers
from the final post-fetch observations through tag creation. Under that
assumption, accepted observations still describe the state when the separate tag
action uses the frozen candidate. The implementation does not acquire an atomic
snapshot or global/ref lock across local processes, Git and GitHub. Arbitrary
writes after those final observations are outside this guarantee; the actual
regression tests establish rejection of changes made during fetch, not atomic
publication in the presence of arbitrary concurrent writers.

Weak fairness assumes local commands and CI observation eventually return and
the shell continues scheduling its enabled steps. It does not establish GitHub
availability or crash durability. The explicit unchanged action represents
waiting/terminal states and permits a missing progress action to produce the
intended temporal counterexample rather than a deadlock. No symmetry, state
constraints, depth bounds or disabled deadlock checking are used.

The positive configuration checks every safety invariant and eventual decision.
Each guard has its own negative config. The progress control disables actual
release completion and must violate the configured temporal property. Witnesses
independently reach a successful release, four rejected CLI statuses and a
rejected identity drift. Witnesses are reachability checks, never mutation kills.
The matrix runner requires TLC exit 0, 12 or 13 and the exact intended semantic
outcome; authoring errors and arbitrary nonzero exits do not pass.

## Commands and limits

```powershell
python -m unittest discover -s scripts -p test_release_ci.py -v
python -m unittest discover -s scripts -p test_cli_ci.py -v
python formal/cli-ci/check_model.py --java <java> --jar <tla2tools.jar> --output <new-evidence-directory>
```

The runner checks the repository-pinned TLA+ 1.7.4 JAR SHA256, preflights with
SANY, then executes all 18 finite configurations at 512 MiB and one worker.
Logs and a receipt bind commands, outcomes, state counts, model/config/test/source
hashes and explicit assumptions. Receipt source correspondence remains OPEN
until main accepts the test slice, implements the gate and reviews/reruns its
final binding. Static CI assertions alone cannot establish actual workflow
execution, assembled CLI behavior, source branch coverage or source mutation
scores. These remain integration-owner obligations.
