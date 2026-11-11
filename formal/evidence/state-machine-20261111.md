# Implicit state machine repairs, 2026-11-11

Seven ownership/transition defects were reproduced and repaired from baseline
`5ec93e0a252bd36ee9acc9fc842f355c8a2798b2`. Tests were written in isolated
test worktrees before behavior changes. An independent local reviewer examined
the destructive filesystem and credential boundaries, and the cancellation
contract. Repository material was not sent to an external review service.

## Behavior and correspondence

| Group | Production boundary | Regression evidence | Model |
| --- | --- | --- | --- |
| Shared history audio | `history_service::cleanup_audio_if_unreferenced_with_connection` holds a SQLite immediate transaction across reference/lease inspection and unlink | Actual SQLite authorizer barriers, competing reprocess write, early unlock, denied queries, two independent leases and Windows unlink failure in `history_audio_gc_tests.rs` | HistoryAudioGcOwnership |
| Assistant retry | `SubtitleOverlay` retry success, error and finally require both recording session and a dedicated retry generation | Deferred real page operations across session changes, overlay close/reopen and independent conversation turns in `SubtitleOverlayRetryOwnership.test.tsx` | AssistantRetryOwnership |
| Download ownership | `useModelStatus`, IPC, cancel command and `download_service` share a request ID; cancellation waits for owned worker completion | Deferred events/commands, actual AppState cancellation waiters and no-network owned Python child processes in `download_worker_tests.rs` | DownloadTaskEventOwnership |
| History mutation reload | Post-polish, ASR and delete continuations invoke the current filter loader | A mutation starts under filter A and settles after switching to B in `HistoryMutationFilterOwnership.test.tsx` | HistoryMutationFilterOwnership |
| Vision credential editor | Dirty drafts belong to the captured provider; switching flushes the draft and real storage promises serialize | Actual SettingsPage inputs, deferred reads/writes, failure retention and provider switching in `SettingsPageProviderDiscovery.test.tsx` | VisionCredentialOwnership |
| Selection publication | A captured listener epoch must still own a mutex spanning cache, payload and actual window publication | Actual production owner callbacks, ABA rejection and concurrent publication/disable ordering in `selection_publication_tests.rs`, plus Windows call-path review | SelectionPublicationOwnership |
| Catalog loading | Context/auth/mode retirement clears loading; stale finally cannot clear a newer request | Real hook and component tests in `useModelCatalogLoadingOwnership.test.tsx`, `ScreenVisionModelPickerLoading.test.tsx`, `SelectionAssistantCatalogLoading.test.tsx` | ModelCatalogLoadingOwnership |

## Test-first evidence

The targeted RED observations were assertion failures, not missing APIs or
compilation failures. Additional controls passed in each slice.

| Slice | RED | Initial focused GREEN |
| --- | --- | --- |
| Audio GC | Two regressions failed; three controls passed | History tests: 21 passed, one ignored |
| Assistant retry | Six regressions failed; two controls passed | Retry and existing subtitle tests: 69 passed |
| History mutation reload | Four regressions failed | Mutation, history and replay tests: 25 passed |
| Vision credentials | Nine regressions failed; one control passed | Complete provider discovery file: 69 passed |
| Catalog loading | Ten regressions failed; twelve controls passed | Four selected files: 24 passed |
| Selection owner | Declaration-only scaffold: four failures, one control passed | Selection tests: 53 passed |
| Download UI/API | Eighteen regressions failed; 51 controls passed | Two focused files: 69 passed |
| Download cancel acknowledgement | Four regressions failed; six controls passed | Included in the combined Rust validation |
| Download actual worker | Seven regressions failed; one source-binding control passed | Included in the combined Rust validation |
| Selection hook failure/exit binding | One source correspondence assertion failed on two direct flag writes | Included in the combined Rust validation |
| Retry and conversation independence | Four DOM regressions failed; eight existing controls passed | Retry and existing subtitle tests: 73 passed |
| Vision provider rollback | One regression failed: B's key appeared under restored A | Complete provider discovery file: 70 passed |

Selection is explicitly scaffold-based TDD at the extracted production ownership
seam. It is not an automated RED execution of the original Windows hook/window
race. Its legacy model counterexample and actual call-path review establish the
correspondence separately. Native hook failure/exit retirement is checked by a
source correspondence regression, not simulated native window execution.

The download worker was extracted without changing behavior, then tested through
that same core called by the public Tauri wrapper. Tests inject preparation and
event collection, but execute real child pipes, cancellation, process termination
and exit waiting. They do not invoke the network downloader or load a model.
Cancellation while waiting for the lifecycle lock can finish without starting
preparation. Once engine preparation starts, cancellation retains its slot and
waits for that preparation to settle before discarding the command. In particular,
dropping an extraction future does not stop `spawn_blocking` work, so the worker
does not drop that future to acknowledge cancellation early. Script `stage:error`
and final `success:false` lines produce one terminal event after child exit.

## Finite verification and negative controls

| Model | Positive distinct states | Safety negatives | Additional witness |
| --- | ---: | --- | --- |
| HistoryAudioGcOwnership | 16,924 | SplitRead, EarlyUnlock, IgnoreLease | Boundary |
| AssistantRetryOwnership | 51 | Legacy, Generation, Finally | — |
| DownloadTaskEventOwnership | 2,783 | Event, Cancel, Ack | — |
| HistoryMutationFilterOwnership | 29 | Legacy, Delete | — |
| VisionCredentialOwnership | 33,362 | Provider, Read, SavedRead, Serial, Switch | — |
| SelectionPublicationOwnership | 700 | Legacy, EarlyUnlock, Reenable | Boundary |
| ModelCatalogLoadingOwnership | 29 | Legacy, Finally, Publication | — |

The seven positive models cover 53,878 distinct states. All 22 deliberately wrong
safety configurations must return TLC exit 12 with the configured invariant
violation. The two Boundary configurations intentionally violate an unreachable-
boundary assertion to demonstrate scenario reachability; they are not safety
proofs or safety negatives. Download progress is conditional on the model's
explicit weak fairness and worker settlement assumptions.

`verification.json` binds the configured assertions, exercised transitions,
source/test anchors and assumptions into the normal formal gate. Existing Lean
StateContracts prove parameter-independent stale-owner, version and referenced/
leased-audio decision lemmas; these repairs reuse those lemmas. There is no new
claim of a Lean proof of Rust/TypeScript execution or automatic source refinement.

## Combined validation

- Frontend: 82 files, 890 tests passed, plus the performance regression; ESLint,
  TypeScript, production build and bundle budget passed.
- Rust: 902 tests passed, 14 existing ignored tests/fixtures, zero failures;
  formatting and strict all-target clippy passed. Tests ran with one build job
  and one test thread.
- Python: 12 verifier, eight trace-provenance, nine output-wiring and 22 downloader
  regressions passed (51 total), without network or model downloads.
- Formal gate: all 75 positive configurations across 56 model families passed;
  52 unchanged configurations reused successful logs only after source/config
  SHA-256 checks, and 23 were checked again. All 131 configured negative controls
  and reachability witnesses produced their required exit-12 invariant violation.
  All 67 integrated AppWorkflow actions were reached across ten workloads.
- Lean StateContracts passed with Lean 4.34.1. The local TLC 1.7.4 run used the
  repository-pinned JAR SHA-256 and Temurin Java 8u462, with a 1 GiB heap cap;
  the existing CI continues to use Java 21.

Per-run logs and the formal JSON summary are retained under the ignored
`tmp/state-machine-audit/` directory. Source-controlled models, configurations,
regressions and registry bindings allow the normal CI gate to reproduce the checks.

```sh
pnpm lint
pnpm exec vitest run --exclude 'src/perf/**' --maxWorkers=1
pnpm exec vitest run src/perf/appFluidity.perf.test.tsx --maxWorkers=1
pnpm build
pnpm check:bundle
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
python scripts/test_check_formal.py
python scripts/test_async_ownership_traces.py
python scripts/test_async_output_wiring.py
python -m unittest discover -s src-tauri/resources -p test_download_models_regression.py
python scripts/check_formal.py --tlc-jar /path/to/tla2tools.jar \
  --lean /path/to/lean --logs /path/to/formal-results
```

## Boundaries

Models enumerate finite request/session/provider/lease/epoch identities specified
in their source. Epoch counters must not wrap. SQLite locking, filesystem calls,
Tokio scheduling, Tauri event delivery and Windows primitives remain trusted
implementation dependencies; tests exercise selected actual failure partitions.

Audio creation uses unique fresh paths before their first history reference;
reprocessing existing paths acquires leases. Arbitrary external SQL insertion of
a missing pathname, external deletion, crash durability and replacement of an
open database file are outside the GC contract. Database inspection errors retain
the audio conservatively; failed unlink leaves a retryable orphan.

The selection mutex orders listener retirement against publication. It does not
lock OS focus, clipboard contents or foreign applications. No real hook/window
interaction or GPU/ASR/LLM quality claim follows from these tests. A built bundle
does not establish that the running installed application was updated.
