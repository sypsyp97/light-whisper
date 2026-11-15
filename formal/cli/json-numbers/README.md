# Backend JSON numbers

Scope: backend output admission in `scripts/light_whisper_cli.py`. Client input,
saved configuration and unknown-field schemas are outside this slice. Python
integers remain exact; only nonfinite decoded floats are invalid here.

Observed failing CLI source SHA-256:
`a6ba60b29646ad93a56a249b516451f50458c983e20286b0fab104df7ada892c`.
The source CLI subprocess reproduced nested `NaN` and lexical `1e999` in matching
startup/action replies. Startup incorrectly succeeded; action stdout contained
`NaN`/`Infinity`, exit 0. Session continued to queued records.

## Existing model gap

`EngineClient.tla` `Noise` preserves all state except `sawNoise` and cannot
represent a matching, structurally valid response with invalid numeric payload.
`StartupSuccess`, `ReceiveMatching` and `Publish` track no payload-validity bit.
`EngineClient.cfg` configures `OnlyCorrelated`, `ReadyRequiresStartup`, lifecycle
invariants and `EventuallyReaped`; none supplies the missing numeric admission.

`CommandSurface.tla` `Responses` contains `invalid_fatal`/`unmatched_fatal`;
`FilteredNoise` and `Receive` abstract those as ignorable envelopes followed by
matching success. `CommandSurfaceFilteringNegative.cfg` uses `AcceptFatalNoise`
to challenge `FatalNoiseFiltered`. Matching nonfinite frames must instead fail
before consuming the queued valid reply, so mapping them to that class is wrong.

`Session.tla` `Complete(ok)` forwards an abstract result. `TransportFailure(reason)`
is instantiated only with `timeout`/`engine_eof` in `Next`; `StartFailure` is also
abstract. Those operators model downstream terminal cleanup, without proving
that nonfinite payloads select it. Existing models/configs remain unchanged.

## Source correspondence and runtime requirements

| Source owner / transition | Additive model | Test / observable requirement |
| --- | --- | --- |
| `_receive`: parse, structural `success` guard, wire-ID correlation | `IgnoreNoise` | Foreign-ID, bool/float-ID and unstructured nonfinite log frames remain ignored. |
| `_receive`: new validity guard after those admission checks, before fatal/return | `ReceiveBad`, `ReceiveFinite` | Nested NaN and overflow fail at startup/action; queued good reply cannot mask bad frame. Finite numbers/large integers/string literals survive. |
| `__enter__` startup exception; `main`/`run_session` context unwind | `Reap` then `FailureReply` | Exit 1, visible strict-JSON error, child and workers settled before failure output. |
| `run_session`: pending caller ID, no next record after terminal exception | `MatchingBadIsTerminal`, `CorrelationPreserved` | One caller-correlated failure; no queued stats read/dispatch and no retry. |
| `_receive`: unchanged absolute deadline | Existing `EngineClient.Timeout`; no new clock abstraction | Controlled clock test proves ignored numeric noise does not reset the deadline. |

`s.number` records NaN/overflow representatives; both map to the same decoded
nonfinite class, so numerical bit patterns and nesting depth do not create extra
model transitions. `s.noise` partitions foreign structured and unstructured log
frames. `s.externalId` abstracts arbitrary signed opaque caller IDs; wire IDs
and correlation implementation remain covered by existing transport models.
This additive model has one owned process, startup/action boundaries and one
queued session operation. The startup owner has already admitted that first
valid record. `ReadTail` abstracts successful completion of the finite second
operation. Cleanup internals, scheduler/pipe time budgets and machine crashes
are outside this model and remain covered by maintained transport models.

## Matrix

All configs select `JsonNumbers.tla`. Positive `JsonNumbers.cfg` activates every
listed safety invariant plus `EventuallyDone`. `LegacyAcceptNegative.cfg` and
`SkipBadNegative.cfg` must violate `MatchingBadIsTerminal`; the latter represents
strict-decoder skip followed by the already-queued good matching reply.
`LegacyOutputNegative.cfg` activates `StrictOutput` against `LegacyAccept` and
must expose the action reply containing the invalid numeric payload.
`ValidateNoiseNegative.cfg` must violate `IrrelevantFramesIgnored`.
`PublishBeforeReapNegative.cfg` must violate `FailureAfterReap`.
Startup/Action/Noise/Finite witnesses must violate `WitnessNotReached` by reaching
their named state. A parser error, unrelated invariant, deadlock or timeout is
not a successful negative or witness.

The complete matrix has 15 configs: one positive, nine safety negatives, one
progress negative and four witnesses. Explicit sensitivity for every configured
behavioral invariant/property is bound below; `TypeOK` is a typing obligation.

| Property | Named control / mode | Source transition challenged and required outcome |
| --- | --- | --- |
| `StrictOutput` | `LegacyOutputNegative.cfg` / `LegacyAccept` | `_receive` returns a matching nonfinite action result; violate `StrictOutput`. |
| `MatchingBadIsTerminal` | `LegacyAcceptNegative.cfg` / `LegacyAccept`; `SkipBadNegative.cfg` / `SkipBad` | Missing numeric guard or decoder skipping the bad matching frame admits startup/action or queued valid result; violate `MatchingBadIsTerminal`. |
| `BadReplyVisible` | `HiddenFailureNegative.cfg` / `HideFailure` | `main`/`run_session` suppress pending terminal failure after context cleanup; `FailureReply` reaches done without reply and violates `BadReplyVisible`. |
| `FailureAfterReap` | `PublishBeforeReapNegative.cfg` / `PublishBeforeReap` | Terminal output runs before context cleanup; violate `FailureAfterReap`. |
| `CorrelationPreserved` | `ReplyIdentityNegative.cfg` / `WrongIdentity` | `run_session` response expansion loses opaque pending caller identity; `Reply` emits the other signed ID and violates `CorrelationPreserved`. Existing `SessionIdentityNegative.cfg` / `ReplyWireId` / `ClientIdentity` and `CommandSurfaceIdentityNegative.cfg` / `WrongReplyIdentity` / `ReplyIdentity` challenge the same public wrapper independently. |
| `IrrelevantFramesIgnored` | `ValidateNoiseNegative.cfg` / `ValidateNoise` | Numeric rejection precedes structural/correlation admission and fails a foreign/log frame; violate `IrrelevantFramesIgnored`. |
| `FiniteFramesContinue` | `FiniteContinuationNegative.cfg` / `FiniteStopsEarly` | `run_session` retires after a valid finite first result, leaving queued stats unread; violate `FiniteFramesContinue`. |
| `CleanExit` | `CleanExitNegative.cfg` / `PublishBeforeReap` | Same concrete early failure-publication guard as `FailureAfterReap`, checked separately at terminal ownership: done still owns a live child; violate `CleanExit`. |
| `EventuallyDone` | `RejectedCleanupProgressNegative.cfg` / `StallRejectedCleanup` | A rejected matching numeric frame enters closing but owned cleanup never settles. `Reap` is disabled only after rejection, explicit stuttering prevents an unrelated deadlock, and `EventuallyDone` remains configured. Require temporal-property violation, not a safety, parser, or deadlock error. |

The local `WF_vars(Advance)` assumption does not force the disabled rejected
cleanup step in the progress control: after rejection all `Advance` alternatives
are disabled while `StallRejectedCleanup` remains a permitted stuttering step.
This produces the intended infinite rejection/cleanup state without weakening
the checked temporal property. The positive model retains enabled cleanup and
fair local progress. The four witnesses check non-vacuity separately from safety
and progress controls.

## Run the maintained checks

`check_model.py` executes every configuration and records source, tool, model and
configuration identities. Use the pinned TLA+ JAR, a Java executable and a new
evidence directory outside the checkout. Replace the capitalized paths:

```text
python formal/cli/json-numbers/check_model.py --java JAVA --jar TLA_JAR --output NEW_EVIDENCE_DIRECTORY
```

The earlier reviewed CLI source SHA-256 was
`e78cad5614d73341c93731d9b38998ee35a1019dbcb3147c13ea5a6159a74d77`.
The current reviewed source pin is
`d1a9f86c0bcc3419ad7f1b1872a24a7e44d11bb552bb0860eda34f67884ac09b`.
Its delta from `1517896105d1f5f0b0fe7acb0aa430b4f597c0be7ac7268bf380025e74daf504`
adds only app caller preflight: `_receive`, numeric guards, the whole client and
cleanup remain AST-identical. This model begins after first-input admission;
new scalar/size failures instead take the existing recoverable caller-error
branch before startup/action response processing. [DesktopAdmission](../DesktopAdmission.md)
owns that separate guard; see [companion correspondence](../README.md#desktop-admission-source-correspondence).
Advancing the runner pin follows this source review and supplies no new TLC
receipt. The execution counts below remain historical, with their original hashes.
The runner rejects source drift, incomplete matrices, unrelated negative results,
timeouts and reused evidence directories. Required CLI CI runs its fifteen unit
tests and the unfiltered matrix. Independent model and runner reviews pass.
The recorded fifteen-configuration TLC run passed: the positive case explored
852 generated and 676 distinct states to depth seven; every safety, progress and
witness control produces its required diagnostic. The progress counterexample
explores 692 generated and 516 distinct states. Its depth comes from `Progress(7)`;
TLC does not print a complete-graph depth line for this temporal counterexample.
The runner requires completed temporal checking, matching counts and empty queues
before admitting that result. Coverage, mutation and assembled application
regressions remain separate gates; old receipts cannot establish this guard.
