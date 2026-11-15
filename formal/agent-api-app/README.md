# Managed desktop query verification

The query tests exercise `agent_api::controller::query<R>` on a real managed
`AppState`, `MockRuntime` app handle and owned native JSONL child. It does not
launch a WebView, audio device, installed desktop, model or network request.

This model covers engine_get/status/stats/transcribe. Wry controls and owner-watch
shutdown have separate [host checks](../agent-api-host/README.md). The query's
direct future drop represents owner abort only. The native suite also includes
windowless Wry desktop setter and controller-admission cases; it does not start
the normal GUI. The supplementary [admission model](../agent-api-host/admission/README.md)
owns lifecycle-wait publication schedules outside AppQuery's fixed-input bounds.

| Contract | Actual owner/path | Deterministic behavioral test | Checked properties |
| --- | --- | --- | --- |
| Saved engine does not become an observed runtime | query; paths::read_engine_config; native response.engine | engine_get_reads_saved_selection_without_backend_or_writes; status_observes_native_engine_separately_from_saved_selection | EngineGetNoIpc, ActualRuntimeIdentity |
| Recording/Processing/native owner excludes IPC and file effects | AppState recording slot/snapshot/native_asr_owner; query admission | recording_rejects_mutating_queries_without_touching_backend; processing_rejects_mutating_queries_without_touching_backend; occupied_native_owner_reports_busy_without_touching_backend | RecordingGuard, ProcessingGuard, ExternalOwnerGuard, BusyRuntimeUnknown |
| Absent or unready native process does not become ready from cached state | AppState funasr_process and current native status reply | absent_backend_is_stopped_and_has_no_observed_runtime; status_does_not_promote_stale_desktop_ready_over_native_unready | ActualRuntimeIdentity; absent partition reachable in positive model |
| Actual stats survive; missing/invalid payload fails | ServerCommand::Stats and ServerResponse.stats | stats_returns_actual_native_payload_without_losing_fields; missing_or_invalid_stats_cannot_be_reported_as_success | ActualStats, BackendErrorsExplicit |
| Native application/IPC failure stays visible | send_command_to_server_with_timeout; CommandProcessGuard | backend_errors_remain_explicit_in_status_and_stats | BackendErrorsExplicit |
| Native success requires string text; empty string preserves valid silence | controller::query transcribe response; ServerResponse.text Option; send_command_to_server_json | transcribe_missing_text_field_is_an_error_for_both_local_engines; transcribe_null_text_field_is_an_error_for_both_local_engines; transcribe_explicit_empty_text_is_valid_silence_for_both_local_engines; transcribe_wrong_type_text_remains_an_error_for_both_local_engines | BackendErrorsExplicit, SilenceIsEmpty; absent/silence witnesses and guard controls |
| Local regular bounded file, explicit/default options, owned stable copy | query validation/read/copy; native Transcribe options/hot_words | transcribe_forwards_explicit_overrides_and_cleans_owned_copy; transcribe_uses_current_profile_defaults_and_preserves_explicit_empty_values; invalid_cloud_and_oversize_audio_fail_before_any_backend_command | NoEffectsForInvalidInput |
| Actual native operation keeps permit until reply | native_asr_owner and IPC await | pending_transcription_retains_lease_until_actual_native_reply; actual child entered/release file barrier | PermitUntilSettled, AllQueriesSettle |
| All return/cancel paths remove owned temp; owner abort kills owned child | owned-temp Drop; CommandProcessGuard/FunasrProcess Drop | transcribe_cleans_owned_temporary_on_application_and_ipc_failure; owner_abort_cleans_owned_audio_and_reaps_pending_backend | NoTemporaryAfterReturn, NoOwnedLeaseAfterReturn, OwnerAbortStopsNative, OwnerAbortEventuallyReaps |

Each scenario gets a fresh subprocess before `paths::get_data_dir()` initializes
its `OnceLock`. Each fixture process has an independently opened owned process
handle. Normal cleanup waits for its exit; failure guards terminate only that
exact child. All input/config/profile data is synthetic. Tests inspect owned
temporary paths, original audio, actual native request bytes, real state and
side effects, without assertions on error prose.

Native fixtures and nested scenario processes open their creator's synchronization
handle before readiness or acquiring descendants. A monitor waits on that exact
kernel object and exits its own child process when the creator dies. This covers
watchdog hard kills that skip Rust destructors; it never polls PID liveness.
Scenario stdout/stderr drain concurrently, retaining at most 2 MiB per stream
while discarding further bytes. The containment control fills both streams with
1 MiB, reaches the real pending native barrier, kills/reaps the exact scenario,
checks the retained fixture process handle has exited, joins diagnostics, and
removes the entire owned subtree. Descendant reap precedes reader joins because
a descendant can inherit the scenario's diagnostic write handles on Windows.

The 64 MiB+1 sparse file test exercises oversize rejection. There is no reader
injection seam: concurrent file growth is not dynamically proved by this slice.
Current `read_audio` uses synchronous `File::take(MAX+1).read_to_end` and rejects
the extra byte. `OwnedQueryAudio::create` installs the private-file RAII guard
before synchronous `write_all`, closing the Windows file handle before cleanup.
There is no await at which future cancellation can interrupt that preparation;
native-IPC cancellation remains covered by the existing test. No additional
file-cleanup cohort is introduced for transcript validation. Concurrent growth
has source-bound read evidence, without a dynamically scheduled growth test.

`AppQuery.tla` has two callers, one shared native owner, one representative
external owner and explicit admission/read-copy/IPC/response/cleanup/reap steps.
Initial partitions include recording, Processing, occupied owner, missing child,
valid input, oversize, cloud, missing input, backend false/EOF and malformed stats.
Saved R2T2 differs from observed Qwen. Local-step and OS-reap weak fairness are
assumptions; inference success is not assumed. No unbounded correctness or native
source refinement is claimed. The positive configuration activates 15 safety
invariants and two temporal properties. Matrix also includes 15 semantic safety
controls, one progress control and 14 reachability witnesses. The added response
classes distinguish missing/null transcript from explicit empty-string silence.
Wrong-typed text maps to the existing typed-IPC error partition; its native test
closes stdout after the malformed typed response to bound the parser's wait.

From repository root, with the coordinated shared task target directory and two
build jobs:

```powershell
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml -j 2 --lib agent_api:: --no-run
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml -j 2 --lib agent_api:: -- --test-threads=1 --nocapture
python formal/agent-api-app/check_model.py --jar <pinned-tla2tools.jar> --output <new-task-evidence-directory>
```

The model runner preflights SANY, uses one TLC worker with a 512 MiB heap, retains
logs/receipts, and rejects parse errors or unrelated violations as negative
controls. Source/test/model/config hashes bind each receipt to inspected files.
Coverage and source mutation remain later GREEN gates for the frozen cohort;
historical scaffold-based measurements cannot establish their closure.

## Test-first and current evidence

The initial declaration-only API produced sixteen intended result/count
failures. That RED establishes new-interface expectations, not a regression in
an earlier shipped query implementation. A later implemented-query regression
returned successful null text for missing/null native transcript fields. Both
cases failed before the response guard; explicit empty text remained valid
silence. These cases pass with the implemented guard.

The fixture containment control first observed WAIT_TIMEOUT (258) after its
scenario owner died. Exact creator-handle monitoring corrected that fixture
failure. The restored control confirms descendant exit, diagnostic-reader joins
and removal of the owned subtree. It is a harness correction, not an application
shutdown claim.

The 31-case model matrix contains one exhaustive positive workload, fifteen
named safety negatives, one progress negative and fourteen witnesses. Its
recorded positive run generated 1,293 states with 758 distinct states, depth 12.
Fresh candidate checking and source/model correspondence remain release gates.

The current query suite passes 34 cases with zero failures and two ignored native
fixtures. It calls the actual desktop setter and controller in isolated,
windowless Wry contexts. Six lifecycle cases cover two recording claims, three
startup-owner claims and owner-shutdown cancellation while requests await the
real lifecycle gate. Two further cases change saved configuration to a cloud
engine during that wait: start/restart must return `invalid_request` before
further native effects. The actor's settled local stop is part of the fixture;
no cloud provider, keyring or microphone is called.

The recording actor publishes the actual Starting slot and snapshot under the
same locks as the recording command. The startup actor publishes through
`try_begin_funasr_start`. Explicit polling and a held process mutex contain a
wrongly admitted request before stop, discovery, download or spawn. Cancellation
checks released request ownership while the competing lifecycle guard remains
held. These are controlled state-publication schedules, not live GUI or recording
sessions. The current controller and test identities are bound in the
[admission record](../agent-api-host/admission/README.md).

The separate admission matrix passes all 32 expected semantic outcomes: one
positive, sixteen safety controls, one progress control and fourteen witnesses.
That prefix evidence does not extend AppQuery into a whole-lifecycle proof.
Earlier official setter source controls fail the intended assertion and pass
after restoration. Fresh LLVM collection, the 396 selected native mutants across
ten execution contexts and five restricted entrypoint builds remain pending.
See the [current verification record](../evidence/agent-cli-180.md) and
[native measurement scope](../agent-api-measurements/README.md) for current
identities, historical denominators and other open gates. Earlier authoring logs
and receipts retain their recorded inputs outside the source tree.
