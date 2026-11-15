# ASR subscription bootstrap

Scope: the saved-engine hook's listener acquisition window. The public-hook test
uses synthetic saved configuration and deferred native installation/JS ACK; it
does not run the installed application, Tauri, CLI, inference or network.

## Source and contract

Observed failing `src/hooks/useAsrEngineSettings.ts` SHA256:
`f7f2eb15dfd9de37b061135b6338403dfa64d5e67d5bdf4ac0b0cffb3725bbb6`.
The baseline reads engine/endpoint/key in the mount effect before asynchronous
`listen("engine-config-changed", ...)` completes. Native writes that emit before
the subscription is installed are lost, leaving the older initial snapshot
visible indefinitely. Acknowledgment only saves the release function.

The desired protocol establishes the listener before authoritative hydration;
registration failure reports a diagnostic and hydrates once as fallback. A
delivered event may already own hydration before the subscription promise ACK:
its newer generation supersedes the mount's bootstrap. Unmount retires the mount
and releases an acquired listener, including ACK that arrives after disposal.

The model's positive protocol matches the reviewed fixed hook SHA256
`a84e86e921f9c3ba5d8432e19c771c1956963af6f5340044de28622dac57b9fe`.
`LegacyEagerReadNegative.cfg` preserves the original baseline mismatch.
The fixed mount defines `hydrateInitialConfig` with the existing captured-version
`current()` guard, installs the listener, and calls hydration from successful
subscription ACK or the visible failure fallback. The pre-ACK event callback
increments `configVersion`, so ACK cannot start a duplicate obsolete bootstrap.
Late successful registration calls its own release function after disposal;
cleanup retires the mount and releases an already acknowledged listener.
This correspondence review covers the mapped engine protocol below. The runner
records the source it reads; a hash alone does not establish correspondence or
replace native/UI evidence. [Source negative controls](SourceControls.md) bind
the fixed source's mechanisms to the preserved seven public-hook tests.

| Source boundary | Model action/state | Required property | Public-hook schedule |
| --- | --- | --- | --- |
| `SettingsPage` calls `useAsrEngineSettings` | `Init` | configured type/provenance invariants | actual `renderHook` of production hook |
| Tauri `event.listen`: `invoke('plugin:event\|listen')`, then release closure | `NativeInstall`, `JsAcknowledge` | `SettledSavedConfig`, `LatestSavedEventually` | saved change before native install; old snapshot completes before install or after ACK |
| Native `agent_api/controller.rs` engine-set writes saved config, awaits stop, then emits; `commands/funasr.rs` has the same write/emit separation | `NativeWrite`, `NativeEmit` | `PublicationNeverRegresses`, `LatestSavedEventually` | mock saved closure; event delivered only after native install |
| `readAsrConfig`: capture `configVersion`, invoke getter, settle with mount/version fence | `RequestRead`, `Snapshot`, `Complete`, read generation/value/revision | `PublicationGenerationOwned`, `PublicationProvenance`, `ReadRequestsSettle` | old snapshots resolve after newer event hydration |
| Event callback increments `configVersion` and rereads saved config | `NativeEmit` when installed and mounted | `PublicationGenerationOwned`, `SettledSavedConfig` | native event before JS ACK; ACK must preserve the newer event's hydration |
| Listener promise rejection logs `console.error` | `RegistrationFails`, diagnostics count, fallback request | `FailureVisible`, `FallbackEventuallyUsable` | registration rejects; saved configuration remains usable |
| Effect cleanup and late subscription completion | `Unmount`, late `JsAcknowledge`, release count | `NoReadAfterDispose`, `ListenerReleasedAfterAck` | unmount before ACK; invoke queued callback; exact release and no new config reads |

Endpoint/key readiness is checked in the public hook tests. The finite model
abstracts one independently read field (engine) and its actual saved value and
revision; it does not prove a multi-field native transaction. Saved Alibaba model
and catalog fetches, GUI selection, debounced writes and transition ownership are
separate cohorts. Keeping their old initial fetch is outside this model.

## Bounds and assumptions

Three positive configurations cover exact-count workloads with zero, one, and
two sequential external writes/emissions. Each checks all eight safety
invariants and all three temporal properties. The zero-write workload can stay
idle after bootstrap indefinitely; the one-write workload stops after its sole
write/emission; the two-write workload stops after its second. Their union covers
finite workloads with up to two changes, including all enabled interleavings
and safety prefixes in each. These profiles impose no requirement that a user
make another change to repair hydration.

`MaxWrites` is the exact final write count for a selected profile: weak fairness
of `NativeWrite` and `NativeEmit` makes that profile's scheduled writes/emissions
finish. It does not cover earlier stopping histories as temporal behaviors of
that same profile; those are the separate lower-count profiles. Read bounds are
one, two, and three respectively: one bootstrap/fallback plus the profile's
delivered invalidations. Writes follow Qwen -> R2T2 -> Qwen, retaining the
two-write ABA case. The legacy lost-event negative and recovered-event witness
use the one-write profile, whose final engine is visibly R2T2 rather than Qwen;
their result cannot depend solely on ghost revision differences after ABA.

Read invocation, native snapshot and JS completion are separate actions. An
invocation may snapshot after another native write. Installation and JS ACK are
separate actions; an event can arrive between them. Native emit follows its
write; another modeled write follows that emit. Concurrent overlapping external
writer transactions are outside this bound.

Unmount is optional and irreversible for this single mount. Late snapshots still
complete; stale generations cannot publish. The default deadlock checker remains
enabled. `Idle` is an explicit terminal self-loop. There are no state/depth
constraints, symmetry, `VIEW`, or disabled deadlock checks. Weak fairness applies
only to eventual native registration success/failure, successful registration's
ACK, the selected fixed-count workload's writes/emissions, and each started
snapshot/completion. For the zero-write profile, write/emit fairness is vacuous
because both actions are disabled from initialization onward.
These are environment termination assumptions, not guarantees about an
unresponsive native process. Unmount is not forced. Progress properties explicitly
allow disposal and do not demand ongoing invalidations after registration failure.

Each safety negative checks its named invariant on the same model with one
mechanism changed. `GuardDispose = FALSE` removes the complete bootstrap
mount/version ownership check, corresponding to unguarded ACK hydration.
The temporal negative retains `LatestSavedEventually` as its only configured
temporal property and removes ACK hydration. The fallback negative similarly
removes failure-path hydration while retaining `FallbackEventuallyUsable`.
`ReadRequestsSettle` checks the explicit snapshot/completion fairness assumption;
it introduces no separate production progress mechanism. A nonzero exit is accepted only
with the intended TLC violation and counterexample text. Witness runs seek
reachability; their expected invariant failures are not safety-mutant kills.

| Config | Kind | Required outcome |
| --- | --- | --- |
| `AsrSubscriptionBootstrapZeroWrites.cfg` | positive, exactly 0 writes / 1 read slot | exhaustive safety and three progress properties pass |
| `AsrSubscriptionBootstrapOneWrite.cfg` | positive, exactly 1 write / 2 read slots | exhaustive safety and three progress properties pass |
| `AsrSubscriptionBootstrap.cfg` | positive, exactly 2 writes / 3 read slots, ABA | exhaustive safety and three progress properties pass |
| `LegacyEagerReadNegative.cfg` | safety negative, exactly 1 write | `SettledSavedConfig` violated with final saved R2T2 |
| `GenerationFenceNegative.cfg` | safety negative | `PublicationGenerationOwned` violated |
| `DisposedHydrationNegative.cfg` | safety negative | `NoReadAfterDispose` violated |
| `BootstrapOwnershipNegative.cfg` | safety negative | `PublicationNeverRegresses` violated |
| `ListenerReleaseNegative.cfg` | safety negative | `ListenerReleasedAfterAck` violated |
| `FailureDiagnosticNegative.cfg` | safety negative | `FailureVisible` violated |
| `AckHydrationProgressNegative.cfg` | progress negative | `LatestSavedEventually` violated |
| `FallbackHydrationProgressNegative.cfg` | progress negative | `FallbackEventuallyUsable` violated |
| `RecoveredMissedEventWitness.cfg` | witness, exactly 1 write | `NoRecoveredMissedEvent` violated with saved/published R2T2 |
| `PreAckEventWitness.cfg` | witness | `NoPreAckDelivery` violated |
| `StaleCompletionWitness.cfg` | witness | `NoStaleCompletion` violated |
| `FallbackHydrationWitness.cfg` | witness | `NoFallbackHydration` violated |
| `LateReleaseWitness.cfg` | witness | `NoLateRelease` violated |

## Execution

Use the project's reviewed pinned TLC jar. `verify.py` checks SHA256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`,
uses 512 MiB and one worker, preflights SANY, runs configurations serially, and
writes commands, input hashes, exits, raw generated/distinct counts and logs to a
new caller-owned output directory. It does not download or install anything.
All configurations instantiate the same module; its SANY preflight covers their
shared syntax/semantics. The receipt lists every selected config hash, exact
write count and read-slot bound, including SANY-only runs. Parsing/evaluation of
the TLC configurations and their activated properties still requires actual TLC.

```powershell
python formal/asr-subscription-bootstrap/verify.py --java $java --jar $jar --output $newEvidenceDirectory
```

`--sany-only` performs no enumeration; `--case NAME` selects a configuration for
diagnosis. A selected-case receipt is partial matrix evidence. Initial writer
validation executed only SANY and the seven mocked hook tests: two intended
assertion failures and five passes on the unchanged baseline. The seventh test
records public state publications with two saved writes and a delayed second
emission. If ACK hydration publishes the second snapshot, an older event read
must never publish the first snapshot afterward. A protocol that skips the
superseded ACK read remains valid and can publish the second save upon its event.
TLC enumeration,
source mutation, measured coverage and real native/UI verification remain OPEN
until independently executed on the reviewed assembly.
