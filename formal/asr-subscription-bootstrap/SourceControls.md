# Fixed-source negative controls

Reviewed hook SHA256:
`a84e86e921f9c3ba5d8432e19c771c1956963af6f5340044de28622dac57b9fe`.
Current seven-test suite:
`src/hooks/__tests__/useAsrEngineSettingsSubscriptionBootstrap.test.tsx`.
Its actual file-byte SHA256 is
`2d3c3d1c603d25450d26968875af83be0c0aa9b6a96e8cfebe8cf4d09ab05ab0`;
its normalized-LF SHA256 is
`b22ae8331298ae4b8c012d8514dded4675162da262066ae6733e1ba82a98437f`.

Historical seven-test writer SHA256:
`789eb4667677c0e9d493b00e5be3b5e54c2d8dd4b0177560714aee5061ce4493`.
That historical identity is not equivalent to the current suite through CRLF
normalization alone. Preserve historical receipts and their captured source,
test and configuration pins; do not relabel them with the current test hash.
Report actual file-byte hashes and normalized-text comparisons separately.
Production owner executes these controls in a disposable implementation checkout,
after establishing GREEN for the exact seven-test suite chosen for that run.
Never mutate the user's primary checkout, the accepted main assembly, or the
Test Writer's baseline. Do not copy production implementations into the old test
lane, whose original RED source is deliberately frozen.

For every control, start again from the reviewed fixed source. Change only the
single mechanism below. Run the hash-bound production-hook test file through a
parameterized Vitest config whose `root` and `@` alias resolve the disposable
target checkout, using the already installed dependencies. Keep one worker and
`NODE_OPTIONS=--max-old-space-size=2048`. Record the config/test/source hashes,
exact command, return code and intended assertion; compilation/import failures
are not successful negative controls. Revert the owned disposable edit and
verify its source hash returns to the fixed baseline before the next control.
Bind each new receipt to the actual test bytes executed; this recipe does not rewrite an older
receipt or establish a new execution result.

| Source control | Exact production change in disposable source | Required public-hook failure |
| --- | --- | --- |
| Legacy eager hydration | Remove both `hydrateInitialConfig();` calls from subscription success/failure. Add one `hydrateInitialConfig();` immediately after that helper's declaration, before `getAlibabaAsrConfig()` and `listen()`. | Both missed-invalidation schedules retain Qwen instead of saved R2T2. |
| Superseded bootstrap version | In `hydrateInitialConfig`, change `if (current()) void readAsrConfig(...)` to `if (!disposed) void readAsrConfig(...)`; retain every other guard. | The two-write publication-history test sees the second save published on ACK, then a regression to the first save when its older pre-ACK event read resolves. |
| Missing registration fallback | Remove only `hydrateInitialConfig();` inside the listener rejection handler; retain its diagnostic and successful ACK path. | Registration failure leaves saved R2T2 unhydrated/readiness false. |
| Missing late release | Change the successful listener promise branch `if (disposed) stop();` to `if (disposed) return;`; retain the acknowledged listener cleanup path. | Unmount-before-ACK requires the acquired listener's exact release count 1, receives 0. |
| Silent subscription failure | Remove only `console.error("Failed to watch speech engine settings", error);`; retain fallback hydration. | Registration-failure test requires a diagnostic carrying the original error. |

The version-only control is distinct from disposal. Its second native write and
emission are deliberately separate, matching the real controller's awaited
engine shutdown between persistence and emission. It accepts implementations
that never start the obsolete ACK read: no fixed read count is required.

Example execution with the task-local external config supplied by the writer:

```powershell
$env:LW_SUBSCRIPTION_TEST_ROOT = $disposableImplementationCheckout
$env:NODE_OPTIONS = '--max-old-space-size=2048'
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $env:LW_SUBSCRIPTION_TEST_ROOT 'src/hooks/useAsrEngineSettings.ts')
node $installedVitestEntry run --config $subscriptionTargetConfig --reporter=verbose
```

`subscription-target.vitest.config.mjs` also prints the actual hook path and
SHA256 before collection. These are execution recipes, not completed source
mutation receipts. The bounded TLA+ negative configurations independently test
their named properties; TLC results and source-control outcomes must be recorded
separately. No claim here closes whole-module coverage or source mutation gates.
