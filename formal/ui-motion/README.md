# UI motion verification

The behavior suite was written against `c28d286ba971881c59fcce391958886f3bd4af2e`. Its baseline run found 20 failing tests and 66 passing tests. The failures showed the outgoing route delaying a ready page, a pending route preventing another choice, load rejection missing a toast, overlay audio samples rerendering the transcript, height changes during waveform updates, per-grapheme subtitle nodes, looping recording rings, a remounted status, missing polishing feedback and visible cancellation text, and history stagger.

From the application root, run the same focused suite with one worker.

```powershell
pnpm exec vitest run src/main.motion.test.tsx src/pages/__tests__/SubtitleOverlay.test.tsx src/pages/__tests__/MainPageMotion.test.tsx src/components/RecordingButton.test.tsx src/components/StatusIndicator.test.tsx src/perf/appFluidity.perf.test.tsx --maxWorkers=1
```

## Source correspondence

| Model | Application boundary | Checked behavior | Behavioral replay |
| --- | --- | --- | --- |
| `PageNavigation` | `src/main.tsx`, `App` and `navigateTo`. The request object owns import settlement. The loaded component and transition class are committed together. Effect cleanup clears the owner. | New choices supersede pending ones. A ready current choice is published in the same settlement step. The current failure clears pending state and reports feedback. Stale and retired completions have no effect. If each requested import settles, its owner eventually clears. | `src/main.motion.test.tsx` controls import completion order and success/failure, holds timers fixed, and accepts Back while the entry class is present. |
| `SubtitleWaveform` | `src/pages/SubtitleOverlay.tsx`, `SubtitleWaveform`, `acceptsWaveformSample` and `applyRecordingState`. The listener closure owns its generation. The parent session and phase refs gate the event. The capsule key resets the child for a new session. The child owns the bars. | Samples from the current listener, current session and active phase alone can change levels. An accepted active lifecycle gives the capsule the current session, including when an early transcript already advanced the latest session ref. Old levels clear when the child retires. A sample cannot change transcript render state. Retired listener acknowledgements release their registration. | `src/pages/__tests__/SubtitleOverlay.test.tsx` checks active and stale samples, fixed transforms, unchanged parent render count, session reset, an interim transcript before recording state, subscription removal, an old callback after remount, and listener acknowledgement after retirement. |
| `MainPolishStatus` | `src/hooks/useRecording.ts` through `RecordingContext`, `MainPage` and `StatusIndicator`. Recording state claims the current dictation owner. Final transcription and polish provider events settle its presentation state. | Foreign and inactive signals cannot change the label. Recoverable errors leave the stage unchanged. Applied or unchanged stages cannot reopen on a late fallback. Final result and capture-phase changes clear active polishing. | `src/pages/__tests__/MainPageMotion.test.tsx` drives real Tauri-facing hook handlers through the provider, checks status identity and text, and verifies that Use original retains its existing session and click path. |

Navigation bounds four requests over three views. Each import's nondeterministic success and failure is represented by one settlement action, including settlement after retirement. Deduplicating a repeated same-target click and prefetching a module without acquiring route ownership are outside the model. It assumes eventual import settlement when checking progress; it does not require finite elapsed milliseconds.

Waveform bounds two listener lifetimes and two backend session identifiers. The two-level abstraction represents silence and a live sample. Listener callbacks may run before acknowledgement or after disposal. `Interim` advances `latestSession` with the transcript listener while preserving the capsule owner `session`. `Lifecycle` covers accepted recording states and abstracts both a same-node starting-to-recording change and a child reset on phase or session exit. It assumes valid, increasing revisions for the lifecycle events in this schedule. `OwnedLevels` includes `ActiveLifecycleOwnsSamples`, which requires the accepted active lifecycle to bind the capsule owner to the current session. The late-state negative control preserves the old capsule key when the interim event advanced the ref before the lifecycle. The witness reaches the correct lifecycle transition with a prior capsule in place. Under acknowledgement fairness, a disposed pending registration is eventually released. CSS scale geometry, bar count and DOM identity are checked by component and CSS tests.

Polish bounds two sessions and three lifecycle revisions. The model projects listening and non-polish processing onto the phase states that determine status ownership. It treats applied and unchanged as terminal and the transport error as recoverable. The model assumes no backend completion deadline and checks safety of the visible phase, including out-of-order and stale events. The cancellation API's pending-stop and response ownership is covered by the existing `src/hooks/__tests__/useRecordingPolishCancellation.test.tsx` suite.

The grapheme boundary, tail animation, reduced-motion behavior, stable rows and WAAPI targets are covered by the behavior and CSS checks. The finite models cover ownership and event ordering.

## Reproduce TLC checks

Run from `formal/ui-motion` using the local pinned TLA+ tools JAR. The recorded runtime was Java 8 and TLC 2.19, revision `5a47802`. The JAR SHA-256 was `936A262061C914694DFD669A543BE24573C45D5AA0FF20A8B96B23D01E050E88`.

```powershell
$motionModels = [ordered]@{
  PageNavigation = 'Nav*.cfg'
  SubtitleWaveform = 'Wave*.cfg'
  MainPolishStatus = 'Polish*.cfg'
}
$motionOutput = Join-Path ([IO.Path]::GetTempPath()) ('lw-motion-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $motionOutput | Out-Null
foreach ($motionCase in $motionModels.GetEnumerator()) {
  $motionModule = $motionCase.Key
  java -cp $env:TLA2TOOLS_JAR tla2sany.SANY "$motionModule.tla"
  if ($LASTEXITCODE -ne 0) { throw "SANY failed for $motionModule" }
  $motionConfigs = @(Get-Item "$motionModule.cfg") + @(Get-ChildItem -Filter $motionCase.Value)
  foreach ($motionConfig in $motionConfigs) {
    $motionResults = Join-Path $motionOutput $motionConfig.BaseName
    java -XX:+UseParallelGC -Xmx256m -cp $env:TLA2TOOLS_JAR tlc2.TLC -workers 1 -coverage 1 -metadir $motionResults -config $motionConfig.Name $motionModule 2>&1 | Set-Content -Path "$motionResults.log"
    "$($motionConfig.BaseName) exit $LASTEXITCODE"
  }
}
```

The primary configuration for each module should complete with exit 0. A safety negative must violate the named invariant with exit 12. A progress negative must report a temporal-property violation with exit 13. A witness must reach its named target with exit 12. The receipt records the checked configurations and explored state counts.
