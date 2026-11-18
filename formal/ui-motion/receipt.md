# Check receipt

The three SANY preflights passed. Each TLC run used one worker. Positive runs exhausted the respective finite state space. Witness and negative counts reflect the search when the counterexample was found.

| Configuration | Result | Named check | Generated | Distinct | Depth |
| --- | --- | --- | ---: | ---: | ---: |
| PageNavigation | Exhausted (0) | All configured properties | 23,405 | 7,566 | 10 |
| NavClosedOwner | Invariant violated (12) | UnmountReleases | 12 | 10 | 3 |
| NavFailureLock | Invariant violated (12) | FailureSettles | 10 | 8 | 3 |
| NavFailureProgress | Temporal property violated (13) | EventuallySettled | 25,009 | 8,086 | — |
| NavLegacyDelay | Invariant violated (12) | ReadyPublishesImmediately | 11 | 9 | 3 |
| NavLegacyLock | Invariant violated (12) | OnlyLatestChoice | 5 | 5 | 3 |
| NavStaleCommit | Invariant violated (12) | NoStalePublication | 25 | 20 | 4 |
| NavStaleToast | Invariant violated (12) | NoStalePublication | 24 | 19 | 4 |
| NavSupersededWitness | Witness reached (12) | NoSupersededCompletionWitness | 212 | 155 | 5 |
| NavUnmountWitness | Witness reached (12) | NoPostUnmountCompletionWitness | 53 | 41 | 4 |
| SubtitleWaveform | Exhausted (0) | All configured properties | 23,243 | 1,518 | 10 |
| WaveAfterRemountWitness | Witness reached (12) | NoCallbackAfterRemountWitness | 928 | 306 | 6 |
| WaveInactivePhase | Invariant violated (12) | OwnedLevels | 306 | 120 | 5 |
| WaveLateAckWitness | Witness reached (12) | NoLateAcknowledgementWitness | 198 | 81 | 5 |
| WaveLateLeak | Invariant violated (12) | RetiredReleased | 27 | 21 | 4 |
| WaveNoReset | Invariant violated (12) | OwnedLevels | 337 | 127 | 5 |
| WaveParentRender | Invariant violated (12) | IsolatedRender | 42 | 33 | 4 |
| WaveReleaseProgress | Temporal property violated (13) | EventuallyReleased | 32,927 | 2,128 | — |
| WaveStaleListener | Invariant violated (12) | OwnedLevels | 891 | 286 | 6 |
| WaveWrongSession | Invariant violated (12) | UnownedCallbackIgnored | 44 | 35 | 4 |
| MainPolishStatus | Exhausted (0) | All configured properties | 62,371 | 2,926 | 8 |
| PolishActiveWitness | Witness reached (12) | NoPolishWitness | 201 | 141 | 3 |
| PolishError | Invariant violated (12) | RecoverableErrorKeepsStage | 213 | 153 | 3 |
| PolishFinal | Invariant violated (12) | FinalClearsPolishing | 3,686 | 639 | 4 |
| PolishForeign | Invariant violated (12) | OnlyOwnerChangesLabel | 200 | 140 | 3 |
| PolishLateFallbackWitness | Witness reached (12) | NoFallbackAfterAppliedWitness | 3,855 | 658 | 4 |
| PolishReopen | Invariant violated (12) | DoesNotReopen | 3,852 | 655 | 4 |
| PolishReset | Invariant violated (12) | ClearsOnPhaseExit | 3,658 | 611 | 4 |
| PolishTerminal | Invariant violated (12) | DoesNotReopen | 3,678 | 631 | 4 |

The receipt binds the inputs as bytewise SHA-256. The baseline Vitest command in the README exited 1 with 20 behavioral failures and 66 passing tests.

```text
bdc8ede5361572f7b98c21d6838094492e3eee874d8470e7fe48594cd6f70f2f  formal/ui-motion/PageNavigation.tla
1653f8aa03019b32a7c70fb19866950575fc54e99e330bf7469e2a99479f7a5d  formal/ui-motion/PageNavigation.cfg
de5c4c28e938cd6828ee1b6653be488c4918825473b186f2b4aeb2d1615c5068  formal/ui-motion/NavClosedOwner.cfg
c99fe3cbd8780e38a6bdd42f7446d04363969ef365dd3d3db7c6b7c10804711d  formal/ui-motion/NavFailureLock.cfg
27ec2437f2cd178c4f4efb385d01c96f3b94f4a77dc0c0133f7f308614d3d3fa  formal/ui-motion/NavFailureProgress.cfg
f7b0b9af216a1647ca50215e0d658a675c28492caf636fb46ed0aa0758c58a99  formal/ui-motion/NavLegacyDelay.cfg
6d0e99bacdc97bd39b230ef6c514d1da0d5d59306e793f5a3e1052ae3fc68ba4  formal/ui-motion/NavLegacyLock.cfg
07525eb09f1e5b9a1f8ad12391b7306e200faa9d125b46f3ffcbfcafd443efe0  formal/ui-motion/NavStaleCommit.cfg
4e0909c03812bb8e889e90652748dbbc764d72e49da2ad6d06e37800babfc696  formal/ui-motion/NavStaleToast.cfg
4825c6582105a34184c9ca004f6bb693ebd3e8615d6e86b35c659c1653668363  formal/ui-motion/NavSupersededWitness.cfg
d60831de457bc1300427ec45733c1f30b76759e2446debd8fd5105bf037657e4  formal/ui-motion/NavUnmountWitness.cfg
6702b27a3548f98fbf21db4cf2d9c5ce9a1678f1056d8ebb979198e4abeaaf96  formal/ui-motion/SubtitleWaveform.tla
cbf031087e634bcb73ba0579891593a740012a557480d9520679e8ac0694735b  formal/ui-motion/SubtitleWaveform.cfg
739ce743014a0015862f7d4bb22de3886cd84c7a89dbcfb4eb38a4f38711aae1  formal/ui-motion/WaveAfterRemountWitness.cfg
a7b705fe8ddb41e69cf5b68ed7bcacb5a50d534f537ebf39b3d8ce750e460b51  formal/ui-motion/WaveInactivePhase.cfg
8f735fdeddb6deef49d83e039cbeb8d71f458f4605369971526c5ae3b25e7fea  formal/ui-motion/WaveLateAckWitness.cfg
51530d8c5ddb04856a6e53b9508f41f79c493e1117b19ee599b35a5f829543b8  formal/ui-motion/WaveLateLeak.cfg
dadeb04964cede48e11a989e7e345758649a2cdd6e9c8fde35755ef6a2e52238  formal/ui-motion/WaveNoReset.cfg
4866ec8595daf9678ca6228f06ece84fbe40bc06f7ecb076d24f3eb02df7466c  formal/ui-motion/WaveParentRender.cfg
f5221993a0a94fc1e0eedb499f1d2db9c998fe4fd8005bb7a617e18543af1fa5  formal/ui-motion/WaveReleaseProgress.cfg
47200ea9a6df82cf260c7858c4122d83d1d9b09c97ba86f89e316dacd976548a  formal/ui-motion/WaveStaleListener.cfg
fd1911ecccff2598ea45497250877a6c0ad1c608518fccc79667f1269ff25949  formal/ui-motion/WaveWrongSession.cfg
066546ef9b6cf146148d9e85cbefc5158972be669e595031d42945a23c36cfef  formal/ui-motion/MainPolishStatus.tla
40d4f1dc98ea0648bb657c72358738e527f656488a20a02be6a80c7df9996bf9  formal/ui-motion/MainPolishStatus.cfg
3fad58c48633e1bb6d0379c92da9fbab160a63aa9872723595ba7495ee30d877  formal/ui-motion/PolishActiveWitness.cfg
36d0dca67be4f0ae13dd2a1aadeefc93cce2f91db7953cbfb28093e3414d4c52  formal/ui-motion/PolishError.cfg
e5320a7061b5751d53d741671453e75c247cf7fe8b25fc259baf835db41407d3  formal/ui-motion/PolishFinal.cfg
6538747432c3f6cc11631df4053108d4b6925ec51b10895638979175beccd6f4  formal/ui-motion/PolishForeign.cfg
3c64c9e02fd44695ab4b334b49943f6a540ed2567b2f0c3befd3c6261de3409f  formal/ui-motion/PolishLateFallbackWitness.cfg
d5b1efaaedcf3deb7b8883dc6d1294085cc909ec2ccfd29d1db24b0c80a1fa99  formal/ui-motion/PolishReopen.cfg
a3d1270c546da1d7dec3007dfc497f2e982603440caaf2ef259266834a6d4cdf  formal/ui-motion/PolishReset.cfg
8c0716f4062eda1f6c6d51db3585c4072c1c5ad2a8a5ac3171658e57bf197202  formal/ui-motion/PolishTerminal.cfg
6769207dc9182621371377eb1849f21cc46c2839d6696700f0f088ff253632ee  src/main.motion.test.tsx
10bad2d6daaaa37d6466cdb3f97c4bacf1f339322a52e3a4d40f33789d06edda  src/pages/__tests__/SubtitleOverlay.test.tsx
fb2196fa21b45ac3147c60c8369ac6a668662119f3867d25de4edc0bc4dd4dc6  src/pages/__tests__/MainPageMotion.test.tsx
0205ce76b9af43e3b1383881a345ba41e2bd7cc75aee913136cb97aca30bc052  src/components/RecordingButton.test.tsx
aa85e9860222023454f4c95ace0a5bbf555de527fb746bd8df0c71986edac9da  src/components/StatusIndicator.test.tsx
db8edd1ca46b90afc5eaebfc5cc5d56f711a8bf605faa5e07102097cb603ea48  src/perf/appFluidity.perf.test.tsx
```

## Navigation and waveform follow-up

The regressions below use the integrated baseline `4268282ff942b745271eb31a6e3b4c2243e49fdf`.

Run from the application root to replay the two regressions.

```powershell
pnpm exec vitest run src/main.motion.test.tsx --maxWorkers=1
pnpm exec vitest run src/pages/__tests__/SubtitleOverlay.test.tsx -t 'accepts the current waveform when its interim transcript arrives before recording state' --maxWorkers=1
```

- Navigation had one failing test and seven passing tests. The test checks DOM visibility in the import settlement turn, before another navigation click. The ready module was still absent from the DOM at that boundary. Ready imports now publish through `flushSync`.
- The targeted waveform test had one failing test and 67 skipped tests. An interim transcript from session B advanced the latest session while session A still owned the capsule. The accepted recording state for B cleared the transcript and left nine bars visible. A stale sample from A left all bars at scale 0.125 as expected. A sample from B should have set all nine scales to 0.5, but they stayed at 0.125. The regression uses increasing lifecycle revisions, which are assigned from the shared `snapshot_revision` counter in `src-tauri/src/state/app_state.rs`.

SANY passed for the revised `SubtitleWaveform` model. TLC ran with two sessions, two listener lifetimes, one worker and a 256 MiB heap. The positive configuration checked `ActiveLifecycleOwnsSamples` through `OwnedLevels`. The late-state counterexample retained a cold capsule key of 0 after an early transcript advanced the latest session to 1 and the matching Starting lifecycle was accepted. The witness reached the prior-capsule path with session 1 starting, session 2's interim transcript and then session 2 recording with the capsule bound to 2.

| Configuration | Result | Named check | Generated | Distinct | Depth |
| --- | --- | --- | ---: | ---: | ---: |
| SubtitleWaveform | Exhausted (0) | All configured properties | 38,914 | 2,530 | 11 |
| WaveAfterRemountWitness | Witness reached (12) | NoCallbackAfterRemountWitness | 1,454 | 450 | 6 |
| WaveEarlyInterimLifecycleWitness | Witness reached (12) | NoEarlyInterimLifecycleWitness | 515 | 183 | 5 |
| WaveInactivePhase | Invariant violated (12) | OwnedLevels | 562 | 190 | 5 |
| WaveLateAckWitness | Witness reached (12) | NoLateAcknowledgementWitness | 262 | 123 | 5 |
| WaveLateLeak | Invariant violated (12) | RetiredReleased | 31 | 25 | 4 |
| WaveLateState | Invariant violated (12) | ActiveLifecycleOwnsSamples | 41 | 32 | 4 |
| WaveNoReset | Invariant violated (12) | OwnedLevels | 596 | 199 | 5 |
| WaveParentRender | Invariant violated (12) | IsolatedRender | 70 | 54 | 4 |
| WaveReleaseProgress | Temporal property violated (13) | EventuallyReleased | 54,582 | 3,500 | — |
| WaveStaleListener | Invariant violated (12) | OwnedLevels | 1,416 | 427 | 6 |
| WaveWrongSession | Invariant violated (12) | UnownedCallbackIgnored | 72 | 56 | 4 |

Bytewise SHA-256 for the revised test and model inputs.

```text
8d55bc514b75fabb3cc872c9154d3600fcef43c40ce301a36c23ddb9968fb9f1  src/main.motion.test.tsx
134f0dd8948149b196aee1d694273c93edfad11526be99e4fec63cbb5e78e86b  src/pages/__tests__/SubtitleOverlay.test.tsx
72c139ecbeaade0f2d125c4fb57b6872bc2983af58dc7b868d663b6e43689609  formal/ui-motion/SubtitleWaveform.tla
a837eec1f1903dc6116465e80e759fa7c97cf754406dfeb0bddcd7b92fb31fab  formal/ui-motion/WaveEarlyInterimLifecycleWitness.cfg
3ef79f6f2fa0331b536653d7f09a6c4fdd6efd7ba547dae058d29c9d05cde674  formal/ui-motion/WaveLateState.cfg
```
