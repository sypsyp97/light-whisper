# Coverage audit

The former audit exposed recording-only assumptions, Boolean authentication,
missing lifecycle/protocol models and a vacuous subtitle-final assertion. The
registry now assigns every current command/event/lifecycle/protocol boundary to
an explicit contract, with actual source and evidence paths.

| Area | Formal contracts and implementation correspondence |
| --- | --- |
| Recording/output | Reservation, cancellation, four modes, finalization, consent, paste/restoration and disposed listeners; audio/finalize, useRecording/disposal and clipboard tests. |
| Capture/monitor | Native acceptance/fallback, worker/device identity, sample cap, detached workers and monitor generations; native tests, publication guard and disposed-effect regression. |
| Hotkeys | Three roles, conflict/backend partitions, hold/toggle, duplicates, session stops, queued starts and obsolete native/dispatch messages; per-gate owner, recording-lock bind, fresh mode gates and never-reused native IDs. |
| Engine/model management | Four engine classes, start/restart/failure, key slots/regions, downloads, busy exclusion, directory migration and frontend cancel/retry/unmount generations; scoped download events/promises, captured cancellation targets, and acknowledgement after worker termination, terminal publication and slot release; engine/native/path, settings, worker and download cancellation tests. |
| GPU idle unload | Startup/runtime enable windows, command-lock and active-stream exclusion, sticky readiness, reload and stale settings reads; two component models plus the integrated R2T2 workload. User-event tests cover blur/click ordering; CUDA speech probes check native unload/reload. |
| Authentication/providers | Browser/device/refresh/logout, partial storage, provider/key/endpoint pairing, captured authentication choices, one-session catalog credentials and storage/cache publication; vision drafts survive provider switches, writes serialize and stale or dirty-at-read responses cannot publish; coordinator/challenge/metadata/key interleaving tests, real SettingsPage interactions, eight LLM/catalog snapshot paths and provider tests. |
| Catalog loading | Context, authentication and separate-mode retirement settle the retired spinner; stale success/failure cannot publish or clear a replacement request's loading state. Real hook, vision picker and selection settings interactions cover deletion, logout, closure/reopen and current completion. |
| Settings/personalization | Version ordering, preference publication, providers/import/export, hotwords/corrections/app rules; profile/state/settings tests and unbounded Lean induction/first-match proofs. |
| Context/processing | Independent Off/On/Auto settings and app overrides, explicit operations/no-search, unknown fail-open, captured screenshots and meaning audit; routed external search/fresh screen joins, post-join foreground snapshot, and reused description regressions. |
| Assistant/selection | Cancellation/conversation/retry, search/source partitions, result ownership and exact-context replacement; retry retirement preserves newer text, phase and busy state even after same-session close. Selection disable retires capture epochs; the publication lock covers screenshot cache and actual window publication through completion. Production owner API, source review and overlay tests. |
| History | Multiple/shared records, consent, transactional deletion, retention/GC failures, export/leases/reprocessing; GC holds a SQLite write transaction through reference read, lease read and unlink. Mutation completion refreshes the visible filter. Terminal results/paste authorization precede archive work; persistence and actual paste completion settle independently. SQLite tests, percentile rank preservation and HistoryPage. Assistant stream chunks/done are excluded from that ordering. |
| Windows/UI | Startup/tray/exit, manual/session generations, terminal subtitles, theme/autostart/navigation; window/overlay/tray/capability/system settings tests. |
| Protocols/transport | Tagged JSONL replies, native streaming, cloud result/error classes, SSE timeouts/retries/cancellation; Python server and Rust protocol/transport tests. |
| Updates/validation | Actual numeric version parser, HTTPS GitHub release URL and http(s) source/provider partitions; URL/input and normalization tests. |

## Bounds and decomposition

- Integrated workloads use one recording, up to two selection contexts and one
  settings change/engine switch each; components explore deeper interleavings.
- Most components use two sessions/requests/contexts. Hotkeys additionally use
  four registrations and a representative replacement/conflict sequence; all
  three event kinds pass through the common dispatcher in separate runs.
  The registration-epoch model uses two queued events, three epochs, one mode
  change and one shortcut replacement, with both native and dispatch queues.
  The setup-failure model explores successful setup, lifecycle failure and both
  restoration outcomes, including stale-gate/orphan-registration cleanup.
  Physical native IDs remain owned by queued cleanup until the backend drains
  them; this is eventual under scheduling fairness, not instant OS removal.
- HistoryRecords uses five rows, three audio classes (two initially present), one
  reprocessor/count-one lease and bounded retention. HistoryLeaseInterleavings
  separately checks two independent holders sharing two audio identities, with
  release after source-row removal. The SQL DELETE count-one and UPDATE
  greater-than-one phases are explicit; holder/count equality is asserted outside
  the transaction. This companion does not replace the detailed base model.
- Runtime enumerates Qwen/R2T2/GLM/Alibaba, two regions/directories, two key saves,
  one download/input and bounded generations.
- Web search publication separates ownership, keyring I/O and cache publication
  for two save/load operations; zero represents deletion. Startup/lazy reads
  share the same critical section. Cache equality is asserted after operations
  settle, with failures leaving prior values intact.
- Provider authentication enumerates explicit OAuth/API-key and inferred default
  choices, two accounts, logout and a settings change. Resolved credentials are
  frozen together; logout does not recall an already captured outbound request.
- Download UI enumerates two pending requests, cancellation/manual retry,
  timer generations and effect replay. Each completion records the observable
  publication count before/after; stale completions cannot change it. A separate
  EventSubscriptionLifecycle covers recording listeners, pending registration,
  late cleanup, at-most-once unlisten and suppression of retired effects.
- Routing independently enumerates three polish/screen/search modes, two app
  overrides, four request modes and Yes/No/Unknown inputs.
- Integrated assistant preparation models external third-party search; native
  model tools run during the LLM request. Foreground checks sample identity at
  dispatch and do not lock OS focus. Reused descriptions have runtime tests.
- SettingsProfileLoad uses one History field and two reads to check initial
  failure/retry and blocked writes before successful restoration. Existing
  SettingsContracts keeps its full command/validation field partition.
- AssistantCredentialOwnership uses two edits, two provider switches, two
  independent reads, serialized real-write completion, save failure and one
  retry per edit. Read eligibility captures the dirty draft and version; a late
  read cannot become publishable merely because a matching save cleared it.
- MicrophoneAsyncOwnership uses two independent test owners, out-of-order
  success/failure, one intent retirement and stop/start/compensation phases.
  MicrophoneEffectOwners adds two mounted effect owners and the shared native
  operation queue; retired test/start cleanup must preserve the replacement.
- OutputDrainOwnership starts with dictation A processing while assistant B
  starts/records, then explores success/error/start-cancel/start-error exits and
  one foreground change. Pending items preserve session, target and text. The
  production controller replay uses actual AppState and dispatch locking with an
  injected native paste primitive; source guards check terminal scheduling.
  One finalizer per unique recording session is assumed. Pending deduplication
  does not promise global exactly-once delivery after a session has drained.
- IpcCommandDeadline separates write, flush and read with one three-tick budget;
  partial-frame cancellation invalidates the process and releases ownership.
  Actual child-pipe tests cover blocked write, progress and cancellation.
- ModelIntegrityRepair partitions absent/valid/invalid completion metadata,
  missing/large weight files, explicit repair and reinspection. A separate false
  reachability target generates the repair path; it is not a safety proof.
  Replay calls actual cache resolvers and downloader integrity/atomic-manifest
  publication on temporary local bytes, without GPU inference.
- Parameter-independent ownership/version/first-match/consent/restoration and
  decision contracts have Lean proofs. Production correspondence is reviewed
  code plus tests, not an automatic source refinement theorem.

- EngineConfigurationWrites checks four independent engine.json updates in one
  application process, including atomic replacement failure and lock release.
  The implementation regression pauses a real transaction and runs each public
  writer against it; it checks engine, model, GPU settings and directory set/reset
  while preserving unknown fields. External processes and filesystem durability
  remain outside this mutex contract.
- ThemeSynchronization checks two windows with two mounted consumers each,
  manual/system modes and delivery of the latest stored preference. Real-hook
  regressions cover stale queued storage notifications, unavailable storage,
  late mounts, full teardown/remount and delayed animation callbacks. Mount
  lifecycles, delivery liveness and pixels are outside the abstract model.
- The credential ownership companion applies independently to polish and
  assistant editors. Polish regressions additionally cover initial profile
  hydration, enabled-only startup, translation auto-enable and legacy IPC
  compatibility through the real keyring plugin with an in-memory credential
  backend. Neither enabling polish nor reading credentials writes a key.
- Engine archive completeness is checked by implementation tests, not by the
  model-cache integrity theorem. Those tests cover every declared local/shared
  dependency, malformed metadata, size/hash mismatch, path escape, duplicate
  ordering, Windows case collisions, nonregular entries and bounded DLL reads.
- Hotword matching retains the existing exact-match, edit-distance, inflection
  and alias-ranking rules. Concrete production-path regressions pin those
  boundaries and the existing latency budget; the text-matching algorithm has
  no source-level refinement proof in these workflow models.

The [2026-11-11 audit](evidence/state-machine-20261111.md) adds the following
bounded companions. Counts are distinct states in each positive configuration;
they do not measure source-branch coverage.

| Model | Positive states | Bound |
| --- | ---: | --- |
| HistoryAudioGcOwnership | 16,924 | One shared audio identity, two independent tasks and three GC attempts; separate reference/lease reads, unlink, query failure and unlink failure. |
| SelectionPublicationOwnership | 700 | Two capture workers, epochs 0/1, one disable/re-enable cycle and optional screenshots; eligibility, screenshot publication and window publication are separate steps. |
| AssistantRetryOwnership | 51 | Two retries, one retirement by new dictation or close, and an optional same-session reopen; success, failure, finally and streamed final are distinct effects. |
| HistoryMutationFilterOwnership | 29 | One polish/ASR/removed-delete/missing-delete mutation, one A-to-B filter switch and B load, followed by mutation/refresh success or failure. |
| ModelCatalogLoadingOwnership | 29 | Two requests, one context/authentication/separate-mode retirement and optional eligibility restoration; late success, failure and finally remain possible. |
| VisionCredentialOwnership | 33,362 | Two edits, two reads, two provider switches and one retry per failed latest edit; captured provider, serialized writes and dirty-at-read ownership. |
| DownloadTaskEventOwnership | 2,783 | Two request identities, two cancellation waiters per request, preparation/progress/child-wait stages, four status classes and late promise completion. |

History GC assumes initially saved paths are fresh and unique; reprocessing of
existing audio uses leases. SQLite write exclusion and native filesystem
operations remain trusted primitives. Selection runtime epochs must not wrap,
and retirement must use the same lock that covers the actual screenshot cache
and `show_selection` publication closure. Owner API tests alone do not establish
that source binding; review checks it separately. Download cancellation progress
requires weak fairness for cancellation observation, terminal publication and
cleanup, without a wall-clock guarantee. Native process, HTTP, clipboard,
screenshot and window outcomes remain external inputs.

These seven families add **22 safety negatives** and **two boundary reachability
witnesses**. The GC and selection `BoundaryNegative.cfg` configurations retain
the correct guards and intentionally violate `BoundaryCoverageNotReached` to
demonstrate a combined scenario. They are reachability evidence, not safety
negatives. Configured assertions and observed state-changing actions are bound
separately in the registry; none of these models supplies compiler refinement.

## What the gates mean

Inventory equality checks that no current registered boundary is omitted.
Reachability checks abstract actions; configured invariants/properties check
safety/progress under explicit bounds/fairness; negative controls check
sensitivity to removed guards. Source review/tests check correspondence. None
alone certifies every source branch or an external dependency.

Reachability is accepted only for the registry's explicit state-changing action
list. Properties must be configured assertions or positive named conjuncts of
assertions; negative/conditional mentions, comments and strings are rejected.
The verifier's eleven regression tests cover the binding gate and equality of
the CI/verifier TLC checksum pins. The original binding regressions reproduced
seven false passes before this gate was tightened.

Read-only getters share a snapshot contract. Validation families share a
valid/invalid partition with concrete validator tests. Folder/browser/window/
clipboard/driver operations delegate to native primitives whose success/failure
are modeled inputs; pixel geometry and foreign-app consumption are reviewed/
tested outside mathematical state claims. Learned output contents are inputs,
not a proof of model accuracy. These are explicit exclusions, never counted as
proved. New boundaries or source changes require renewed correspondence review.
