# Revised test slice: cli-settings-sync

Verdict: READY_REVIEW, replacing rejected candidate
`63a2c0b5203c4c262622751d334df6afe8e392a5f69e76b4b4bcdbabfbb8a118`.
No production changes. Baseline remains
`07ae6efe83686dcb5ee73d8390a0a6ec9bf63603`.

Frozen executable inputs:

| Path | SHA-256 |
| --- | --- |
| src/hooks/useAsrEngineSettings.ts (unchanged baseline) | cace6bebb46f3a371d8efb5b0e3b28334eb6900603443ecaa57a07d0a4e8edb6 |
| src/hooks/__tests__/useAsrEngineSettingsExternalChanges.test.tsx | 408c31719eadf374e103ef74a6c22f90e2f5a33851b4efe95e1f0cfb52e814dd |
| formal/asr-agent-settings/AsrAgentSettings.tla | f947699930a4995d1296008b4b549b9297c5f6d6bbd6cefd649759e485ffb2be |
| formal/asr-agent-settings/AsrAgentSettings.cfg | e4893e69a8b6cb0c8208c77433efbf820cce52d5bb09d930cc413de8162b47fe |
| formal/asr-agent-settings/run_checks.ps1 | c09224047a9217f4b8684f309c2c446ea12206276fa3d47f1a8f1bda1f77cd97 |

All original 44 regression assertions remain intact; their only edits are the
previously authorized default Tauri-listener mocks. Earlier new ownership
assertions remain. The original backwards-engine schedule keeps a common
config-settlement barrier solely to isolate engine completion order; separate
new success/failure schedules use independent old/new endpoint and key promises,
different values, and exact state/error/loading/readiness/refresh snapshots.

The correction is semantic, requiring fresh acceptance:

1. Both engine and region handlers now have deterministic event/unmount tests
   while awaiting credential flush, native mutation success/failure, and
   post-mutation cloud reads. Retired flush cannot issue persistence. Retired
   completion cannot start reads, overwrite state, retire pending fresh reads,
   or publish stale success/error/model refresh effects.
2. Independent old config parts resolve/reject only after newer parts settle.
   Their completions must preserve all observed current config fields, error,
   loading/readiness and exact refresh count. Initial late failures remain covered.
3. Failed current engine reads retain the remaining endpoint/key barriers:
   `retryModel` must run exactly once after both settle. Retired failures must add
   no refresh. GLM and Alibaba readiness is invalidated immediately, protecting
   direct key and region handlers while the new cloud identity is unknown.
4. The remaining loading gap adds exactly four engine/region success/failure
   schedules. Newest `getEngine` remains unresolved through stale native
   completion; loading stays true, all state/effect snapshots stay unchanged,
   then current reads settle with exactly one model refresh. All earlier 79
   tests/assertions remain intact.

The formal correction adds explicit engine/region kind and GUI flush,
persistence, and post-read phases; independent endpoint readiness; three guard
negatives and a region-phase witness. The final repair adds `engineLoading`,
`loadingOwner`, guarded `FinalizeUI`, `LatestReadKeepsLoading` and the semantic
`FinallyNegative` control. The README states finite lifecycle/action
bounds explicitly, including the restriction of remount to the listener-replay
workload before GUI/events. It does not claim arbitrary GUI work spanning remount.

Executed commands, from the test worktree:

```text
pnpm exec vitest run --maxWorkers 1 src/hooks/__tests__/useAsrEngineSettingsExternalChanges.test.tsx src/hooks/__tests__/useAsrEngineSettings.test.tsx src/hooks/__tests__/useAsrEngineSettingsHydration.test.tsx src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx
pnpm exec tsc --noEmit
pwsh -NoProfile -File ./formal/asr-agent-settings/run_checks.ps1 -Java <Java8 executable> -Jar <pinned tla2tools.jar> -OutputDirectory <lane evidence/tlc-finally>
git diff --check
```

Results: Vitest exit 1, 37 semantic assertion failures / 46 passes (83 collected;
new 39 cases, old 44 passing). No collection, fixture, timeout, type or unhandled
rejection failures. TypeScript and whitespace checks exit 0. SANY exits 0.
All 18 TLC configurations have their expected result: positive complete with
839295 generated / 172619 distinct states, depth 20; twelve named safety
negatives, one temporal negative and four named reachability witnesses.
Exact per-config counts and raw-log locations are in RESULTS.md.

Main owns gate acceptance, production implementation, GREEN, source/model
correspondence, coverage, source mutation, formal-registry integration and actual
native/desktop evidence. This slice proves no native emitter or live GUI claim.
