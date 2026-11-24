# Controller engine lifecycle admission prefix

This supplementary model follows actual `controller::dispatch` engine_set admission
through rejection or the first configuration write. The original AgentHost model,
runner, native fixture and twenty-case application driver stay byte-identical.
AppQuery's Wry/watch exclusion remains explicit. This is an additional prefix
contract, not a replacement lifecycle or transport model.

The original authored Rust case is
`agent_api::tests::controller_engine_set_admission_rejections_preserve_owned_state`.
It added one current-thread, isolated, windowless Wry dispatch test to query25.
The historical query26 source SHA256 was
`f4befa7e12565efb2bc4ba5e0419ba8c568e994a6932ffb2c714239c3507035d`;
that suite passed 26 cases plus two ignored native fixtures. Those receipts remain
historical. The current query34 cohort includes the six lifecycle-publication/
cancellation cases and two saved-configuration revalidation cases. All 34 current
native tests pass with zero failures; two fixture bodies remain ignored. The
current identities are recorded in `source-bindings.json`:

| Current input | SHA256 |
| --- | --- |
| `src-tauri/src/agent_api/controller.rs` | `46f5a457464422f9117e01a8d9679578ef4ae433e17dc5b0616c7f77c2cefb7f` |
| `src-tauri/src/agent_api/tests.rs` | `1d66dced5f4539e87fd3cd51e2869083ae36e59e58d08d345be0ecd47e5afcac` |

The current 32-case admission matrix also passes its expected semantic outcomes.
Raw native/formal receipts remain external; fresh LLVM and complete source
mutation measurements remain separate gates.

The new tests invoke actual `controller::dispatch` in isolated, windowless Wry.
Two cases publish a recording claim while engine_set/engine_restart await the
actual lifecycle gate. Three publish a nonzero startup owner while engine_set,
engine_restart or engine_start await that gate. A sixth cancels all three at
the pending gate and checks released request ownership. The recording actor uses
the actual `RecordingSlot::Starting` plus snapshot publication under the real
recording/lifecycle mutexes, matching audio.rs 103–159. It is an explicitly
controlled state-publication seam, not microphone capture or live GUI evidence.
The startup actor uses `try_begin_funasr_start`, matching start_server 283–326.
The recording cases retain an actual owned synthetic native child.

Before releasing lifecycle, each claim test holds the actual process mutex as a
containment guard. A single resumed controller poll must return structured busy.
A wrongly admitted baseline can expose its configuration write/generation change
and become Pending at that mutex. The test captures state and drops the controller
before releasing the process guard, preventing stop, discovery, download or spawn.
No sleeps or timeout outcomes establish the intended schedule.

## Source observations and effects

The model samples the initial watch borrow, requested-engine allowlist,
recording/processing inputs, try-ASR-lease result, starting flag, and DownloadTask
value at their actual source boundaries. A receiver created with initial true
has already seen that version: the initial borrow is necessary even when
`changed()` is not ready. The biased watch branch is checked before each request
poll, including resumption from lifecycle/download-slot mutex waits.

A watch change after that poll's check can race the remaining synchronous prefix.
It can precede a configuration write without being observed in that poll. The
model therefore retains an explicit witness for unobserved retirement followed by
a write. It proves no stronger global cancellation barrier. DownloadTask is read
after acquiring lifecycle ownership; one optional start/finish can occur while
that acquisition is pending, or between later source observations. Guard
properties use the sampled value, rather than assuming a later external change
was already observed.

The seven optional Boolean observations use a uniform string encoding:
`"unread"` represents no sample, and `"true"` / `"false"` represent sampled Boolean
values. `Observe` encodes each actual source read; every guard property and
witness compares the corresponding encoded value. This is a bijection from
`None` / `Some(true)` / `Some(false)`, preserving the original sample boundaries,
sticky observations and rejection effects while avoiding TLC comparisons between
strings and Booleans. `observed.changed` remains an actual Boolean accumulated by
the watch poll.

`ChangeDownload` binds to `download_service.rs`: task publication at 142–154,
clearing at 71–75 and the clear call at 286. These writers use the same
`download_task` mutex that the controller later reads. The source SHA256 is
`781d29dc25582ca45c67f7f0addadb3a7b4580584b855a9dd9957a6bbefcab21`.
This freezes an environmental-input dependency; it adds no mutation-cohort file.

The real ASR-free startup partition has a nonzero startup owner, no accepted or
starting child, false readiness and absent inline transport. It corresponds to
common `start_server` preparation before find_engine/child acceptance. This does
not claim all CLI startup phases are lease-free. Processing and external ASR
ownership stay fixed during this bounded call. Recording and startup actors may
publish once while they own a lifecycle gate on which the request is pending.
The recording actor requires a live, ready native process and no existing
recording/processing/startup owner. The startup actor requires process absence,
false readiness and no existing recording/processing/startup owner. Publication
precedes release of the actor's lifecycle guard. Neither actor is fair-forced;
neither can publish after admission in this model. The owner-watch, mutex
settlement and download-slot overlap schedules remain unchanged.

The positive prefix is the test-first target: validate engine, acquire lifecycle,
then read recording/processing, acquire ASR, read startup and inspect download.
`Wrong = "pre_lifecycle_admission"` preserves the original controller order,
which reads those admission guards before awaiting lifecycle. The current source
binding places lifecycle acquisition at controller line 279 before
recording/processing/ASR/startup admission at 288–291. The separate
RuntimeConfiguration prefix maps saved-value revalidation at 280–287 for
engine_start/engine_restart; explicit engine_set remains this model's scope.
The lifecycle guard is dropped at 307 before reentrant start_server at 308.
The current native regression and this admission matrix have executed against
their recorded bindings. Their success does not establish fresh LLVM coverage,
framework mutation outcomes or the separate RuntimeConfiguration replay.

`LeaseRetainedBeforeWrite` now begins at successful lease acquisition and applies
through every remaining prefix state. It no longer demands a lease before its
acquisition, while retaining the entire post-acquisition ownership promise.
The `release_lease` control drops that real ownership at the download-slot await,
rather than the earlier lifecycle await. `LifecycleRetainedBeforeWrite` likewise
starts at actual lifecycle acquisition, including the intervening admission reads.
Both use sticky acquisition history and still reject release/reacquisition gaps.

No-effects invariants compare actual configuration/native/write/stop/generation/
event/readiness/transport frames with the initial frame when a blocking input was
observed. They do not rely on a declared rejection/verdict flag. Structured reply
and returned guard ownership have separate checks. Faults bypass source guards
or change actual ownership/reply behavior; they do not directly corrupt a proof
predicate.

The admitted effect projection uses a successful owned configuration write;
fallible filesystem outcomes remain outside these admission-guard partitions.
First write marks admission transfer. Optional common continuation actions then
record stop invocation/generation advance, native settlement/readiness clearing,
and event emission as separate steps. There is no fairness on those projections,
no atomic write-stop-event assumption, and no claimed post-commit cancellation or
completion proof. The [existing host model](../README.md) owns that lifecycle.

## Frozen matrix and negative controls

The positive configuration activates all seventeen safety invariants, including
TypeOK, and admission completion. It explores all interleavings in the fixed
bounds. The complete inventory contains 32 cases: one positive, sixteen named
safety controls, one progress control and fourteen reachability witnesses.
Sixteen safety cases are not sixteen independent source mechanisms: the two
ASR-retention cases intentionally exercise one fault against different properties.

| Case | Source mechanism / changed behavior | Required violation |
| --- | --- | --- |
| InitialBorrowNegative | controller 231 initial true borrow bypass | InitialShutdownNoEffects |
| ChangedWatchNegative | controller 234–236 biased changed-watch selection bypass | ObservedShutdownNoEffects |
| EngineGuardNegative | controller 272–276 local-engine validation bypass | InvalidEngineNoEffects |
| RecordingGuardNegative | lock_idle_asr recording predicate bypass | RecordingNoEffects |
| ProcessingGuardNegative | lock_idle_asr Processing predicate bypass | ProcessingNoEffects |
| LeaseGuardNegative | failed native-ASR try-lease admitted | LeaseUnavailableNoEffects |
| StartingGuardNegative | controller 289 startup predicate bypass | StartingNoEffects |
| LifecycleGuardNegative | controller 279 lifecycle ownership skipped | LifecycleHeldAtWrite |
| DownloadGuardNegative | controller 293 active DownloadTask admitted | DownloadNoEffects |
| LeaseRetentionNegative | acquired ASR ownership released across download-slot Pending | LeaseRetainedBeforeWrite |
| LeaseAtWriteNegative | same ASR-retention fault, checked at first write | AsrHeldAtWrite |
| LifecycleRetentionNegative | lifecycle ownership released across download-slot Pending | LifecycleRetainedBeforeWrite |
| RejectedGuardReleaseNegative | early return retains actually owned guards | RejectedGuardsReleased |
| ReplyNegative | structured failure loses its actual result code | RejectionMatchesObservedGuard |
| LegacyRecordingAdmissionNegative | original guard-before-lifecycle order admits a recording published during the await | RecordingClaimDuringWaitNoEffects |
| LegacyStartupAdmissionNegative | original guard-before-lifecycle order admits a startup owner published during the await | StartupClaimDuringWaitNoEffects |

The progress control disables settlement of pre-existing internal mutex owners.
Only that control activates the named AdmissionCompletes temporal property;
waiting remains a legal self-loop, so a missing settlement must fail progress
rather than an unrelated deadlock check. The positive assumes weak fairness of
internal prefix steps and both mutex owners' settlement. It does not force owner
shutdown, download start/finish, inference success or any user action.

Witnesses require initial-true/version-already-seen rejection, observed-watch
rejection, invalid/recording/processing/lease rejection, ASR-free startup rejection,
initial active download rejection, download starting while lifecycle is pending,
shutdown during a pending mutex, an accepted common-effect path and the allowed
unobserved-retirement/write race. They activate negated reachability invariants;
an expected counterexample establishes reachability, not a failed product check.
The two added witnesses require recording/startup publication during lifecycle
Pending, observation of that published owner, structured busy rejection and both
request guards released. Their new no-effects properties compare the actual
effect frame, rather than assuming a rejection flag prevents writes.

Bounds remain one request, two local engine identities plus invalid input, three
busy partitions, one external ASR owner, one prior owner of each mutex, one
optional false-to-true watch change and one optional download flip, with one
optional recording claim or startup-owner publication during lifecycle Pending.
There are still 864 initial input combinations after the startup/native-state
correspondence rule. The current positive run generates 51,448 states, finds
20,476 distinct states and completes at depth 19 with an empty queue. There is
no VIEW, symmetry, state/action constraint, depth limit or disabled deadlock check.

## Framework mutation correspondence

Model sensitivity controls and source-framework outcomes remain separate.
The historical generator inventory below predates the current controller
binding; its coordinates are retained as historical evidence. Current official
discovery contains 58 controller entries and 412 raw entries across the full
native scope. The reviewed plan selects 396 safe native mutants across ten
execution contexts; framework execution remains pending. The historical examples
below are not current source coordinates or completed mutation outcomes:

```text
controller.rs:231:5: replace dispatch -> Value with Default::default()
controller.rs:245:5: replace failure -> Value with Default::default()
controller.rs:272:16: delete ! in dispatch_request
controller.rs:280:30: replace match guard !state.engine.is_funasr_starting() with true in dispatch_request
controller.rs:280:30: replace match guard !state.engine.is_funasr_starting() with false in dispatch_request
controller.rs:280:30: delete ! in dispatch_request
```

The first two remove structured replies and can be detected without causing
native effects. The validation negation affects both valid and invalid input
partitions; EngineGuardNegative isolates its invalid-admission consequence.
The starting true/delete-negation variants admit the observed-starting partition.
The always-false variant rejects accepted work and requires the maintained native
driver's accepted path, beyond the new rejection-only case.

`commands/funasr.rs:39:48` changes Processing equality and `40:18` changes the
recording/Processing disjunction to conjunction. They map to the corresponding
busy-input partitions. Four generated lock_idle_asr replacements at 35:5 construct
synthetic MutexGuard values; their compiler viability has not been measured.
The historical inventory had no isolated generated DownloadTask-condition
mutant at its then-current controller line 285; the current guard is line 293.
There is no claimed isolated generated mutation for biased watch selection,
lifecycle acquisition or Rust Drop retention. These blind spots remain explicit;
formal negative controls do not become invented framework kills.

The historical generated controller 283 action comparison can reroute engine_set
into startup/discovery. It is outside this new case's safe execution recipe. Any later
source controls require exact reviewed generator diffs and owned mirror guards.
The historical 398-mutant inventory is superseded by the current raw inventory,
not by completed execution. No controller framework result is inferred from these
formal controls. The five early-entrypoint entries remain in the raw inventory
as protected-prefix refusals; five separate restricted builds remain pending.
Fresh LLVM collection also remains pending. See the
[native measurement scope](../../agent-api-measurements/README.md) for the complete
412/396 partition and preserved historical coverage denominators.

## Current evidence and execution gate

The current SANY preflight and all 32 configurations produce their expected
semantic outcomes: one positive, sixteen safety controls, one progress control
and fourteen reachability witnesses. The positive run exhausts its queue with
51,448 generated states, 20,476 distinct states and depth 19. Witness
counterexamples establish reachability and are not mutation kills. All seventeen
source bindings match the current files, including the real audio claim writer.
The native-runtime replay receipt SHA256 is
`a391e4a28ffa2d096c095a0c46e36c7d59c18de7f6bd0569bba67408e2bbaefb`.
The pre-migration 32-case receipt SHA256 was
`2a730cb603aa1d8e8e5c6a442fcc037af00a5c9f373def2333f5f1e8a1fbdbbb`.
The earlier run recorded a zero exit and reaped its owned root process.

The historical 28-case run remains bound to its earlier model and inputs. Its
positive generated 37,376 states, found 14,092 distinct states and reached depth
19; that receipt does not cover the added actor transitions, ownership formulas
or four new cases. Historical pins and receipts remain unchanged.

Native tests confirm structured `busy` for claims published during lifecycle
waits, preservation of the actor's configuration/process/recording state and
released request guards. The owner-shutdown case cancels set/restart/start while
the competing lifecycle owner still holds its gate. Separate start/restart tests
first reproduced accepted cloud configuration or a generation advance after a
saved-engine change; both now reject with `invalid_request` before further native
effects. Their recording/startup actors and process-mutex containment are the
controlled seams described above; they do not operate live recording or a cloud
provider.

Before any SANY/TLC launch, the declarative runner checks the pinned JAR and all
seventeen source bindings, rejecting changed source pending correspondence review.
It has no case filter, uses one worker and a 512 MiB heap, writes a hash-bound
fresh external receipt and uses its own Java temporary directory. It refuses a
wrong named violation.

Two pure mocked runner tests pass after scaffold semantic RED; their synthetic
checker outputs are not formal evidence. A second helper RED exposed
the missing isolated Java temporary option before the repair.

To reproduce the admission matrix, invoke the unfiltered runner:

```text
python formal/agent-api-host/admission/check_model.py --java <java> --jar <pinned-tla2tools.jar> --output <fresh-owned-external-output>
```

Use review and the coordinated heavy-resource grant before TLC or native runtime.
Source bindings cover the frozen production cohort, actual state/path source,
current query test and unchanged component models. Raw logs, checker/tool hashes
and RED receipts remain outside the deliverable tree. Historical query24/native20
LLVM coverage (245 profiles; derived 256/322 branch outcomes) does not measure the
current query34/controller cohort. Fresh LLVM, all 396 selected native mutants
across ten execution contexts and the five restricted entrypoint builds remain
pending.
