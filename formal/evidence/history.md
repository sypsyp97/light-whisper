# HistoryRecords formal evidence

## 2026-10-24 output before archiving

The current `AppWorkflow` safety condition is `ArchiveFollowsTerminalOutput`.
Terminal publication and paste authorization precede audio encoding and history
persistence. Raw audio is snapshotted only with consent before final ASR drains
the capture buffer. Persistence and actual paste completion then settle
independently. The implementation records successful output timing separately
and updates the exact inserted row ID, because session IDs can repeat across
application launches. Missing or deleted rows are not recreated.

All ten `AppWorkflow` workloads passed and reached all 67 mapped actions. The
Mixed workload covered 369,326 distinct states. Removing the archive publication
guard violated `ArchiveFollowsTerminalOutput` after 2,125 states.
Two reachability witnesses intentionally violate the retired
`HistorySettlesBeforePublication` condition (4,283 states) and the assertion that
history requires completed paste (19,869 states). These witnesses demonstrate
the requested schedules; they are not safety negatives or native timing proofs.

Deterministic implementation coverage lives in `history_output_tests.rs`,
`output_tests.rs`, `history_timing_tests.rs`, and `useRecordingTiming.test.tsx`.
It checks queue/native completion timing, both persistence/completion orders,
raw samples after ASR buffer consumption, row-specific updates and stale UI
ownership. `test_async_output_wiring.py` supplies source correspondence for the
terminal branches and blocking encoder; it does not execute Windows input.

## Historical: 2026-10-11 finalization overlap extension

The ordering described in this section was superseded on 2026-10-24. Its original
baseline and evidence remain below for provenance.

AppWorkflow now separates the history audio write (`audioWrite`) from ASR/LLM
processing and the history persistence attempt (`historyFinalized`). `StartLlm`
does not wait for audio I/O. Successful or failed audio writes settle before
`SaveHistory`/`HistoryWriteFailure`; terminal transcription/subtitle results and
paste authorization require the finalized attempt. Assistant stream chunks and
stream done emitted during LLM work are excluded from this ordering claim.

The source path is `audio_service/finalize.rs::finalize_recording`, from the
consent-gated spawned `history_service::save_audio` through branch-local
`resolve_history_audio` and `HistorySessionContext::persist`. Both audio errors
and persistence errors retain the existing text/result path. This is bounded
ordering evidence, not a timing or filesystem durability proof. The
`history-publish-before-audio-resolves` mutation removes the audio settlement
guard and must violate `HistorySettlesBeforePublication`.

The production percentile regression compares empty, single, even/odd, tied,
extreme and 100,000-element duplicate-heavy inputs against sorted rank selection.
It preserves the previous rounded rank definition for p50/p95 while production
uses `select_nth_unstable`. Runtime gate results and hashes are recorded in
[optimization evidence](optimization-20261011.md).

The historical HistoryRecords evidence below retains its original runtime and
baseline. Current full checks use the pinned TLC 1.7.4 and Lean 4.34.1 runtimes.

## Scope and source baseline

- Repository baseline: `d29cd45db8876bd7bcfa915ae642af3b0fcc4bbe`.
- Owned artifacts: `formal/HistoryRecords.tla`, `formal/HistoryRecords.cfg`, and this file.
- The model is test-only. No production file was changed by this slice; the existing working-tree changes in other slices were left untouched.
- TLC runtime: `/mnt/c/Users/sun/AppData/Local/Temp/light-whisper-tla2tools-1.8.0.jar`, invoked through WSL with four workers, unique `/tmp` metadirs, and `-noGenerateSpecTE`.

## Requirement and source mapping

| Behavior | Source mapping | Model mapping |
| --- | --- | --- |
| Shared audio identities and reference counting | `src-tauri/src/services/history_service.rs:188-281` defines the history and `history_audio_leases` tables. The initial fixture has rows 1 and 2 sharing `A`, and rows 3 and 4 sharing `B` (`formal/HistoryRecords.tla:33-109`). | `References`, `ReferencedOrLeasedAudioRetained` (`formal/HistoryRecords.tla:17-25`, `538-541`) require every referenced or leased file to remain present. |
| Atomic reprocess read plus lease | `history_service.rs:446-494` uses an `IMMEDIATE` transaction to read the row, upsert the lease, and commit. | `AcquireReprocess` (`153-182`) updates the snapshot and lease in one action. `ReprocessLeaseProtectsSnapshot` and `DeletedSourceStillLeased` (`552-562`) observe the protection while the source row can be deleted. |
| Delete and retention transactions | `history_service.rs:742-775` deletes the row in an `IMMEDIATE` transaction; `777-823` selects distinct audio identities, deletes expired rows, commits, then performs post-commit GC. | `BeginDelete`/`CommitDelete` and `BeginRetention`/`CommitRetention` (`306-395`) separate transaction state from post-commit GC. `DeleteTransactionIsAtomic` (`546-550`) observes the transaction boundary. A `retentionDays = 0` guard matches the source early return. |
| GC safety and query failures | `history_service.rs:687-718` checks row references and lease counts; query or filesystem failure retains the file. | `GC`/`GCQueryFailure` (`399-420`) and `ReferencedOrLeasedAudioRetained`/`GCFaultRetainsAudio` (`538-544`) model both safety paths. |
| Reprocess from a snapshot after source deletion | `src-tauri/src/commands/history.rs:156-262` reprocesses the captured row and inserts a new record with `reprocessed_from_id`; `264-293` releases the lease on both success and failure. | `AcquireReprocess`, `InsertReprocessed`, and `ReleaseReprocessLease` (`153-302`) keep the captured audio/text/origin values independent of later row deletion. `ReprocessedRowsUseSnapshot` (`564-568`) checks the inserted row. |
| Read, export, and workflow failures | `history_service.rs:319-325` reads saved audio; `commands/history.rs:93-146` covers export, saved-audio read, and result failures; `148-154` rejects assistant/edit workflows. | `AcquireQueryFailure`, `AcquireMissing`, `RejectNonDictation`, `ReadAudioSuccess`, `ReadAudioFailure`, and `ReadAudioMissing` (`185-253`) preserve failure states and leases. `BeginExport` plus success/failure/cancel (`422-439`) models the export branches. |
| Saving consent gate | `services/audio_service/finalize.rs:59-143,256-324` persists history only when enabled and schedules audio saving only when `history_enabled && save_audio`; `commands/profile.rs:208-228` stores `save_audio` as `enabled && save_audio`; defaults are in `state/user_profile.rs:181-200`. | `SetHistorySettings` and `PersistRecord` (`112-150`) enforce `saveAudioEffective = historyEnabled /\ requestedSaveAudio`; `SaveAudioGate` and `PersistedAudioHasConsent` (`531-536`) observe the gate. |

The fixture deliberately includes two records per each of two audio identities. It also reaches a no-audio persisted row, an assistant row rejected for ordinary reprocessing, a deleted source with an active lease, successful and failed audio reads, retention query failure, GC query failure, and all export terminal states.

## TLC positive result

Committed `formal/HistoryRecords.cfg` checks `FairSpec`, all nine state invariants, and `CleanupEventually`. `FairSpec` adds weak fairness only to successful `GC(audio)` actions; `CleanupEventually` is conditional on a candidate remaining unreferenced, unleased, and outside a transaction (`formal/HistoryRecords.tla:471-486`).

Exact safety/liveness command:

```text
wsl.exe bash -lc 'set -o pipefail; java -XX:+UseParallelGC -jar /mnt/c/Users/sun/AppData/Local/Temp/light-whisper-tla2tools-1.8.0.jar -workers 4 -noGenerateSpecTE -metadir /tmp/light-whisper-history-safety-$(date +%s%N) -config /mnt/c/Users/sun/Downloads/light-whisper/formal/HistoryRecords.cfg /mnt/c/Users/sun/Downloads/light-whisper/formal/HistoryRecords.tla'
```

Result on 2026-09-28:

```text
Model checking completed. No error has been found.
2004619 states generated, 414760 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 16.
Implied-temporal checking--satisfiability problem has 3 branches.
```

The `-coverage 1` replay also completed with no error and the same 2,004,619 / 414,760 / depth-16 totals. Nonzero coverage was observed for `AcquireReprocess`, `ReadAudioSuccess`, `ReadAudioFailure`, `ReadAudioMissing`, `InsertReprocessed`, `BeginDelete`, `CommitDelete`, `BeginRetention`, `RetentionQueryFailure`, `GC`, `GCQueryFailure`, `BeginExport`, `ExportSuccess`, `ExportFailure`, and `ExportCancelled`.

## Intentional negative mutation

In a temporary directory outside the repository, I copied the model and replaced the GC reference guard

```tla
/\ ~References(state, audio)
```

with

```tla
/\ TRUE \* MUTATION: reference-count guard removed
```

The corrected mutation was run from `C:\Users\sun\AppData\Local\Temp\light-whisper-history-negative-ce60923edeab4ec1abee32fb4ed7e071` with the same TLC jar, workers, `-noGenerateSpecTE`, and a unique `/tmp/light-whisper-history-negative-*` metadir. TLC produced a semantic RED:

```text
Error: Invariant ReferencedOrLeasedAudioRetained is violated.
State 3: <BeginDelete(1)>
State 4: <CommitDelete>
State 5: <GC("A")>
```

The smallest counterexample deletes row 1 while row 2 still references `A`, then the mutated GC removes `A`; state 5 has `records = {2, 3, 4}`, `gcCandidates = {}`, and `audioPresent[A] = FALSE`. The failing invariant is therefore the intended reference/lease retention observer, rather than a parser, type, or fixture failure.

## Existing source tests read

`history_service.rs` contains `latency_percentiles_are_stable` (857), `audio_paths_reject_traversal` (864), `sqlite_schema_round_trips_and_cleans_expired_history` (871), `audio_lease_survives_source_row_deletion_until_reprocess_finishes` (925), and `legacy_schema_migrates_assistant_workflow` (972). `commands/history.rs` contains `only_plain_dictation_history_can_be_reprocessed` (325) and `edit_export_keeps_instruction_source_and_actual_model_separate` (332). These source tests were used for mapping; this slice adds no Rust test code.

## Trusted assumptions and limits

- Bounds are intentionally finite: `MaxRecords = 5`, `MaxLeaseCount = 1`, and `MaxRetentionDays = 1`; the model allows one settings update, one delete attempt, one retention attempt, one reprocess read, and one export run. This is exhaustive only for that bounded abstraction.
- SQLite `IMMEDIATE` transactions, commit/rollback outcomes, filesystem presence, and query/read/write failures are abstract state transitions. The model does not prove rusqlite, WAL, OS filesystem, audio bytes, ASR, LLM, UI dialogs, or scheduler implementation details.
- The model captures the enabled/save-audio consent gate and the retention-day-zero behavior. It abstracts the exact profile command ordering around `cleanup(retention_days)` and persistence, and it does not model app-profile resolution or startup orphan scanning beyond the relevant retained-file condition.
- Existing production source semantics were preserved in the model; no production fix is proposed by this slice. The formal result is bounded evidence, not an implementation proof.
