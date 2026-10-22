# Application formal contracts

This directory is source-controlled verification code and regression data.
Keep models, configurations, Lean proofs, the coverage registry and reviewed
replay fixtures in Git. Store per-run logs, model-checker state and downloaded
tools under the ignored `tmp/` directory; compiled Lean output is ignored too.

The application state model covers recording/capture/hotkeys, all four engine
classes, credentials/OAuth, providers/settings/personalization, context/routing,
selection/assistant, history, windows/subtitles, updates, theme/autostart and
bundled protocols.

[`verification.json`](verification.json) binds **128 registered commands**,
**19 event families**, **16 lifecycle entries** and **14 protocol families** to
obligations, model operators, source/test paths and explicit assumptions. CI
rejects inventory drift, missing anchors, unasserted properties, unreachable
explicitly declared actions, failed models/proofs and negative controls that no longer find the
expected counterexample. Registry membership is correspondence evidence, not
automatic proof of a Rust/TypeScript function.

## Verification layers

| Layer | What is established |
| --- | --- |
| Integrated AppWorkflow, ten workloads | Cross-subsystem safety: recording/output, consent/context/clipboard, download/configuration, assistant/selection, history/subtitles, updates and GPU suspension/reload. All declared actions must be reached across workloads. |
| Component TLA+ models | Deeper interleavings and validation/error partitions, with explicit finite bounds and conditional fairness for progress. |
| Lean StateContracts | Parameter-independent ownership/decision contracts, profile-version induction, first matching rule, consent, clipboard preservation, sample cap, correction provenance and theme. No sorry, admit or declared axiom. |
| [Verus implementation proofs](verus/README.md) | Required CI verification of production capture arithmetic, hotkey ownership transitions and interim interval control, plus typechecked negative controls. Caller wiring, synchronization and mixing retain separate Rust tests and review. |
| Implementation correspondence | Actual regression tests and source-path review, including a source guard requiring provider/endpoint/key snapshots before OAuth awaits. This is not compiler refinement. |
| Negative controls | Legacy races and deliberate guard removals must fail with Java exit 12 and the named violated invariant. Required CI gates. |

## Model families

| Area | Models |
| --- | --- |
| Recording and capture | RecordingLifecycle, CapturePipeline, MicrophoneMonitor, MicrophoneAsyncOwnership, MicrophoneEffectOwners |
| Hotkeys | HotkeyLifecycle (three event-kind runs plus registration workload), HotkeyPendingStart, HotkeyRegistrationEpoch, HotkeyRegistrationFailure |
| Engines and migration | EngineDownload, RuntimeConfiguration, EngineConfigurationWrites, ModelDirectoryMigration, ModelIntegrityRepair, EngineStatusOwnership |
| GPU residency and settings | GpuIdleLifecycle (startup and runtime enable), GpuIdleSettings |
| Authentication and provider pairing | OAuthLifecycle, OAuthStorage, ProviderRequests |
| Credential publication and account snapshots | WebSearchKeys, ProviderAuthSnapshot, AssistantCredentialOwnership, CatalogDiscoveryOwnership, OnlineAsrIdentity |
| Settings and personalization | ProfilePersistence, SettingsContracts, Personalization, AppProfileRules, AsyncSettings |
| Routing, tasks and history | ContextRouting, TaskOwnership, HistoryRecords, HistoryLeaseInterleavings, OutputDrainOwnership, HistoryFilterOwnership, HistoryStatsOwnership |
| Windows and UI state | WindowLifecycle, UiPreferences, ThemeSynchronization |
| Download timers and event subscriptions | DownloadUiLifecycle, EventSubscriptionLifecycle |
| Protocols and transport | RequestProtocols, LlmTransport, LlmRequestParameters, IpcCommandDeadline |

AppWorkflow is the additional integrated model. The registry is the authoritative
positive configuration/negative-control list. Operator names and concrete
source/test anchors are recorded per contract there.

AppWorkflow separates routed external search and fresh screen preparation from
LLM dispatch. Both preparations settle before dispatch, and fresh screen payloads
are checked against the foreground identity sampled after the join. This sample
does not prevent later OS focus changes. ModelNative search executes inside the
LLM request; reused pre-polish descriptions retain their existing semantics and
have Rust regression coverage.

History audio I/O may remain pending during ASR and LLM work. Audio resolution and
the history persistence attempt settle before terminal transcription/subtitle
publication and paste authorization, including failed writes. Assistant stream
chunks and stream done are emitted during LLM work and are outside this ordering
claim. SettingsProfileLoad checks one representative preference through failed
initial load, retry and saved-value restoration without multiplying the existing
full field workload. See [optimization correspondence](evidence/optimization-20261011.md).

The async ownership companions cover captured assistant credentials, microphone
test/effect ownership, two independent history leases, overlapping dictation and
assistant output, whole-command IPC deadlines, and corrupt-cache repair.
[Async ownership evidence](evidence/async-ownership.md) records their exact bounds,
negative variants, and TLC-generated replay into actual production seams.
The normal formal gate checks model/config and raw-counterexample provenance;
editing an event list independently of its TLC State blocks fails the gate.
The [2026-10-19 quality review](evidence/code-quality-20261019.md) records the
credential/theme/configuration/archive regressions, hotword optimization and
resource-limited verification results.
The [2026-10-21 RSI/TDD iteration](evidence/rsi-tdd-20261021.md) records independent
catalog ownership, filter reset/recovery, accessibility, and measured grapheme-work
reduction, together with companion models and implementation replay boundaries.
The [second RSI/TDD round](evidence/rsi-round2-20261022.md) records statistics
freshness, independent query/start ownership, real startup progress interleavings,
the rejected first candidate, and the final formal/replay/mutation checks.
The [third RSI/TDD round](evidence/rsi-round3-20261022.md) records online ASR
credential identity readiness, selective read recovery, and source-backed
interleavings of failed reads, pending siblings and engine changes.

## Reproduce

Use Python 3.10+, Java 21, official TLA+ tools **1.7.4** and Lean **4.34.1**.
Official binary downloads and SHA-256 digests are pinned in
[CI](../.github/workflows/ci.yml).
The stable TLC release avoids the automatically replaced `v1.8.0` prerelease
asset. SHA-256 verification remains mandatory. The runner keeps mutated modules
and their configs together for TLC 1.7.4 module resolution.

```sh
python scripts/check_formal.py --inventory-only
python scripts/check_formal.py --tlc-jar /path/to/tla2tools.jar \
  --lean /path/to/lean-4.34.1-linux/bin/lean --logs /path/to/formal-results
```

The runner saves per-configuration TLC logs, Lean output and a summary with
measured state counts/source hashes. See [delivery results](evidence/results.json),
[coverage boundaries](COVERAGE.md) and [implementation evidence](evidence/source-contracts.md).

The binding gate treats actions and properties separately. Only named actions
with state updates may use execution coverage. Every mapped safety/liveness
property must be an actual configured INVARIANT/PROPERTY, or a positive conjunct
of one. References inside negation, implication, comments or strings never count
as checking that property. Eleven verifier regressions cover this distinction
and equality between the CI and verifier TLC checksum pins.

## Proof boundary

This verifies application **state/protocol contracts**, not every instruction
of compiled Rust/TypeScript/Python, unrestricted Cartesian-product combinations,
or Windows/Tauri/CPAL/WASAPI/keyring/SQLite/browser/network implementations.
Native primitives supply success/failure inputs; concrete parser/URL/length
partitions have implementation tests. Pixel layout, accessibility, floating
point signal conversion and ASR/LLM linguistic quality are not mathematical
claims of this state model; implementation tests remain required.

Lean proves the listed abstract contracts without finite bounds, but does not
provide a verified compiler/source-equivalence bridge. Progress requires stated
scheduler/timeouts/I/O assumptions; generation counters must not wrap.
OAuthLifecycle.MemoryMatchesDisk describes successful atomic publication;
physical partial failures are separately modeled in OAuthStorage. Committed
atomic writes are assumed durable under normal restart. If all marker/deletion
writes fail, logout clears current-process memory and returns an error; impossible
storage writes cannot ensure durable logout across restart.
