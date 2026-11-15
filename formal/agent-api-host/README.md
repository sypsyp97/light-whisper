# Windowless native desktop host verification

The driver exercises the built windowless host and connection broker. It uses
the application's real Wry runtime, managed `AppState`, private transport and
engine lifecycle with a synthetic JSONL engine. A separate managed-state test
checks that an admitted start remains observable before child acceptance.
Historical RED evidence is recorded in the [verification report](../evidence/agent-cli-180.md).

The driver never discovers installed executables. Pass the newly built task's
`debug/light-whisper.exe` explicitly, verify its source/build identity, and retain
evidence outside temporary state. No Computer Use, UI automation, microphone
capture, paid APIs, model download, credentials or user profile data are involved.
`devices` only calls actual enumeration. No normal desktop window is launched.

## Real call path and hidden owners

| Owner / hidden state | Actual source | Observable contract / test |
| --- | --- | --- |
| Native argument dispatch before plugins | `src-tauri/src/lib.rs::run` | Compiled marker; client ready/exit/EOF with no endpoint; explicit host data directory; window count from actual Wry app, tests 01–06 |
| Host `AppHandle`, managed `AppState`, endpoint lease | `lib.rs` host setup; `agent_api/transport.rs::AgentServer` | Ready only after endpoint publication, distinct data isolation, duplicate refusal, local host exit/EOF, tests 06–10 |
| Saved configuration / observed native identity | `utils/paths.rs`, `agent_api/controller.rs::query` | Saved `configured_engine`; no fake observed identity; engine switch stops old native without autostart, tests 06,11 |
| Installed engine fingerprint and native child | `funasr_service/installation.rs::find_engine`, `funasr_service.rs::start_server` | Own engine `.version` follows real archive/version fallback; actual `serve --engine` arguments, tests 07–13 |
| Start ticket, generation, starting child, native ASR lease | `EngineState`, `start_server`, `commands/funasr.rs::lock_idle_asr`, controller dispatch | Startup barrier holds owner, competing controls have no effects; quit reaps child; test 18 and direct Rust STARTING test |
| Admitted operation lifetime vs client lifetime | Transport worker -> controller query -> native JSONL | Python CLI timeout disconnects broker, accepted transcription stays running; second client sees busy and cannot mutate; completion once, test 16 |
| Owned audio copy and original file | `controller.rs::OwnedQueryAudio`, `read_audio` | Real native IPC reads owned copy; original unchanged; cleanup after completion or owner quit; no profile/history writes, tests 13,16,17 |
| Owner shutdown watch vs client disconnect | Host owns watch sender, controller owns admitted future; stop/drain server before backend stop | EOF cancels its own pending future and reaps child without releasing fixture gate; endpoint removed under lease, tests 17–18 |
| Raw local file/device observation | `funasr_service::check_model_files`, `audio_service::list_input_devices_sync` | Actual empty isolated cache reports missing; actual device result shape, no engine commands, tests 14–15 |
| Test containment independent of product cleanup | Python exact Windows Job Object and Rust fixture parent handle watch | Hard-kill test checks native exits before fallback job termination; concurrent bounded pipe drains and evidence survives teardown, test 19 |
| Allowlisted desktop boundary | Native broker/transport schema -> controller | Local engines only; cloud, invalid legacy ID, arbitrary invoke and credential actions fail, host remains alive, test 20 |

Tests 18 and 20 retain an exact handle to a real synthetic native child. Every
rejected control checks that child is still alive, native command audit is
unchanged and persisted configuration is byte-identical before proceeding. Test
20 starts the sentinel first, so an otherwise silent stop/kill cannot pass.

Existing query tests in `agent_api/tests.rs` remain intact, including recording,
Processing, occupied native ownership, client-independent IPC, original-file
safety, watcher and hard-kill containment scenarios. This slice adds one test,
not a replacement query harness. Broker invalid-action tests reach the native
boundary; they do not independently establish controller validation after a
transport schema rejection. Main's frozen decision preserves Wry-only dispatch:
its internal action match and owner watch receive source review, while existing
generic query guards and actual Wry public busy tests provide runtime evidence.
No genericized fake dispatch stand-in or standalone dynamic bypass claim is used.
Normal GUI registration and `engine-config-changed` frontend invalidation require
source correspondence / the separately assigned frontend tests; no GUI execution
is claimed here. Parent supplies final independent-engine model inference run.

The headless switch tests establish that the native setter stops without starting
a replacement. In the GUI, the settings hook's current invalidation calls
`retryModel`; `useModelStatus.retry` checks status and can start a stopped local
engine. The headless result does not establish that the full GUI remains stopped.

## Deterministic synthetic native fixture

`native-fixture` is a tiny Rust executable installed as **only this test's**
`data/engine/engine.exe`. It implements production JSONL startup and the actual
status/stats/transcribe/settings/exit protocol. It records its actual PID,
arguments, input audio bytes and native commands under the test-owned control
directory. It never infers text; its fixed transcript is explicitly synthetic.
`hold-startup`/`hold-transcribe` and matching `entered-*`/`release-*` files are
barriers. Timing polls observe barriers; no sleep establishes race ordering.
The fixture opens its exact live parent host process handle before readiness
and exits if that host is hard-killed. The driver contains exact owned descendants
in a Job Object and drains stdout/stderr concurrently even on fallback cleanup.

The data directory contains a synthetic profile. Every case owns fresh data,
model cache, temp audio, backend and host stdin. The lease file
`agent/host.lock` is allowed to remain; `agent/endpoint.json` must disappear on
normal host shutdown. An endpoint's token is never copied into evidence logs.

The missing/empty-data-directory negative has an additional launch safety gate.
On Windows, `paths::get_data_dir` falls back through `dirs`/`dirs-sys` to
`SHGetKnownFolderPath(FOLDERID_RoamingAppData)`; changing a child `APPDATA` variable
does not isolate that fallback. Before either negative process is spawned, test
05 requires `lib.rs::run` to begin with the exact whitespace-normalized reviewed
missing-data guard, or the exact unconditional baseline agent exit2 stub. No
prior statement is allowed. An unrecognized prefix fails before process creation,
without skipping the test or touching Windows profile/global configuration. The
guard receipt records this narrow static proof separately from native exit2;
it does not claim a general Rust control-flow proof or dynamic regression when
the source safety gate refuses to launch. Main binds the compiled app to that
reviewed source build; a source scan alone does not identify an arbitrary PE.

## Commands

Run heavy commands serially. Cargo offline/locked, two jobs; Rust test threads one;
Java maximum heap 512 MiB, TLC one worker. No dependency installation is required.

```powershell
$env:CARGO_TARGET_DIR = '<task>/rust-app-target'
cargo build --offline --locked --manifest-path src-tauri/Cargo.toml --bin light-whisper -j 2
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --lib agent_api::tests::status_during_native_start_before_child_acceptance_is_unknown_not_stopped -j 2 -- --test-threads=1 --nocapture
cargo build --offline --locked --manifest-path formal/agent-api-host/native-fixture/Cargo.toml --target-dir '<task>/host-fixture-target' -j 2
python formal/agent-api-host/run.py --app-exe '<task>/rust-app-target/debug/light-whisper.exe' --fixture-exe '<task>/host-fixture-target/debug/agent-host-native-fixture.exe' --evidence '<task>/host-red'
python formal/agent-api-host/check_model.py --java '<Java 8>/bin/java.exe' --jar '<pinned>/tla2tools.jar' --output '<task>/host-tlc'
```

Use repeated `--case test_<name>` to run a coherent subset without rebuilding.
Initial native RED may run 01,02,06 only; all remaining cases stay pending until
the new host interface exists. Initial direct STARTING RED is a separate semantic
query failure, with no backend process involved.

## Finite model, controls and composition

`AgentHost.tla` models the entire owned host/client/operation/shutdown graph in
this slice: two clients, one native operation owner, two engine IDs, at most two
admitted operations, up to two starts-or-restarts, one transcription, one host lifetime.
It distinguishes local broker readiness/exit, host readiness and endpoint lease,
saved/observed identity, external recording/Processing busy, start preparation,
admitted transcription, client disconnect, owner watch retirement, server drain,
backend stop and final lease release. All intermediate actions can interleave;
terminal stopped state explicitly stutters. No symmetry/depth/state constraints.

| Source mechanism | Active property | Negative control | Reachability |
| --- | --- | --- | --- |
| Host exclusive endpoint lease | `EndpointHasOneOwner` | `lease` removes owned endpoint lease | `NoDuplicateAttempt` |
| Operation keeps native lease despite broker disconnect | `AdmittedOperationRetainsLease` | `disconnect` releases lease early | `NoDisconnectedPending` |
| Broker exit is local | `ClientExitIsLocal` | `client_exit` drains host | `NoClientOnlyReady` plus normal host states |
| Busy rejection before effects | `BusyAndInvalidHaveNoEffects` | `busy` admits forbidden effect | `NoBusyRejection` |
| Local-only/action allowlist before effects | `BusyAndInvalidHaveNoEffects` | `invalid` admits forbidden effect | `NoRestartAfterInvalidRejection` reaches both invalid rejection and subsequent start/restart |
| Startup ticket/generation retirement | `RetiredStartCannotPublish` | `stale` publishes after owner retirement | `NoStartupRetirement` |
| Owned audio cleanup after native settlement | `OwnedAudioLifetime` | `audio` retains copy after terminal completion | `NoCompletion` |
| Backend stop before final host lease release | `StoppedIsClean` | `release` permits host release with live backend | `NoCleanShutdown` |
| Runtime identity comes from accepted native owner | `ObservedIdentityIsActual` | `identity` substitutes another engine ID | `NoCompletion` reaches accepted runtime |
| Shutdown cancels owned future before drain | `ShutdownProgress` | `cancel` disables cancellation | `NoCleanShutdown` |
| Admitted native response eventually settles | `OperationProgress` | `finish` disables completion | `NoCompletion` |

`NoRestartAfterInvalidRejection` requires two start admissions in one host lifetime.
The first accepted start reaches a running native child before a different client
can admit the second start/restart; this keeps restart reachability non-vacuous
within the same two-client/two-admission bound. The same witness also reaches the
invalid-request rejection antecedent without consuming a valid admission.

`OwnedAudioLifetime`, `StoppedIsClean`, `ObservedIdentityIsActual` and `TypeOK`
are also active in every positive run, with the named sensitivity controls above.
The no-effects invariant compares snapshots of actual native/configuration/
operation/counter state immediately before and after each rejected call. Its
busy negative changes the native state and increments the real effect counter;
its invalid negative changes configuration/native state and increments that
counter. Neither negative assigns a verdict flag or fabricates a failed comparison.
The 13 observed fields bind to source as follows: `native` to accepted/starting
process slots; `configured` to `read_engine_config`; `runtime` to the accepted
native engine identity; `effects` to native/configuration effects in the audit;
`starts` and `transcriptions` to spawn/completion counters; `nativeLease` to
`native_asr_owner`; `copy` to `OwnedQueryAudio`; `phase` and `owner` to the admitted
controller future and its owner watch; `generation` and `ticket` to
`funasr_generation`/`StartTicket.generation`; `requested` to `StartTicket.engine`.
The frame snapshots are verification observers, not production verdict fields.
Progress assumes the native operation eventually responds when the owner stays
alive and assumes host cleanup actions are fairly scheduled. A permanently hung
native process requires owner shutdown; model liveness does not prove arbitrary
external responsiveness. The fixture's hard-kill containment is test-only and
is not a product crash-cleanup claim.

Composition is explicit, not an unbounded refinement proof. The existing query
model covers real query admission/audio/native response contracts; the separate
transport model covers private endpoint/auth/schema/deadline contracts. This model
assumes authenticated validated requests reach dispatch once, then models that
operation's owner through host shutdown. Runtime tests compose all three actual
production paths. Source hashes in each TLC receipt bind its checked snapshot;
they do not replace implementation correspondence review. The actual native
driver passed all twenty cases and owned-process cleanup checks. Current source
identities, coverage/mutation limits and separate inference results are in the
[1.8.0 verification record](../evidence/agent-cli-180.md). Installed application
and live GUI behavior require separate runtime evidence.
