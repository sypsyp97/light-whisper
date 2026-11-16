# Application formal contracts

This directory is source-controlled verification code and regression data.
Keep models, configurations, Lean proofs, the coverage registry and reviewed
replay fixtures in Git. Store per-run logs, model-checker state and downloaded
tools under the ignored `tmp/` directory; compiled Lean output is ignored too.

The application state model covers recording/capture/hotkeys, all four engine
classes, credentials/OAuth, providers/settings/personalization, context/routing,
selection/assistant, history, windows/subtitles, updates, theme/autostart and
bundled protocols.

[`verification.json`](verification.json) binds **132 registered commands**,
**22 event families**, **20 lifecycle entries** and **20 protocol families** to
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
| CLI companion matrices | Separate required `CLI contracts` CI job: command/session/configuration/bootstrap/target/thread contracts, artifact delivery, private files, authenticated transport, managed queries, headless host, release gates and mounted settings invalidation. Each runner retains its own configured properties, semantic controls and reachability witnesses. |
| Lean StateContracts | Parameter-independent ownership/decision contracts, profile-version induction, first matching rule, consent, clipboard preservation, sample cap, correction provenance and theme. No sorry, admit or declared axiom. |
| [Verus implementation proofs](verus/README.md) | Required CI verification of production capture arithmetic, hotkey ownership transitions and interim interval control, plus typechecked negative controls. Caller wiring, synchronization and mixing retain separate Rust tests and review. |
| Implementation correspondence | Actual regression tests and source-path review, including a source guard requiring provider/endpoint/key snapshots before OAuth awaits. This is not compiler refinement. |
| Negative controls | Safety mutants must fail with Java exit 12 and the named violated invariant. Separately labeled boundary witnesses use intentional reachability-target violations; they are not safety proofs. Required CI gates. |

## Model families

| Area | Models |
| --- | --- |
| Recording and capture | RecordingLifecycle, CapturePipeline, MicrophoneMonitor, MicrophoneAsyncOwnership, MicrophoneEffectOwners |
| Hotkeys | HotkeyLifecycle (three event-kind runs plus registration workload), HotkeyPendingStart, HotkeyRegistrationEpoch, HotkeyRegistrationFailure |
| Engines and migration | EngineDownload, RuntimeConfiguration, EngineConfigurationWrites, ModelDirectoryMigration, ModelIntegrityRepair, EngineStatusOwnership |
| GPU residency and settings | GpuIdleLifecycle (startup and runtime enable), GpuIdleSettings |
| Authentication and provider pairing | OAuthLifecycle, OAuthStorage, ProviderRequests |
| Credential publication and account snapshots | WebSearchKeys, ProviderAuthSnapshot, AssistantCredentialOwnership, VisionCredentialOwnership, CatalogDiscoveryOwnership, ModelCatalogLoadingOwnership, OnlineAsrIdentity |
| Settings and personalization | ProfilePersistence, SettingsContracts, Personalization, AppProfileRules, AsyncSettings |
| Routing, tasks and history | ContextRouting, TaskOwnership, HistoryRecords, HistoryLeaseInterleavings, HistoryAudioGcOwnership, OutputDrainOwnership, HistoryFilterOwnership, HistoryMutationFilterOwnership, HistoryStatsOwnership |
| Assistant and selection publication | AssistantRetryOwnership, SelectionPublicationOwnership |
| Windows and UI state | WindowLifecycle, UiPreferences, ThemeSynchronization |
| Download timers and event subscriptions | DownloadUiLifecycle, DownloadTaskEventOwnership, EventSubscriptionLifecycle |
| Protocols and transport | RequestProtocols, LlmTransport, LlmRequestParameters, IpcCommandDeadline |
| CLI and desktop agent | `cli/` companions, `agent-api-security/PrivateFiles`, five `agent-api-transport/` companions, `agent-api-app/AppQuery`, `agent-api-host/AgentHost`, `agent-api-host/admission/ControllerAdmission` |
| CLI delivery and external settings invalidation | `cli-packaging/Packaging`, `cli-ci/ReleaseGate`, `asr-agent-settings/AsrAgentSettings`, `asr-subscription-bootstrap/AsrSubscriptionBootstrap` |

AppWorkflow is the additional integrated model. For the application runner, the
registry is the authoritative positive configuration/negative-control list. Operator names and concrete
source/test anchors are recorded per contract there.

The root `runs`, `actions` and `negative` registry entries remain the application
runner's execution matrix. `check_model_bindings` checks configured properties
and observed actions only for models visited through `runs`. The path-qualified
CLI companion bindings are source/operator anchors; adding them does not cause
the root runner to execute those models or check their property activation and
reachability. Inventory success alone is not a completed CLI proof.

The separate required [`CLI contracts` job](../.github/workflows/ci.yml) executes
all fifteen CLI matrix runners, the eighteen-case mounted-settings runner
and the sixteen-case subscription-bootstrap runner.
The existing runner commands are in its `Run complete CLI formal matrices`
step. Each positive configuration, semantic negative and reachability witness
must complete through its owning runner. Final correspondence review must
match the configured properties and completed receipts to current source,
test, model and configuration hashes; registry anchor checks cannot replace it.
The companions retain their own bounds and environmental assumptions, documented
in [CLI commands](cli/CommandSurface.md), [private files](agent-api-security/README.md),
[transport](agent-api-transport/README.md), [managed queries](agent-api-app/README.md),
[headless host](agent-api-host/README.md), [delivery](cli-packaging/README.md),
[release gates](cli-ci/README.md), [settings invalidation](asr-agent-settings/README.md)
and [subscription startup](asr-subscription-bootstrap/README.md).

`engine-config-changed` is an invalidation signal emitted by native GUI and
agent engine setters after their successful backend transition. The settings
hook rereads saved configuration and retires old read/mutation/listener owners;
it does not trust event payloads as saved state. The mounted-settings model begins
with a delivered event. The bootstrap companion checks listener acquisition,
initial reads, fallback and disposal. Hook regression tests cover overlapping
read, mutation and finalizer ownership. The combined transition, invalidation
and effect-replay model is outside the accepted verification scope. Native
emission ordering and actual GUI delivery require separate runtime evidence.

AppWorkflow separates routed external search and fresh screen preparation from
LLM dispatch. Both preparations settle before dispatch, and fresh screen payloads
are checked against the foreground identity sampled after the join. This sample
does not prevent later OS focus changes. ModelNative search executes inside the
LLM request; reused pre-polish descriptions retain their existing semantics and
have Rust regression coverage.

Terminal transcription/subtitle publication and paste authorization precede
history audio encoding and persistence. Consent-gated raw samples are retained
before final ASR consumes its buffer. History persistence can settle while an
automatic paste remains queued; successful paste timing later updates the exact
inserted row, without recreating deleted records. Assistant stream chunks and
stream done remain outside this terminal ordering claim. The bounded model and
implementation checks are described in [history evidence](evidence/history.md).
SettingsProfileLoad checks one representative preference through failed
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
The [2026-11-11 state-machine audit](evidence/state-machine-20261111.md) records
seven additional companions for history GC, selection publication, assistant
retry, mutation refresh, catalog loading, vision credentials and download tasks.
They add 22 safety negatives and two separately labeled boundary reachability
witnesses. Their finite bounds and positive state counts are listed in
[coverage](COVERAGE.md); implementation tests and source review remain the
correspondence boundary.

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
measured state counts/source hashes. Each TLC invocation has a 512 MiB Java heap
cap and one worker. See [delivery results](evidence/results.json),
[coverage boundaries](COVERAGE.md) and [implementation evidence](evidence/source-contracts.md).

The binding gate treats actions and properties separately. Only named actions
with state updates may use execution coverage. Every mapped safety/liveness
property must be an actual configured INVARIANT/PROPERTY, or a positive conjunct
of one. References inside negation, implication, comments or strings never count
as checking that property. Thirteen verifier regressions cover this distinction,
equality between the CI and verifier TLC checksum pins, and the bounded TLC
subprocess invocation. The resource regression mocks Java; it is not a model run.

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
