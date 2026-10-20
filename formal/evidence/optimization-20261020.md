# Runtime and persistence optimization verification, 2026-10-20

Base: `1e8cc10`. Results describe local, uncommitted source changes.

## Changes

| Area | Result |
| --- | --- |
| Profile persistence | Failed background and immediate writes retain the latest pending snapshot. Generation ownership prevents old completions from clearing newer work or publishing stale status. Retry reads current application state. A revisioned, main-window-only status event and bootstrap snapshot drive a persistent error notification with a retry action. |
| Selection state | A window-targeted event replaces normal 100 ms polling. Listener registration precedes the initial snapshot; versions reject stale deliveries. Registration failure uses a slow, single-flight fallback. Cancellation and disposal preserve existing ownership. |
| Streaming Markdown | Appended updates share one animation frame and a memoized parser. First content, final content, clearing and non-prefix replacements display immediately. Old frames cannot revive retired content. The selection assistant and both assistant response surfaces use the optimization; ordinary ASR subtitles retain their existing path. |
| History statistics | A retained SQLite connection uses `PRAGMA data_version` to reuse unchanged statistics. Aggregate and percentile inputs share a read transaction. Internal and external writes invalidate the cache; changes during a scan prevent stale cache publication. Existing percentiles and null handling remain exact. |

## Test-first evidence and review

Dedicated test agents owned all new or modified test code. Main review and
independent local reviewers accepted each frozen slice before implementation.
No source was sent to an external evaluation service.

- History: five compiled failures demonstrated repeated full scans after
  correctness checks, including insertion, deletion, both retention paths,
  failed mutations and an external commit interleaved with a real scan.
- Profile persistence: a real filesystem write failure reproduced the lost
  pending snapshot. Additional status tests used declared interfaces before
  state publication was implemented. Notification tests began against an
  explicitly empty hook scaffold.
- Selection and Markdown: existing tests passed first; new failures demonstrated
  polling, missing event handling and repeated parser work.
- Review corrected Markdown fixtures to use cumulative appended content and
  separately test immediate non-prefix replacement.
- Review corrected notification coverage for Sonner's default action dismissal,
  stable toast identity and stale retry rejection after a newer successful save.
  A later React 19 fixture-only `flushSync` correction preserved these assertions.
- Final review found a scrolling regression: a delayed child Markdown commit did
  not rerun the parent's scroll effect. Two new tests reproduced it. A layout
  commit callback now follows the new bottom while retaining the user's
  scroll-up guard. All 132 targeted frontend tests passed after the repair;
  the final independent review found no remaining actionable issue.

## Measurements

These are controlled workloads, not end-to-end latency guarantees.

| Workload | Before | After |
| --- | ---: | ---: |
| Five simulated idle seconds, including initial selection snapshot | 51 IPC requests | 1 IPC request |
| Initial Markdown content plus 20 appended updates before one frame | 21 parser renders | 2 parser renders |
| Synthetic history, 25,000 rows, 30 repeated calls: median | 48,440 us | 20 us |
| Same history workload: p95 | 49,596 us | 25 us |
| Same history workload: repeated full scans | 30 | 0 |
| Same history workload: cold computation | 54,612 us | 55,432 us |

History measurements use the debug Rust test build and real SQLite/statistics
code. The benchmark is explicitly invoked and has no timing pass threshold.
Markdown tests wrap the real parser; scroll geometry is simulated in jsdom.

## Final checks

- `pnpm check`: 655 functional tests plus one fluidity test passed; whitespace,
  ESLint, TypeScript, production Vite build and bundle budgets passed.
- Rust all-target regression: 782 passed, 13 ignored, zero failures. The normal
  documented command filters one existing timing-sensitive hotword benchmark.
  The ignored entries comprise the existing 12 entries plus the new statistics
  benchmark, which was run separately above.
- `cargo clippy --all-targets --locked --offline -j 2 -- -D warnings`: passed.
- Rust format checks: passed.
- Formal inventory/provenance: 128 commands, 19 event families, 16 lifecycle
  contracts and 11 protocols passed.
- Formal verifier regressions: 11 passed. Async trace regressions: eight passed.

The abstract TLA+/Lean models were unchanged and were not rerun in full.
The registry records new commands/events and implementation evidence. Failed
profile writes, status delivery and cache validity are covered by these
implementation tests; the existing ProfilePersistence model assumes successful
writes. This is not a source-level refinement proof.

No release, installation, microphone/GPU integration run or live WebView UI
operation was performed. The cache assumes the database path stays fixed during
the process; replacing the open database file is outside its contract.

Full commands, hashes, RED/GREEN logs and benchmark output are retained in the
ignored local directory `tmp/optimization-20261020/`, including `evidence.json`,
`frontend-final.log`, `rust-full.log`, `rust-clippy-final.log`,
`history-benchmark-{baseline,after}.log` and `frontend-scroll-{red,green}.log`.
