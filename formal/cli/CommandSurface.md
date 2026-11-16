# Python command surface and terminal broker failures

This slice specifies the desired Python CLI contract from baseline
`adba35190512313de45cd2ae86a6c7cdfc699127`. The baseline rejects the new command
spellings and hot-word option through its existing parser and returns matching
fatal responses from `EngineClient.request`. The executable RED is an assertion
failure through those existing public seams; no new API scaffold is used.

The production owner is `scripts/light_whisper_cli.py`. Test Writer changes are
limited to `scripts/test_cli_commands.py` and these `CommandSurface*` artifacts.
The main agent owns final production/model correspondence, assembled regression,
coverage, source mutation, real native transport and actual desktop evidence.
These tests execute Python main and real owned synthetic JSONL children. They do
not execute a native broker, connect to a desktop, load a model or decode audio.

| Requirement / retained state | Production call path | Executable test in `CommandSurfaceTests` | Active model property; control / witness |
| --- | --- | --- | --- |
| Default app target and six desktop wire actions | `main -> parser -> resolve_target -> request_client -> EngineClient.request` | `test_one_shot_desktop_commands_dispatch_the_exact_wire_action`; `test_desktop_session_accepts_all_commands_and_preserves_opaque_external_ids` | `DefaultApp`, `Admission`, `WireActionMatches`; Default/Admission/WireAction negatives, Default/Commands witnesses |
| Desktop actions rejected for explicit/inferred engine target before child acquisition; invalid session record recoverable | `main/run_session -> target validation -> request_client` | `test_desktop_commands_reject_explicit_or_inferred_engine_target_before_spawn`; `test_desktop_only_session_records_reject_engine_target_without_starting_child`; `test_rejected_desktop_records_preserve_an_existing_independent_session` | `RejectBeforeSpawn`, `Admission`; Target negative/witness |
| Only two local engine names accepted; exact `engine` field and no unrelated session fields | `parser/main`; `run_session -> session_request -> EngineClient.request` | `test_one_shot_engine_set_accepts_only_the_two_local_engine_names`; `test_session_engine_set_and_command_field_validation_is_recoverable_before_spawn`; positive command/session tests | `Admission`, `EngineFieldMatches`; EngineValidation/EngineField negatives and witnesses |
| Repeatable strings retain order, Unicode, empty and duplicate hot words at top level; omitted option absent | `parser -> main transcribe fields -> EngineClient.request` | `test_transcribe_hot_words_are_repeatable_top_level_strings_and_omitted_when_absent` | `HotWordsAtTop`, `OmittedHotWordsAbsent`; HotWords/OmittedWords negatives and witnesses |
| Valid matching `fatal: true` raises for either success value; startup alone proves no connection | `EngineClient.request -> _receive` | `test_matching_valid_fatal_response_raises_from_engine_client_for_either_success_value`; fatal session test | `FatalTerminates`; Fatal negative, Fatal/FatalSuccess witnesses |
| Fatal response stops before consuming queued record, correlates external ID, never retries | `run_session pending_id -> client.request -> exception -> ExitStack cleanup` | `test_fatal_session_response_is_correlated_terminal_without_reading_or_retrying_next_record` | `FatalDoesNotReadTail`, `ReplyIdentity`, `NoAutomaticRetry`; FatalRead/Identity/FatalRetry negatives, Fatal/Identity/Cleanup witnesses |
| Correlation and response-shape validation precede fatal handling | `EngineClient._receive -> EngineClient.request` | `test_unmatched_or_invalid_fatal_response_is_ignored_until_valid_matching_reply` | `FatalNoiseFiltered`; Filtering negative/witness |
| `success: false` with missing/false fatal field remains recoverable | `run_session -> client.request -> session_reply -> next input` | `test_explicit_backend_failure_without_fatal_true_remains_recoverable` | `NonFatalContinues`; Recovery negative/witness |
| One broker reused; terminal path closes/reaps only owned transport; doctor static | `request_client`; `run_session ExitStack`; `EngineClient.__exit__`; `doctor` | command/session tests call inherited `assert_closed`; existing `test_cli_target` and `test_cli_session` retain detailed ownership/doctor tests | `OwnedCleanup`, `DoctorReadOnly`, `EventuallyDone`; Reap/Doctor/CleanupProgress negatives, Cleanup/Doctor witnesses |

The model has five finite input profiles, one-shot/session surfaces, three target
inputs, six desktop actions, both local engines, cloud/missing engine classes,
two hot-word shapes, seven response partitions and two signed external-ID
representatives. A session contains at most two records; IDs may repeat. Input
validation, acquisition, dispatch, response publication, next-record consumption
and cleanup are distinct transitions. Fatal filtering represents the real receive
loop dropping unmatched or invalid records before the matching response.

`WF(Advance)` assumes each bounded synchronous response/cleanup transition can
finish. It makes no guarantee about arbitrary native desktop behavior. Terminal
states stutter explicitly. The CleanupProgress negative removes cleanup progress
and must violate `EventuallyDone`; it is a liveness sensitivity control. There
are no symmetry reductions, state constraints, action constraints or VIEW.

Each safety negative checks `TypeOK` and the named behavioral invariant. The
positive config activates every claimed invariant plus `EventuallyDone`.
Witnesses deliberately violate `WitnessNotReached`; they are reachability
evidence, never safety-control kills. `CommandSurfaceCheck.py` records exact
commands, tool/source/model/config hashes, exits, state counts and complete logs.
It rejects a JAR whose SHA-256 differs from the pinned supplied tool.

Run from the repository root, using a fresh output directory:

```powershell
uv run --no-sync python -m unittest scripts.test_cli_commands -v
uv run --no-sync python formal/cli/CommandSurfaceCheck.py --java $javaPath --jar $jarPath --output $evidencePath
```

Set `$javaPath` to Java, `$jarPath` to the pinned `tla2tools.jar`, and
`$evidencePath` to a new directory outside the tracked specifications.

SANY preflight and all 32 TLC configurations must execute before this finite
model evidence is accepted. Baseline RED cannot satisfy final GREEN or measured
coverage/mutation gates. Final production correspondence requires review of the implemented call path
against the accepted artifacts; the current admission mapping is recorded below.

## Current desktop admission boundary

At reviewed CLI SHA256
`db2655d9beecc41de1b20338cb44c6af64dae72701d833b5459639d0cc55d9b7`,
app payloads pass `validate_desktop_request` before acquisition or dispatch.
Installed-command discovery changes only `app_path`; these admission call paths
are unchanged. The [command-entry contract](../cli-command-entry/README.md)
owns that separate boundary.
`Validate` retains command/schema/target semantics; admitted string representatives
are scalar and fit the physical cap. Additional scalar/size failures use the
existing recoverable input-error boundary. This model does not calculate those
guards. [DesktopAdmission](DesktopAdmission.md) owns their finite contract,
including next internal ID, newline width and live-owner preservation. The
[correspondence table](README.md#desktop-admission-source-correspondence) records
the unchanged model boundaries. Earlier execution claims retain their original
source identities; current-source replay is a separate gate.
