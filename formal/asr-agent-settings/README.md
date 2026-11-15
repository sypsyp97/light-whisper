# Saved engine invalidation in mounted settings

This test slice exercises `src/hooks/useAsrEngineSettings.ts` through its public
hook handlers and the real Tauri `listen` boundary. Tauri, local config/keyring,
and catalog calls are mocked; all keys and URLs are synthetic. It does not drive
the desktop, native event emitter, credentials, models, providers, or network.

The hook uses configuration generations to retire old reads and GUI completions
when saved settings change. Its event callback reads saved state instead of
trusting event payloads. This model begins with a delivered invalidation;
[subscription bootstrap](../asr-subscription-bootstrap/README.md) covers listener
installation. Hook tests cover additional overlap schedules; the coupled
transition, invalidation and effect-replay combination is not formally qualified.
Current source identities and measured
results are in the [1.8.0 verification record](../evidence/agent-cli-180.md).

| Observable contract | Source owner / call path | Deterministic hook schedule | Model / configured property |
| --- | --- | --- | --- |
| Events invalidate; fresh saved engine supplies data | SettingsPage -> useAsrEngineSettings -> getEngine | Spoofed payload disagrees with saved Alibaba engine | Event -> captured read epoch; NoStalePublication |
| New events retire earlier config reads | configVersion, readAsrConfig | Two getEngine promises complete backwards; separate old/new endpoint and key promises return different values or reject after the newer round; initial hydration also arrives late | Event, CompleteEngine, CompleteEndpoint, CompleteKey; NoStalePublication |
| Unknown saved cloud identity blocks credential access | engineReady, endpointReady, keyReadVersion | Both GLM and Alibaba start ready, event arrives, engine/endpoint remain pending, direct key/region handlers remain blocked | Event, engineKnown, endpointKnown; UnknownIdentityBlocksCredentials |
| Retired credential flush cannot start a new mutation | handleEngineSwitch / handleOnlineAsrRegionChange -> onlineAsrKeySave.flush | Event or unmount while save is pending, then save resolves; neither setEngine nor setOnlineAsrEndpoint may start | BeginUI, CompleteFlush; NoObsoletePersistence |
| New saved state retires both GUI mutation completions | engine switch -> setEngine; region change -> setOnlineAsrEndpoint | Each native mutation succeeds or fails after event/unmount; stale completion cannot start reads, change loading/error/readiness, or retire pending fresh reads | BeginUI(kind), CompleteUI; NoStalePublication |
| Pending current engine read retains loading ownership | readAsrConfig engine finally; handleEngineSwitch finally; region completion must preserve the same shared state | Four engine/region success/failure schedules leave newest getEngine unresolved through old native completion; loading remains true, then exactly one refresh follows current settlement | Event, CompleteEngine, FinalizeUI, loadingOwner; LatestReadKeepsLoading |
| New saved state retires post-mutation reads | both GUI handlers -> readAsrConfig | Event or unmount while cloud key/endpoint reads are pending; old key read rejects without stale toast or model refresh | CompleteUIRead; NoStalePublication |
| New saved state retires catalog responses | modelsRequestId, refreshAlibabaModels | Old cloud result and warning arrive after local selection | Same config-epoch retirement abstraction; direct hook assertions retain source detail |
| User key edits keep their own read generation | keyReadVersion, handleOnlineAsrApiKeyChange | Current engine/endpoint resolve, user edits, old key read resolves | EditKey, CompleteKey; UserDraftSurvives |
| Existing credential save slot stays captured | useDebouncedCallback -> setOnlineAsrApiKey | Queued Alibaba international edit survives external local selection | Direct regression test; queue internals remain in existing formal replay family |
| Current read failures remain visible and retryable | configReadErrors, retryAsrConfig | Engine read fails before endpoint and key settle; exactly one refresh after all settle; existing retry gets saved local engine | CompleteEngine failure, CompleteEndpoint, CompleteKey, Refresh; CurrentReadFailureVisible, LatestReadRefreshes; retry action covered directly |
| Subscription failure is visible | mount effect -> listen rejection | Registration rejects; diagnostic observed; no writes/catalog call | ResolveSubscription failure; SubscriptionFailureVisible |
| Unmount retires async work | mounted + per-effect disposal identity | External read and GUI promise settle after unmount | Unmount, CompleteEngine, CompleteUI; NoStalePublication |
| Async listener cleanup releases the returned listener | effect cleanup -> async listen completion | Unmount first, registration later returns exact stop function | ResolveSubscription, Unmount; RetiredListenerReleased, RetiredSubscriptionSettles |
| StrictMode old callbacks cannot acquire new effect ownership | effect-local disposal, configVersion | Old registration resolves after replay; old callback ignored, current callback refreshes | Remount, Event; OnlyCurrentListenerReads |
| Latest settled external read refreshes model status | external read continuation -> retryModel | Latest engine arrives before config parts; refresh only after settlement; old result adds no refresh | Refresh; LatestReadRefreshes |

Finite scope: two external writes/events and two config reads; one GUI mutation
choosing engine or region, with separate credential-flush, native-persistence,
post-mutation-read and finalization phases; two effect owners; independent engine, endpoint,
and key completions; success/failure of engine reads, native mutations,
post-mutation reads and subscriptions; one key edit per config epoch. Epochs
are monotonically increasing within this bound, never wrapping. The initial
hydrated cloud configuration is abstracted as `displayed = 0` (Alibaba); values
1/3 represent GLM and value 2 Alibaba. Direct hook tests cover initial hydration.
The endpoint has its own completion and readiness state. Catalog payloads share
the config epoch abstraction; concrete catalog behavior is checked directly.
GUI success/error/finally side effects share GUI ownership in the model and
have direct success/failure/unmount tests.
The shared engine loading state has an explicit owner (GUI or current read).
`FinalizeUI` abstracts cleanup that could affect that shared state. The source's
engine handler writes `engineLoading` in its finally; the region handler's
finally writes its separate region loading flag. Direct region schedules also
assert that no stale region effect interferes with current engine loading.

The lifecycle workload permits one remount before any external event or GUI
mutation; GUI mutation starts on owner 1. Configuration workloads can unmount
at every GUI/read phase. This keeps listener replay and GUI retirement finite
without claiming arbitrary GUI operations spanning multiple remounts. These are
explicit action bounds; there are no state constraints or symmetry reductions.

Weak fairness assumes admitted local reads, GUI promises, and listener
registration eventually settle; the internal refresh continuation is scheduled.
It does not assume external events, user input, or unmount happen. `Next` includes
explicit idle stuttering for a mounted window waiting for native/user input.
No symmetry, state constraints, search-depth cutoff, or disabled deadlock checks.

Run from the repository root, supplying the verified Java executable, pinned JAR and
an isolated output directory outside source-controlled `formal/`:

```powershell
& ./formal/asr-agent-settings/run_checks.ps1 -Java $javaPath -Jar $jarPath -OutputDirectory $evidencePath
```

`run_checks.ps1` verifies the pinned tool digest, preflights the one module with
SANY, then runs the positive config, twelve safety negatives, one progress
negative, and four separately labeled reachability witnesses. It checks exact
named invariant/temporal failures rather than accepting arbitrary nonzero exits.
Positive liveness checks use one worker and a 512 MiB heap. Raw logs, TLC states
and `results.json` go only to the supplied evidence directory.

Negative controls remove read epoch, GUI epoch, post-flush ownership, finally ownership,
post-mutation-read ownership, readiness invalidation, mounted, listener owner,
listener cleanup, diagnostic, read-error, key version, and refresh progress mechanisms.
Witnesses separately reach two competing reads with a pending GUI set, a late
retired listener after remount, a user edit overlapping a current key read, and
an explicit region mutation waiting for post-mutation reads across an event.
They are reachability evidence, not guard kills.

Coverage and source mutation require a GREEN production implementation. They
remain integration gates owned by main. The new model is scoped verification
code; registration in the application-wide formal inventory is a separate main
integration step. Native emission and actual desktop observation remain separate
evidence, regardless of the jsdom or finite-model result.
