# CLI protocol verification

Frozen cohort: `scripts/light_whisper_cli.py`, `EngineClient` and `main(argv)`.
Tests use the public client with a real Python child and real stdin/stdout JSONL.
Only CLI command planning is redirected to that child. No model, GPU, network,
clipboard, running GUI or user application data is accessed.

## Hidden state and correspondence

| Obligation / owner | Source binding | Model / checked property | Test schedule |
| --- | --- | --- | --- |
| Acquired process cannot admit requests before successful startup | `EngineClient.__enter__` | `Spawn`, `StartupSuccess`, `StartupFalse`, `ReadyRequiresStartup` | startup noise followed by success; false / EOF / timeout followed by reap |
| Ownership begins immediately after process acquisition, before reader allocation/start | `EngineClient.__enter__` acquisition-to-reader boundary | `Spawn`, `StartReader`, `ReaderSetupFailure`, `FaultRetainsOwnership` | standard `Thread` construction and `Thread.start` each raise the original `RuntimeError` / `KeyboardInterrupt`; actual child reaped and pipes closed |
| Failed process acquisition preserves the original error and acquires no cleanup resource | `EngineClient.__enter__` process creation boundary | no acquired process; outside the stateful model | real `Popen` boundary raises the injected `OSError`; original object preserved and no child registered |
| Reader ignores non-JSON, non-object and non-response startup lines | `EngineClient` stdout reader | `Noise` | native log, malformed JSON, array and event object before startup |
| Monotonic request identity and stale/future response exclusion | `EngineClient.request` | `Request`, `ReceiveStale`, `ReceiveMatching`, `Publish`, `OnlyCorrelated` | two requests with stale/future/no-ID responses; 260 real requests cross CPython's small-integer cache |
| Matching failure retains all engine response data | `EngineClient.request`, `main` | publication abstracts payload; identity remains checked | backend `success:false`, error and nested detail preserved; CLI exit 1 |
| EOF / bounded timeout terminates startup and waiting | `EngineClient.__enter__`, `request` | `EOF`, `Timeout`, `EventuallyReaped` | child exits or closes stdout while remaining alive; blocked input/event; no sleep establishes ordering |
| Stdin backpressure consumes the same request deadline | `EngineClient.request`, `_send`, cleanup | `Timeout`, `Kill`, `Reap` abstracts pipe waits | real nonreading child receives a request larger than its pipe buffer; timeout and reap |
| Response and sender waits cannot extend an exhausted request budget | `EngineClient.request`, `_receive`, `_send` | `Timeout`; quantitative clock behavior outside the finite model | real backend plus controlled standard queue/thread dispatch; partial and expired receive waits after actual sender termination must reach the receiver and retain `TimeoutError` |
| Dequeued response cannot publish after the deadline | `EngineClient.request`, `_receive` | `Timeout` precedes `Publish` | artificial scheduler boundary wraps standard-library queue delivery of a real child response, advances the clock, and requires `TimeoutError`; this is public-client evidence with a controlled scheduler, not an unmodified OS scheduling claim |
| Context exit sends exit, kills uncooperative child, waits/reaps | `EngineClient.__exit__` and startup failure cleanup | `Quit`, `SendExit`, `GracefulEnd`, `Kill`, `Reap`, `ReapedMeansReleased`, `EventuallyReaped` | caller exception, failed startup/request and stubborn exit; actual `Popen.poll/wait` |
| Cleanup interruption / sender launch failure retains cleanup ownership and original error | `EngineClient.__exit__`, exit send and graceful wait | `CleanupFailure`, `Kill`, `FaultRetainsOwnership`, `EventuallyReaped` | standard `Thread.start` and `Popen.wait` faults through public client; isolated watchdog reports a failed cleanup instead of hanging the suite; public `main` must publish JSON exit 130 |
| Cleanup does not join an unstarted worker and mask the original fault | worker joins in `EngineClient.__exit__` | `JoinWorkers`, `NoUnstartedJoin` | failed reader/sender thread launch must retain the injected error object/message and close actual child pipes |
| Graceful send, forced reap and worker waits share the cleanup budget | `EngineClient.__exit__` | `SendExit`, `Kill`, `Reap`, `JoinWorkers`; quantitative time outside the model | controlled standard queue/wait/join boundaries around real children and reader threads; a completion at 1.5 seconds must fit, exhausted waits cannot extend, and omitted joins leave the reader observably alive |
| One JSON object and exit codes for each CLI command/error | `main` | argument/output details outside protocol model | doctor/status/stats/transcribe, parse/path/spawn/protocol failures, interrupt; real script with Unicode paths/errors and redirected Windows CP1252 stdout |
| Script entrypoint forwards argument failure exit status | `__main__`, `main` | outside protocol model | execute real script via `runpy` with invalid arguments, require `SystemExit(2)` and one JSON object |
| Child settings/data have one command lifetime | `main` environment and data-directory context | process ownership only; directory/environment outside model | explicit interpreter/executable, threads/offline flags, unique temporary dirs cleaned, explicit data retained |
| Diagnostics report actual paths without claiming model readiness | `doctor`, parser and `main` | argument/diagnostic details outside the model | explicit existing/missing runtimes, decoded command strings, missing command, invalid options with an independently valid interpreter, one-thread budget and nested data directory |

The initial declaration-only module supplied historical scaffold RED. Later
reader-launch and cleanup-interruption tests exposed executable regressions,
independent of that original interface evidence. The implemented source must
match these contract bindings; the model does not prove Python refinement.
Current source hashes, full regression, coverage, runtime results and remaining
gates are in the [1.8.0 verification record](../evidence/agent-cli-180.md).

Test fixture cleanup captures real `Popen` instances independently of client
fields and runs after behavioral assertions. Watchdog probes isolate potentially
blocking pipe-close paths, retain logs on disk, and terminate only their owned
worker/process tree when the 3.5-second failure watchdog expires. This watchdog
is test containment, not the product's cleanup implementation.

## Finite model and assumptions

One serial client owns one child, at most two requests (IDs 1, 2), and one
nonmatching response representative (ID 3). Both cooperative and stubborn children
are initial choices. Startup false, startup/request EOF, timeout, noise, stale
response, close, forced termination and reap are separate transitions. There is
an independent process state after stdout EOF: the child may remain running or
may already have ended, and cleanup still owns kill/wait/reap. There is
no GUI actor or concurrent request API in the frozen contract. Process creation
failure acquires no child and is tested outside this model. Response success and
failure share the same identity/publication transition; payload preservation is
tested against the real client.

The fake backend uses the current Python interpreter's base executable. On
Windows, a virtual-environment redirector retains its inherited stdout handle
while the backend runs; closing the backend's own stdout descriptor alone does
not deliver EOF to the client. A controlled check observed the real backend
terminate within 0.00089 seconds after terminating/reaping its redirector. The
base interpreter therefore isolates the intended live-process stdout-EOF case
without changing its 0.3-second timeout or expected `EOFError`. CLI command
planning tests still use the explicitly selected `sys.executable`. This model
and the cleanup assertions own the `Popen` process; arbitrary additional
descendant-process cleanup remains unverified.

Acquisition and reader setup are separate actions. Reader construction/start may
fail with or without an allocated unstarted worker. A cleanup send/wait can be
interrupted or fail before process termination. Cleanup retains the resource
owner, forces termination when graceful exit fails, joins only started workers,
and then reaps. The finite abstraction admits one reader setup failure and one
cleanup fault per client; repeated asynchronous interrupts beyond those bounds
are not proved. `workerUnstarted` represents either an allocated reader or sender
whose start failed, and `badJoin` represents the resulting exception masking.
Cleanup can allocate an unstarted sender only while the process is running;
the already-ended branch may be interrupted during wait but creates no sender.
An unstarted reader allocated before a setup failure can persist until after
process termination and must still be skipped during joins.

`Spec` weakly fairly schedules startup/wait deadlines and cleanup transitions.
This assumes the OS schedules the client and honors kill/wait; arbitrary OS hangs
are outside the finite model. Timeout is an eventual deadline action, not a
wall-clock proof. `AwaitCleanup` is a deliberate waiting stutter so missing
kill/reap progress yields a temporal counterexample, rather than an unrelated
deadlock. No symmetry, state constraints, depth bounds or deadlock suppression.

Positive `EngineClient.cfg` enables `TypeOK`, `OnlyCorrelated`,
`ReadyRequiresStartup`, `ReapedMeansReleased`, and `EventuallyReaped`.
It also enables `FaultRetainsOwnership` and `NoUnstartedJoin`.

| Config | Expected result / sensitivity |
| --- | --- |
| `CorrelationNegative` | `OnlyCorrelated` fails after removing matching-ID guard |
| `StartupNegative` | `ReadyRequiresStartup` fails after accepting startup false |
| `ReapNegative` | `ReapedMeansReleased` fails after reporting reap before child ends |
| `KillProgressNegative` | `EventuallyReaped` fails for stubborn child without forced kill |
| `ReapProgressNegative` | `EventuallyReaped` fails without reap |
| `CorrelationWitness` | named witness fails after stale receipt and two correlated publications |
| `TimeoutWitness` | named witness fails after timeout and real release/reap |
| `StartupWitness` | named witness fails after startup rejection and reap |
| `ReaderSetupNegative` | `FaultRetainsOwnership` fails when a setup fault abandons the acquired process |
| `CleanupFailureNegative` | `FaultRetainsOwnership` fails when a cleanup fault abandons the process |
| `UnstartedJoinNegative` | `NoUnstartedJoin` fails when cleanup joins an allocated but unstarted worker |
| `ReaderFailureWitness` | named witness fails after unstarted reader setup failure and successful reap |
| `CleanupFailureWitness` | named witness fails after cleanup failure and successful reap |

Witnesses establish reachable antecedents. They are separate from negative controls.

## Sender handoff slice

`SenderHandoff.tla` starts after successful `EngineClient.__enter__`: one ready
serial client owns one running child. It supplements `EngineClient.tla`; its
startup-ready precondition does not replace the original lifecycle model or
compose the two models into a refinement proof. The original fourteen
configurations and their configured properties are unchanged. The supplemental
model bounds execution to two serial requests, one child, two distinct request
writer identities, and cooperative/stubborn child choices.

The earlier implementation published a successful
`_send` queue result before its writer thread necessarily terminates. The
baseline source LF SHA256 is
`487f120aa5747a976e649d679c78cad234bb8a5bee94a1f1b2c17d79606b906b`.
The positive supplemental model specifies the required handoff; the baseline
does not implement its `WaitForTail` guard. Source correspondence must be checked
again against the final production content before reporting GREEN.

The reviewed repair joins the successful sender with the same remaining request
deadline, then rejects a still-live sender with `TimeoutError`. EOF error
precedence is unchanged. The repaired production LF SHA256 is
`7aa551f29b153fb0634bffdff0c79a4773b8a652bdedabfb5e661ddff2ccb885`;
the accepted test LF SHA256 is
`c889f86279431237d6ddda342dc259b18b8e6132bc78b754fa98a93273ead61f`.

| Requirement / source owner | Model state and checked property | Executable schedule |
| --- | --- | --- |
| A published write outcome does not establish writer termination; `_send` must retain its active worker until bounded handoff | `outcomes` changes in `WriteOutcome`; `writers` changes separately in `StopWriter`; `Handoff` requires the current writer to stop | A real child accepts the write; standard `Queue.put` publishes its actual outcome and then blocks the real sender tail; no seeded client slot or replaced request method |
| A completed `request` must own no live prior sender | `PublishResponse`, `published.quiescent`, `NoPrematureHandoff` | `test_completed_requests_release_the_sender_before_handoff_and_exit` captures live held writers at each public response, then requires a real recorded exit and cooperative child return code 0 |
| Serial requests cannot overwrite the only reference to an active prior writer; `request` / `_send` own `self.sender` | `Request` replaces `slot`, independently retained `writers` detects the lost owner; `NoLostSenderSlot` | The same public test performs both one-request and two-request schedules; its first sender remains held until the actual bounded thread wait releases it |
| Sender completion waits share the original request deadline; pending work must become `TimeoutError` without a fresh budget | `ObserveWrite`, `handoffBudget`, `NoExtraTailBudget`; symbolic `budget`, `Deadline`, `FailDeadline`, `EventuallyReaped`, `RequestEventuallySettles` | `test_sender_tail_timeout_uses_the_remaining_request_budget_and_reaps` advances the clock after real result dequeue, holds the real writer alive, and checks remaining and already-exhausted budgets, typed error, actual reap and closed pipes |
| Timeout/write failure retains ownership of a live sender; `__exit__` skips exit send when `self.sender.is_alive()` and may kill after grace wait | `FailDeadline` / `FailWrite` preserve `writers`; `DecideExit` records `skippedExit`; `Kill` explicitly permits the skipped-exit path; `ReapedMeansReleased` | New exhausted-tail test plus existing real nonreading-stdin and broken-stdin tests; an exit message is required only after successful completed handoff |
| A completed request allows graceful exit-send; `__exit__` must see a quiescent sender | `GracefulExitNotSkipped` follows the same `WaitForTail` guard as `NoPrematureHandoff` and `NoLostSenderSlot`; both safety negatives remove that shared guard | The held-tail success schedule requires the real child to record action `exit`, exit with code 0, and be reaped with both pipes closed |

`WriteOutcome` and `StopWriter` may occur in either order relative to response
arrival. `ObserveWrite` is the caller's separate observation of the published
write outcome. An unfinished write can reach its deadline without publishing an
outcome. Cleanup retains the current slot and may reap the child while a worker
tail still awaits scheduling; the model does not claim that a bounded cleanup
wait can force an arbitrary thread to terminate. `StopWriter` after process end
abstracts the OS unblocking a pending pipe operation and later scheduling the
worker. The cleanup exit writer is abstracted by exit-send/grace/kill actions;
the existing lifecycle configurations and concrete launch/cleanup fault tests
retain its interruption and unstarted-worker obligations.

`ObserveWrite` uses a finite original budget of two abstract ticks and elapsed
dispatch representatives 0, 1, 2 and 3: full, partial, just exhausted and already
expired. It records the tail-wait allotment separately. `NoExtraTailBudget`
requires elapsed plus allotted time to fit within the original budget, or a zero
allotment after expiry. This is a bound on the requested wait, not an OS timing
guarantee. `SenderBudgetNegative` deliberately refreshes the allotment to two
ticks; unlike eventual-settlement properties, this safety check detects that
extra time even though the request could eventually finish. The tail-timeout
witness requires the reachable partial-budget case (one tick already consumed,
one tick allotted), followed by timeout with the real writer still owned.

`Deadline` remains a symbolic expiration event, not a wall-clock proof.
Weak fairness of deadline processing and cleanup assumes a scheduled client and
OS kill/wait progress. `RetainDeadline = FALSE` removes only the transition from
an expired request into owned cleanup; waiting stutters remain enabled, so its
expected temporal counterexample is not a deadlock. No state constraints,
symmetry, depth bounds or deadlock suppression are used.

| Supplemental config | Expected result |
| --- | --- |
| `SenderHandoff` | Exhaustive safety and request/cleanup progress success |
| `SenderHandoffNegative` | `NoPrematureHandoff` fails after write outcome publication, premature handoff, response publication while the writer remains alive |
| `SenderOwnershipNegative` | `NoLostSenderSlot` fails when premature publication permits request 2 to replace request 1's live writer slot |
| `SenderDeadlineProgressNegative` | Configured temporal properties fail after an expired request cannot enter cleanup |
| `SenderBudgetNegative` | `NoExtraTailBudget` fails when the tail wait receives a fresh full budget after dispatch already consumed time |
| `SenderHandoffWitness` | `TailHandoffNotReached` fails after a published live tail, two quiescent responses and graceful child reap |
| `SenderTailTimeoutWitness` | `TailTimeoutSkipReapedNotReached` fails after a published successful write observes and allots a partial remaining budget, times out with its writer alive, skips exit-send, kills and reaps |
| `SenderBlockedTimeoutWitness` | `BlockedTimeoutSkipReapedNotReached` fails after a write with no published outcome times out with its writer alive, skips exit-send, kills and reaps |

The negative handoff trace is replayed through the public client by the held-tail
tests. The original source produces semantic assertion failures for both
premature single/sequential completion and a live-tail request returning a
response instead of `TimeoutError`. Standard queue/thread barriers establish
ordering; no sleep is used as ordering evidence. The test's release-on-wait
controller permits bounded handoff to complete and rejects unbounded joins; it
does not assert a production join count. All held workers have independent
fixture release/join cleanup even after an assertion fails.

## Commands

From repository root:

```powershell
python -m unittest scripts.test_light_whisper_cli -v
python formal/cli/run_models.py --jar $env:TLA2TOOLS_JAR --output tmp/cli-model-evidence
python -m coverage run --rcfile formal/cli/coverage.ini -m unittest scripts.test_light_whisper_cli
python -m coverage combine --rcfile formal/cli/coverage.ini
python -m coverage json --rcfile formal/cli/coverage.ini -o tmp/cli-coverage.json
python -m coverage report --rcfile formal/cli/coverage.ini
```

Set `TLA2TOOLS_JAR` to the existing local pinned `tla2tools.jar`; the runner
records its SHA256 and uses one TLC worker with a 512 MiB JVM heap. Use a new
output directory for each model matrix run so prior receipts remain intact.

Coverage requires a task-local reviewed `coverage.py`. Keep whole-file reports
for the production CLI, CLI builder, and both native Python adapters, including
unexecuted source. Report the changed adapter constructors separately from their
whole modules. Windows hiding is runtime-tested on Windows; other OS branches
remain unmeasured until run there.
For parent and cleanup-probe subprocess coverage, set `COVERAGE_CORE=ctrace`,
`COVERAGE_PROCESS_START` to the absolute `coverage.ini` path, and `COVERAGE_FILE`
to an external evidence path. Add `formal/cli/coverage_startup` and the task-local
coverage installation's site-packages directory to `PYTHONPATH`. The bootstrap
uses coverage.py's supported `process_startup` hook; no global `.pth` file is
changed. Clear startup environment overrides before combining/reporting. The
parallel datasets include real production execution in the cleanup probe workers.
Mutation configuration targets the same complete module. Run Cosmic Ray only on
a disposable production/test copy after its unmutated baseline is GREEN, using a
task-local environment. Use the reviewed Cosmic Ray and coverage installations;
the executed receipts identify their exact versions and hashes.
The [context-selection procedure](ContextSelection.md) describes collection,
input bindings, conservative test selection and the three current mutation
configurations. A missing or stale dataset selects the complete current cohort.

```powershell
cosmic-ray baseline formal/cli/cosmic-ray.toml --session-file tmp/cli-mutation-baseline.sqlite
cosmic-ray init formal/cli/cosmic-ray.toml tmp/cli-mutation-session.sqlite
cosmic-ray exec formal/cli/cosmic-ray.toml tmp/cli-mutation-session.sqlite
cosmic-ray dump tmp/cli-mutation-session.sqlite > tmp/cli-mutation-mutants.jsonl
cr-report --all-mutations --no-show-output --no-show-diff tmp/cli-mutation-session.sqlite
cr-rate tmp/cli-mutation-session.sqlite
```

Create `tmp` before these commands and run them from the disposable copy's root.
Clear inherited `COVERAGE_*` and coverage-bootstrap `PYTHONPATH` settings before
launching the framework. The tracked runner also removes those settings from its
test child. It uses the current interpreter and fail-fast unittest execution.
The default cohort includes CLI, packaging, measurement and native constructor
regressions. The separate build and SDK configurations specify their test
cohorts explicitly. SDK mutation selection uses the complete constructor AST
ranges and preserves framework work-item identities; other SDK methods remain
outside this mutation scope.

Use the timeouts in the selected configuration and validate them against its
unmutated cohort before execution. Each suite writes stdout and stderr directly
to separate disk logs, so a leftover pipe writer cannot strand the framework's
`communicate` call. On Windows, the runner assigns the captured child to an owned
kill-on-close Job Object. Closing that handle releases its descendants on normal
completion and failures. This does not prove containment if the runner crashes
between process creation and Job assignment. POSIX uses an owned process group;
that path remains unverified. The fixture independently tracks real backend
`Popen` objects and reaps them after assertions, including ordinary failed-test
exits.
Interrupts and unexpected errors while waiting for an acquired test process use
the same bounded cleanup, preserve a receipt, and re-raise the original error.
Secondary cleanup failures remain visible in `cleanup_error` without replacing
that original error. Repeated asynchronous interrupts are not covered by the
bounded probe evidence.

Every runner invocation preserves a JSON receipt and disk logs under
`tmp/cli-mutation/logs`. A framework timeout alone does not prove a behavioral
kill. Keep wrapper timeouts and containment errors separate, replay timing
candidates with a sufficient suite budget, and inspect their individual results.
Benchmark the unmutated suite and valid cumulative-delay variants after any test
change before the final fresh full run. Preserve generated, killed, survived,
invalid, pending and timeout counts; equivalents require source-backed review.

## Desktop admission source correspondence

The desktop-admission review used `scripts/light_whisper_cli.py` SHA256
`d1a9f86c0bcc3419ad7f1b1872a24a7e44d11bb552bb0860eda34f67884ac09b`.
Relative to `1517896105d1f5f0b0fe7acb0aa430b4f597c0be7ac7268bf380025e74daf504`,
the only executable additions are `DESKTOP_REQUEST_MAX`,
`validate_desktop_request` and its two app-only caller preflights. Every existing
function except `main` and `run_session`, including the entire `EngineClient`
class, has an identical AST. This is a reviewed source correspondence boundary,
not a new proof receipt or a composition/refinement theorem.

The current reviewed CLI SHA256 is
`db2655d9beecc41de1b20338cb44c6af64dae72701d833b5459639d0cc55d9b7`.
Its only AST change from that admission review is `app_path`. The installed
command can find its parent desktop executable and does not select itself.
Admission, numeric validation, client ownership and cleanup are unchanged.
The [command-entry contract](../cli-command-entry/README.md) owns that separate
discovery and installation boundary. Historical receipts keep their source hashes.

`main` validates assembled fields with internal ID 1 before `request_client`.
`run_session` validates after `session_request`, inside per-record `ValueError`
recovery, before lazy acquisition and `client.request`. The candidate ID is
`client.sequence + 1`, or 1 before acquisition. Rejection neither writes nor
increments that sequence, preserves an existing owner and the opaque caller ID,
and continues. Local `exit`, `doctor` and the independent engine bypass this
preflight. The helper rejects invalid scalar strings through strict UTF-8
encoding, then counts the unchanged ASCII-escaped wire message plus the platform
text newline against 65536 bytes. See [DesktopAdmission](DesktopAdmission.md)
for the finite size, Unicode, newline and ID-width guards.

| Existing model | Correspondence after this change |
| --- | --- |
| `Configuration` | `main -> resolve_engine_selection` still precedes dispatch and the new guard. Configuration decisions are unchanged. An app payload must additionally pass admission before acquisition; this model's selection predicates do not prove that payload guard. |
| `Bootstrap` | Resource, path, capability, cache and stdin-decoding decisions are unchanged. Rejected app payloads skip `request_client` and its later app-path/acquisition work; the model's dispatch cases assume an admitted payload. Invalid scalar/size classification belongs to `DesktopAdmission`. |
| `EngineClient` | The whole class is AST-identical. A rejected caller record invokes neither `__enter__` nor `request`; startup/correlation/deadline/reap obligations apply when those boundaries are entered. No preflight guarantee follows from this model alone. |
| `Session` | The new app guard extends the recoverable per-record `ValueError` branch. Its rejection maps to `ReadInvalid`/`bad_fields`: one caller-correlated failure, no `Acquire`/`Dispatch`, no wire increment, retained live owner and later input. The abstract invalid class does not calculate the new scalar/byte predicate; `DesktopAdmission` supplies that separate contract. |
| `TargetRouting` | `resolve_target` still runs first. App `Launch`/`Request` cases now assume payload admission; invalid payload rejection occurs before broker ownership. Engine routing, environment/data boundaries and doctor are unchanged. Payload rejection itself is outside this model's routing inputs. |
| `NativeThreads` | Parser/`positive_threads`, independent-engine environment construction and SDK selection/allocation are unchanged. App broker acquisition occurs only after payload admission; this model proves thread decisions under its own inputs, not that admission guard. |
| `CommandSurface` | `Validate` continues to represent command/schema/target admission. Its admitted hot-word and other app-payload representatives are scalar and fit the wire cap. New scalar/size failures refine the recoverable invalid-record boundary before `Dispatch`; their exact predicate, live-owner preservation and ID-width decisions are checked separately by `DesktopAdmission`. |
| `SenderHandoff` | `_send`, `request` and cleanup are AST-identical. Rejected input starts no writer and never enters this model's ready-request path; successfully admitted requests retain the existing sender ownership/deadline contract. |
| `UnicodeSerialization` | `Encode`/`Pipe`/`Decode` still describe unchanged `_send` for strings admitted to the direct client or independent engine, including escaped unpaired surrogates. App callers reject unpaired surrogates before that path and admit scalar strings only within the physical cap. The serialization theorem proves neither native serde acceptance nor either new admission guard. |
| `json-numbers/JsonNumbers` | `_receive`, finite-number validation and client cleanup are AST-identical. The model begins with an admitted first request; rejected app input never reaches startup/action response admission. Numeric failure remains terminal after ownership cleanup, distinct from recoverable caller input errors. |

The native `agent-api-transport` companion also records the CLI source hash.
Its broker schema, request-size reader, deadline and authentication code are
unchanged. It still validates direct native ingress independently of the Python
caller; it does not establish Python pre-acquisition rejection. Its current
receipt must bind the new CLI content after replay.

All existing model/configuration bytes, properties, controls, witnesses and case
counts are preserved. Historical receipts retain their original source hashes.
Fresh current-source matrix execution, assembled regression, coverage, mutation
and native/artifact evidence remain separate gates; this correspondence review
does not turn historical execution into a new result.
