# RED slice evidence

Initial test slice. Production was unchanged during RED collection.

Dependencies: locked offline install, `pnpm 10.28.2`, 507 packages reused,
zero downloads; `--ignore-scripts --frozen-lockfile --child-concurrency=1
--network-concurrency=1`. Vitest 4.1.4, TypeScript 5.9.3.

```text
pnpm exec vitest run --maxWorkers 1 src/hooks/__tests__/useAsrEngineSettingsExternalChanges.test.tsx src/hooks/__tests__/useAsrEngineSettings.test.tsx src/hooks/__tests__/useAsrEngineSettingsHydration.test.tsx src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx
```

Revised candidate after rejection of patch `63a2c0b5203c4c262622751d334df6afe8e392a5f69e76b4b4bcdbabfbb8a118`.
The repair changes test/model semantics: both GUI writers' await boundaries,
independent stale field outcomes, cloud readiness and failed-read refresh timing.
README changes describe those semantics; this file records fresh execution.
The final correction retains all previous 79 tests/assertions and adds four
native-completion schedules with the newest engine read still unresolved,
explicit loading ownership/finalization and a sensitive finally negative.

Exit 1: 37 failed / 46 passed, 83 tests in four files, 8.20 seconds. All three
existing suites pass (44 tests). The new file has 39 collected cases: 37 intended
assertion failures; current GUI success and already-safe region read cleanup
remain passing. No missing API,
collection, fixture, or unhandled-rejection failure. Representative RED:

- `listen` expected `engine-config-changed` and a callback; actual zero calls.
- Saved Alibaba expected; actual initial `qwen3-asr-0.6b` after invalidation.
- Deferred GUI completion after unmount still invokes `retryModel` and a success
  toast; both are forbidden by the ownership contract.
- Engine/region handlers issue new persistence after a retired credential flush.
- A retired region completion overwrites/invalidates newer config reads.
- GLM/Alibaba credential readiness remains enabled after invalidation.
- A failed external read does not produce the required exactly-once refresh after
  its remaining config parts settle.
- Retired mutation/finally effects disturb the current loading/state owner while
  the newest `getEngine` promise remains pending.

`pnpm exec tsc --noEmit`: exit 0. `git diff --check`: exit 0.

SANY: exit 0, module semantic processing succeeds. Tool JAR SHA-256:
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
TLC matrix invoked through `run_checks.ps1`, Java 8, 512 MiB heap, one worker.
Runner exit 0: every actual outcome matches the declared positive, semantic
negative, or witness expectation.

| Configuration | Expected actual outcome | Generated | Distinct |
| --- | --- | ---: | ---: |
| AsrAgentSettings | Exit 0, complete, no error | 839295 | 172619 |
| ReadEpochNegative | Exit 12, NoStalePublication | 146 | 100 |
| UiEpochNegative | Exit 12, NoStalePublication | 285 | 173 |
| FlushNegative | Exit 12, NoObsoletePersistence | 61 | 41 |
| FinallyNegative | Exit 12, LatestReadKeepsLoading | 315 | 194 |
| UiReadNegative | Exit 12, NoStalePublication | 1104 | 594 |
| ReadinessNegative | Exit 12, UnknownIdentityBlocksCredentials | 8 | 7 |
| MountedNegative | Exit 12, NoStalePublication | 132 | 89 |
| ListenerOwnerNegative | Exit 12, OnlyCurrentListenerReads | 50 | 36 |
| CleanupNegative | Exit 12, RetiredListenerReleased | 12 | 10 |
| DiagnosticNegative | Exit 12, SubscriptionFailureVisible | 6 | 5 |
| ReadErrorNegative | Exit 12, CurrentReadFailureVisible | 43 | 30 |
| KeyVersionNegative | Exit 12, UserDraftSurvives | 807 | 454 |
| ProgressNegative | Exit 13, temporal property violation | 64381 | 20717 |
| TwoReadsWitness | Exit 12, TwoReadsCompeteNotReached | 1156 | 625 |
| LateListenerWitness | Exit 12, LateListenerNotReached | 48 | 34 |
| DraftOverlapWitness | Exit 12, DraftOverlapNotReached | 171 | 118 |
| RegionMutationWitness | Exit 12, RegionMutationOverlapNotReached | 412 | 248 |

The positive search exhausts all states (queue zero), graph depth 20, 62 seconds.
Negative and witness searches stop at the intended counterexample; their counts
are not exhaustive positive-search counts. All ten positive invariants
and both temporal properties are configured. Witnesses establish that competing
reads, GUI overlap, late listener resolution, key-edit overlap, and the region
post-mutation phase are reachable.
The finally counterexample is semantic: GUI starts, an event starts current read
1 with `engineDone[1] = FALSE`, obsolete flush retires GUI work, then unguarded
`FinalizeUI` clears `engineLoading` and `loadingOwner` from read owner 1 to zero.
The direct hook additions exercise the same cleanup guard after native mutation
success and failure while the current real `getEngine` promise remains pending.

Raw logs and TLC states remain in `evidence/vitest-red-finally.log`,
`evidence/tsc-finally.log` and `evidence/tlc-finally/` in the lane run, outside
the exported source patch. GREEN, coverage, source mutation, production/model
correspondence review, application-wide registry integration, native emission,
and actual desktop verification remain OPEN for main's implementation/integration.
