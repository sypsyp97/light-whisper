# Native agent transport verification

This harness imports the actual production
`src-tauri/src/agent_api/{transport,secure_files}.rs`; it does not import Tauri,
load an engine, download a model, activate a microphone or use user data.
Its synthetic `Dispatch` proves the native TCP/broker boundary only. It does not
prove the Tauri controller's `AppState` wiring. The separate
[application host tests](../agent-api-host/README.md) exercise that path.
All 34 transport tests passed for the frozen native measurement. Current source
identities, coverage limits and outstanding mutation checks are recorded in
[native measurements](../agent-api-measurements/README.md).
Tests reuse the read-only native ACL fixture from
`../agent-api-security/tests/support/mod.rs`.

## Public seam and wire contract

The only required production API is `Dispatch = Arc<dyn Fn(Value) -> Value +
Send + Sync>`, `AgentServer::start(&Path, Dispatch) -> io::Result<AgentServer>`,
`request(&Path, Value, Duration) -> io::Result<Value>` and
`run_client(&Path, &mut impl BufRead, &mut impl Write, Duration) -> io::Result<()>`.
`AgentServer` owns listener/lease/workers until Drop completes.
The approved cfg(test) seam `AgentServer::start_observed` adds an ingress
observer to the same start core. The real worker invokes it after retaining its
actual slot and before parsing/reading ingress. This is a scheduling observation,
not a replacement server; shared-core/source coupling stays pending until GREEN.
Tests import the actual transport source under cfg(test), while the probe binary
uses the public non-observed library entrypoint.
The approved cfg(test) `start_observed_writes(&Path, Dispatch, usize,
Arc<dyn Fn(&TcpStream, Instant, usize) + Send + Sync>)` uses that same real start
core and real `write_frame`. Only this test limits actual write calls to 1024
bytes. Its observer fires after each successful socket write, receiving the
actual stream, original absolute egress deadline and actual written length.
The observer queries the real stream's `write_timeout()` and schedules barriers;
it does not supply a fake writer, socket or pretended timeout. Public production
writes remain unconstrained. The new declaration currently returns Unsupported.
Shared-core/write-call/getter source correspondence and reset-deadline,
fixed-timeout and early-slot-release source negatives remain required after GREEN.

The Windows 10/11 endpoint is private `agent/endpoint.json`, at most 4096 raw
bytes, with exactly `protocol:1`, numeric `127.0.0.1:<nonzero port>`, 64 lowercase
hex token, 32 lowercase hex instance ID and positive u32 PID. The bytes must
come from the same native file handle whose owner, private ACL and regular-file
status were validated. Clients neither repair state nor autostart a host.

The token is decoded to a 32-byte HMAC-SHA256 key and never sent over TCP.
Request keys are exactly `protocol`, `instance_id`, `nonce`, `auth`, `request`;
response keys are exactly `protocol`, `instance_id`, `nonce`, `auth`, `response`.
Each client request uses a fresh 128-bit lowercase hex nonce. The MAC is over
`serde_json::to_vec(json!([domain, 1, instance_id, nonce, inner]))`, with domains
`request` and `response`. The response repeats the request nonce. JSON maps use
the existing default sorted map representation. Native `ring::hmac::verify`
must verify tags; the model assumes HMAC authenticity and nonce freshness.
The tests check real signed packets, wrong keys/domains/nonces, response replay, payload
tampering and absence of the token on the wire. They do not prove a cryptographic
primitive or measure constant-time execution.

Schema validation follows JSON decoding; duplicate keys intentionally use
serde_json's last value. Request/response limits are 64 KiB/4 MiB **including**
LF. CRLF is allowed and counts two bytes. Incomplete or unauthenticated frames
can close without an error oracle. A valid MAC with bad inner input gets a
signed `invalid_request` failure before dispatch. Valid IDs are u64, including
zero. Allowed actions are status/stats/models/devices/engine_get/engine_start/
engine_restart, engine_set with qwen3-asr-0.6b/confucius4-r2t2 and transcribe with a nonempty
audio_path and options object. Options accepts only language/context strings.
The optional top-level hot_words field is an array of strings; nested
options.hot_words is rejected, including a list containing only valid strings.
This matches `scripts/light_whisper_cli.py::session_request` and the serialized
`ServerCommand::Transcribe` in `src-tauri/src/services/funasr_service.rs` and
`services/funasr_service/protocol.rs`. All other keys/types/actions fail.
`exit` belongs only to the local stdin broker.
The engine strings match `scripts/light_whisper_cli.py::LOCAL_ENGINES` and
`src-tauri/src/utils/paths.rs::configured_engine`. Internal aliases localQwen
and R2T2 are rejected at this public boundary.

Controller objects need a boolean success; transport overwrites request_id from
the validated request. Panic becomes operation_failed. Invalid shape, excessive
response size or token/protocol/instance_id keys anywhere in a payload become
invalid_response. Errors use a nonempty sanitized string and a snake_case code;
they never echo authentication material or panic/private input. A valid backend
failure remains an ordinary recoverable response.

Ingress has an absolute 2s deadline from accept. Egress has an absolute 2s
deadline from controller completion. Client duration covers endpoint discovery
through response, with best-effort noncancellable native file operations.
Partial reads/writes never reset those deadlines. Dispatch itself has no outer
timeout or cancellation. Four retained network worker slots bound admissions;
a fifth rejects/closes without dispatch. Drop stops new admissions, closes the
listener, drains admitted dispatch, removes only its matching endpoint while
holding its stable non-delete-shared host.lock lease, then releases the lease.

The broker first flushes exactly the success/protocol/surface fields plus
`agent_protocol: "LIGHT_WHISPER_AGENT_PROTOCOL_1"`. This means broker readiness,
not connection to a running desktop. It processes stdin serially, ignores blank
records and reports recoverable invalid input without server effects. A local
exit ACK stops before reading another queued record. EOF has no padding reply.
A transport failure flushes a correlated fatal transport_error, returns Err and
stops before the next input read. Output failures propagate.
A schema-valid stdin body can fit the 64KiB cap while its authenticated TCP
envelope exceeds that same cap. This is a recoverable local invalid_request:
no dispatch, no fatal flag, and the following status/local-exit records work.
Both caps continue to count encoded bytes including LF.

## Hidden owners, transitions and evidence bindings

Production bindings below name the frozen public seam. Detailed production
guards, locks, shared helpers and exception paths remain **OPEN** while the
current transport implementation is partial and not yet GREEN.
The parent must review correspondence after implementation, including use of
the real private-file helpers and constant-time MAC verification.

| Owner/promise | Public seam and model | Native test schedule |
| --- | --- | --- |
| Descriptor identity/private handle | start/request; Protocol PrivateEndpoint | strict descriptor/ACL oracle; malformed/private/4096 vs 4097 byte endpoint controls |
| Authentication and fresh response binding | request/server ingress; Protocol MAC/nonce/correlation properties | signed raw frames; tampering; wrong key/domain/nonce; old nonce replay; no plaintext key |
| Schema before side effects | server ingress; Protocol ValidatedDispatch/BoundedCompleteFrame | invalid ID/action/key/type partitions; callback count remains zero; exact 64KiB LF/CRLF boundary |
| Controller result authority | server egress; Protocol SanitizedReply/ServerOwnsIdentity | overwritten ID; panic; nested forbidden keys; invalid success; exact 4MiB vs excessive reply |
| Exclusive stable lease | start/Drop; Lifecycle SingleHost/PublishedUnderLease/StableLeasePath | real child host; second child denied; path replacement denied; stable file identity across restart |
| Bounded admitted work | start/Drop; Lifecycle WorkerBound/NoPostCloseAdmission | four callback barriers, fifth busy/close, release and receive four replies |
| Slots exist before authentication through completed egress | start_observed/public shared core; Workers WorkerBound/NoPostCloseReservation/ClosingDrains | four actual reservation observer barriers; partial unauthenticated input; fifth must reach EOF/reset while barriers remain held; queued-open timeout fails |
| Dispatch outlives client | request/Drop; Lifecycle NoCanceledDispatch/AdmittedSettles | actual client deadline while callback gated; listener closes; lease/endpoint remain until dispatch release |
| Endpoint cleanup owner | Drop; Lifecycle DrainedCleanup/PreserveForeignDescriptor/ReleaseAfterCleanup | pre-cleanup descriptor replacement survives; neighbor survives; own descriptor removed after drain |
| Absolute socket deadlines and retained egress slots | request/server/start_observed_writes; Deadlines AbsoluteDeadline/EgressStartsAfterDispatch and Workers | timed partial-byte input; client total deadline; four real partial writes with callback barriers, native socket timeout getters and nearly-4MiB valid payload |
| Serial broker/fatal stop | run_client; Broker configured safety properties | guarded BufRead refuses queued read; local exit; recoverable record/backend error; correlated fatal; flushed startup/replies |
| Local encoded-envelope rejection | run_client; existing Broker bad/RecoverableFailures/recovery witness | schema-valid 64KiB stdin body exceeds wire budget after signing; recoverable correlated failure; no callback; following status and local exit succeed |
| No retry after unknown outcome | request; Broker NoRetry | malformed peer keeps listening until request returns; queued second connection is an assertion failure |

Native ordering uses channels, controller gates and actual socket/listener
observations. Timer ticks supply adversarial deadline input; they do not replace
the admission barriers. The controlled-write test holds four admitted dispatches
for 2.2s, then prepares valid replies 64 bytes below the 4MiB frame cap. Each
owned TCP peer reads its actual first 1024-byte prefix while the four successful
write observers retain their worker slots. A fifth request must close or fail
busy. Releasing the first callback phase about 1.2s into the egress budget allows
second real 1024-byte writes. Actual native socket timeout getters must now show
a reduced remaining budget, and callbacks must retain the original deadline.
The second phase remains held until that original deadline plus 100ms; release
must produce EOF with only 2048 incomplete-frame bytes and no third write. A
sixth actual handler admission must occur by the original deadline plus 700ms.
Every socket/read/channel wait and the outer process are bounded; release guards
open all callback gates before panic cleanup drains the server.

This is controlled scheduling of real TCP writes. It does not prove kernel
backpressure. Historical experiments let all four 4,194,240-byte writes complete
even with actual SO_SNDBUF=2048 and pre-connect SO_RCVBUF=1024. Their failed busy
assertion exposed an unreliable environment premise, not a product retention
bug. Those raw experiments stay in their original frozen lanes. The replacement
test uses neither buffer-setting seam nor receive-window helper. The exact OS
accept queue length is outside the assertions.
The ingress test counts actual reservation observations and checks connection
closure before releasing the four observers. A missing capacity guard cannot
hide behind a queued-open socket or client timeout. All five peers close and
observers release before failure assertions. Accept-to-ingress deadlines retain
their original origin; observation pauses do not suspend time.
Invalid endpoint socket specimens point at owned loopback canaries, including a
separate 127.0.0.2 listener. An IPv6 canary is used only if the OS can bind it.
No routable remote specimen is dialed, even by an incorrect implementation;
general remote rejection also requires source/model correspondence review.

Lifecycle explicitly separates lease acquisition, bind, publication, acquisition/
bind/publication failures, controller completion, cleanup observation, unlink
and lease release. A cooperating second host cannot publish while the lease is
held across observation/unlink. A manual changed descriptor **before** cleanup
is represented and must survive. An arbitrary same-user writer replacing the
endpoint between observation and unlink is outside this trusted-principal
assumption; neither the model nor these tests claim atomic check/unlink.

## Checks and finite bounds

Run from the repository root, with no competing heavy test/TLC job:

```powershell
cargo test --offline --manifest-path formal/agent-api-transport/Cargo.toml -j 2 --no-run
cargo test --offline --manifest-path formal/agent-api-transport/Cargo.toml -j 2 -- --test-threads=1
python formal/agent-api-transport/check_model.py --jar <existing-pinned-tla2tools.jar> --output <fresh-owned-results-directory>
```

The model runner SANY-checks all five modules, then runs every case sequentially
with `java -Xmx512m`, `-workers 1` and default deadlock checks. Positive cases
must finish exhaustive checking. Safety controls require the named invariant;
temporal controls require a temporal violation. Witness failures are recorded
separately and are never mutant kills. A selected `--case` run is partial.
Receipt hashes bind modules/configs/runner, source, tests, fixtures and JAR.

Protocol covers 23 semantic partitions, including malformed endpoint/header,
request/response MAC and nonce failures, schema/frame errors, panic and backend
false. Lifecycle uses two host identities, five possible requests and four
slots; identities do not wrap. Broker explores two records with six kinds and
distinct internal IDs. Deadlines explores two chunks, deadline=2, elapsed=0..3,
and dispatch duration=0..3; these are before/at/after partitions, not claims
about continuous-time precision. Weak fairness applies to enabled finite owner
steps and eventual dispatch completion. Stalled completion controls remove the
corresponding environment/owner progress and must fail temporally. No symmetry
reduction or state/action truncation is used.

`Workers` adds five requests moving through separate unreserved, unauthenticated
ingress, dispatch, egress and released states. Its count includes all three
retained phases. Seven configurations check normal safety/drain progress,
post-auth-only counting, early egress release, reservation after close, stalled
egress and explicit four-ingress/four-egress witnesses. The original 59 model
configurations and their properties remain unchanged.

In-process scenarios run inside an owned test subprocess with a 20s watchdog,
bounded diagnostic tails and an isolated TEMP/TMP subtree. The parent terminates
and reaps a stalled child, drains/closes its pipes, then removes that owned
subtree before reporting assertion failure. Deadline tests also have shorter
recv watchdogs: stop fake-peer drip input or release callback gates before
joining workers. Real host probes use bounded process waits and owned
kill/wait/pipe-reader cleanup. A native stalled-probe control exercises the
watchdog. A mutator timeout never counts as semantic detection.

Nonce/foreign-instance rejection specimens carry otherwise correct MACs, so a
missing schema/identity guard cannot pass through an unrelated MAC mismatch.
Exact and excessive request, server-response and client-response fixtures include
multibyte text with fewer characters than the byte cap. A character-count size
implementation therefore violates these assertions.

The controlled-write seam's initial Unsupported scaffold supplied historical
interface evidence, not a reproduced product defect. The implemented seam and
the other public TCP/broker paths now pass the 34-case suite. Real-source mutation
accounting remains a separate gate.
The accepted integration repair uses implemented public TCP and broker paths.
Broker's existing `bad` class includes a locally oversized signed envelope;
RecoverableFailures and its existing stop-recoverable negative/recovery witness
already cover that classification and required continuation. No new model
mechanism or weakened limit is introduced. The original finite checks remain
historical evidence until a final GREEN source-bound rerun.

Remaining feasible branches, source mutations, MAC/nonce source sensitivity,
installed binary behavior and full private cleanup closure remain OPEN.
The measurement recipe uses isolated LLVM coverage and cargo-mutants tools;
their presence does not establish a passing coverage or mutation result.
Use the frozen cohort in `measurement.json` after a reliable native GREEN;
measurements and source controls must run on disposable copies and real source,
not a rewritten test helper. Preserve separate raw source/model mutation counts.
