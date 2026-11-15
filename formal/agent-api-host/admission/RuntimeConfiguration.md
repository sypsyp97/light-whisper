# Saved runtime configuration during lifecycle admission

This separate prefix covers `engine_start` and `engine_restart` through lifecycle
transfer to the common startup call. The explicit
`engine_set` argument remains in the unchanged Stage A ControllerAdmission model.
Both runtime actions must sample and validate saved configuration while holding
the lifecycle gate, before restart-stop or transfer to common startup code.
The existing initial saved-value read/allowlist check remains: an initially cloud
engine rejects before acquiring lifecycle. A valid initial sample is read and
validated again after acquisition, ahead of recording/ASR/startup admission.

The actual controller regressions are
`controller_lifecycle_admission_engine_start_rechecks_saved_configuration` and
`controller_lifecycle_admission_engine_restart_rechecks_saved_configuration`.
They invoke real `controller::dispatch` in isolated windowless Wry. While holding
the lifecycle gate on which dispatch is pending, an actor acquires the actual
`lock_idle_asr` lease, writes the owned configuration to `glm-asr`, and calls the
real `stop_server` on its owned synthetic native child. The test waits for that
exact child to be reaped, publishes ready=false/inline=None, and releases ASR
before lifecycle. This follows the GUI writer's ownership/stop protocol without
calling the full GUI command, keyring, cloud readiness validation or a provider.
An additional process-mutex guard contains the resumed legacy request before
child stop, discovery, download or spawn. The tests require an explicit
`invalid_request` and preserve the actor's files, readiness, generation, profile,
native state and event/audit frame. The comparison frame is captured after the
actor's stop, including its generation advance and IPC audit. A wrongly admitted
CLI restart would add another generation advance (1 to 2). No sleep establishes
ordering, and no cloud key or positive readiness is fabricated.

This schedule depends on the Stage A ordering change. In the original controller,
the CLI owns native_asr_owner while awaiting lifecycle; the real GUI set_engine
must acquire that ASR lease before writing and therefore cannot produce this
overlap. Under lifecycle-before-ASR, the waiting CLI owns no ASR lease and the GUI
writer can complete. The snapshot negative represents a Stage A-only candidate
that retains the old early configuration sample. The controlled publication tests
must not be reported as reproducing this full GUI overlap on the original source.

The model stores two local identities, one cloud class and one raw unknown
configuration partition. `paths::configured_engine` normalizes unknown/missing
raw values to qwen3-asr-0.6b, represented by localA. Unknown is therefore accepted
as that default local engine; it is not treated as a new invalid controller input.
The legacy qwen3-asr-1.7b alias belongs to the localB/r2t2 class.

The configuration writer may change saved configuration once, only while it owns
the pre-existing lifecycle gate and the request is pending. It must acquire ASR
before writing, settle its own stop/reap, publish false readiness, and release
ASR before lifecycle. No third ASR owner is present in this bounded partition;
the Stage A request has not acquired ASR while pending on lifecycle. Unknown is
an initial raw-file partition; the GUI actor writes only allowlisted local/cloud
values. There are 16 initial states:
two runtime actions, four raw configurations and presence/absence of the prior
lifecycle owner. The optional change has no fairness requirement. Weak fairness
on request prefix steps, settlement of an already-started actor, eventual
external-gate release, and the CLI's separately awaited restart-stop settlement
establishes finite prefix progress, without a wall-clock bound. Successful
actor-owned stop/reap and CLI stop settlement are environmental assumptions.
The CLI stop counter is bounded by one; generation is bounded by two to account
separately for one actor stop and one possible CLI stop. No constraint, symmetry, VIEW,
depth limit or disabled deadlock check reduces the declared state space.

| Configuration | Meaning | Expected result |
| --- | --- | --- |
| RuntimeConfigurationPositive | Current configuration sampled under lifecycle; full safety/progress | Exhaustive success |
| RuntimeConfigurationLegacySnapshotNegative | Stage A guard order with the old pre-wait snapshot retained | InvalidSavedConfigurationNoEffects violation |
| RuntimeConfigurationIdentityNegative | Post-gate local allowlist bypassed | AdmittedIdentityIsCurrentLocal violation |
| RuntimeConfigurationValidationLeaseNegative | Lifecycle guard released before current validation | LifecycleHeldDuringValidation violation |
| RuntimeConfigurationRestartLeaseNegative | Lifecycle guard released at stop invocation before its await settles | LifecycleHeldDuringValidation violation |
| RuntimeConfigurationTerminalReleaseNegative | Post-gate rejection or startup transfer retains the lifecycle guard | TerminalGuardsReleased violation |
| RuntimeConfigurationActorProgressNegative | Already-started actor's stop never settles | AdmissionCompletes temporal violation |
| RuntimeConfigurationRestartProgressNegative | CLI restart's stop never settles | AdmissionCompletes temporal violation |
| RuntimeConfigurationStartChangedWitness | Local-to-cloud change while start waits, then explicit rejection/released guard | NoStartChangedConfigurationRejection violation |
| RuntimeConfigurationRestartChangedWitness | Same schedule through restart | NoRestartChangedConfigurationRejection violation |
| RuntimeConfigurationLocalChangedWitness | Different local identity sampled and admitted after the wait | NoChangedLocalIdentityAdmission violation |
| RuntimeConfigurationRestartPendingWitness | Actor settled, then CLI stop invoked while lifecycle is held and reply remains pending | NoPostActorRestartPending violation |

The five safety controls challenge read placement, the current local-engine
predicate, ownership at validation, retention across the restart await, and guard
release. The release fault is shared across post-gate rejection and both transfer
paths; each releases the same actually acquired lifecycle guard in source. The
two progress controls disable different settlement actions. Each config activates
only `PROPERTY AdmissionCompletes`, and its registry row uses `property` rather
than `invariant`; the required result is TLC exit 13 with a temporal violation.
Each temporal config places that sole singular `PROPERTY` as its final
uncommented directive, with no plural/continuation list or trailing live directive,
matching the generic checker's fail-closed typed configuration profile.
Both wait phases retain a legal self-loop, so a blocked settlement is not an
unrelated deadlock. Starting the actor remains optional; its progress control
requires a behavior where that actor has already begun. Stage A's older progress
control is not reused as sensitivity evidence for these new actor phases.

Witness failures establish reachability and are not safety-mutant kills.
InvalidSavedConfigurationNoEffects compares actual CLI-stop/generation/readiness
projections against the post-actor frame. The explicit invalid reply, admitted
configuration identity, lifecycle ownership and released guards have separate
properties. `BeginRestartStop` advances generation and enters a pending stop
state with lifecycle held and reply `none`. `SettleRestartStop` separately models
successful return and readiness clearing while retaining that guard.
`TransferRestartAdmission` then matches the explicit lifecycle drop before the
common startup call. Start transfer likewise drops lifecycle with reply `none`.
Only rejection has a returned reply in this prefix; neither transfer invents an
accepted response. `TerminalGuardsReleased` applies at rejection and actual
transfer, while `LifecycleHeldDuringValidation` also covers the pending stop.
`AdmissionCompletes` ends at those prefix boundaries; it proves no common-service
completion, child spawn, final success response or readiness acknowledgement.

Source mapping: controller.rs runtime branch reads saved configuration and checks
its local allowlist; commands/funasr.rs `set_engine` acquires lifecycle before its
write and keeps it through the switch; paths.rs `read_engine_config` applies the
normalization; funasr_service.rs `stop_server` advances generation before awaiting
the process mutex. The positive model is the test-first target. The legacy source
already contains the validated Stage A lifecycle-before-ASR change. Its cached
saved-value read still lacks the post-gate resample; that behavior remains
unmodified until accepted Stage B semantic RED permits implementation.

The corrected model has 12 configurations: one positive, five safety controls,
two temporal controls and four reachability witnesses. The rejected six-config
slice, its patch and frozen source hashes remain immutable historical artifacts.

`formal/verification.json` appends these 12 cases to the existing generic
`scripts/check_formal.py` matrix, retaining the 39 existing contracts and all 220
earlier expected log identities. No new checker or driver is added. The dedicated
`scripts/test_cli_runtime_configuration_formal.py` is discovered by the existing
CLI unit pattern. The original 39 contract identities and 220 log identities are
preserved, with 12 added RuntimeConfiguration log identities. Six separately
accepted UnicodeSerialization entries are appended from their owner's exact
registration handoff; they are independent of this model correction. Genuine
temporal-negative dispatch requires the separately test-first, root-owned generic
checker update. This slice does not edit that checker or its tests.
Syntax/static checks and registration
unit results do not establish native controller RED or TLC completion;
runtime/model execution remains subject to the coordinated grant.
