# Entrypoint provenance verification

The application formal registry runs this model and the neighboring
`EntrypointObservation` routing model through `scripts/check_formal.py`.
They are verification support contracts; they add no application commands,
events, lifecycle contracts or protocols. No additional runner is required.

The original model/source correspondence received independent review and SANY
preflight. The complete formal run then exposed an unreachable restore action.
The correction below passed SANY and the eleven provenance configurations with
actual TLC. The completed application formal check includes this model; see the
[current verification record](../../../evidence/agent-cli-180.md). Registry
validation alone is not a completed model check.

## Checked boundary

`entrypoint_probe.py` admits only the complete 21-pin source/mirror identity,
exact original or generated variant, exact observer and owned path/backup
conditions. A prepared owner's `observed_head` must be exactly 40 lowercase
hexadecimal characters. Apply records that actual owner value in its receipt.
Both `verify_launch` and `restore` recheck current identity and receipt/owner
equality before admission or writing. Verification also checks the owned app
artifact and trusted fixture; it returns metadata without launching a process.

The model represents complete stage-specific identity as `identityValid` and
the additional app/fixture checks as `artifactValid`. `last` snapshots invocation
inputs and the prior helper write count. `appliedHead` and `recordedHead` preserve
receipt creation history, while mutable `receiptHead` permits later metadata edits.

| Model seam | Helper correspondence |
| --- | --- |
| `HeadValid`, `Apply` | `_mirror`, including HEAD type/regex and all 21 pins; `apply_overlay` receipt creation before overlay writes. |
| `DriftOwner`, `DriftReceipt`, `DriftIdentity` | External metadata or identity changes between invocations. |
| `Invoke`, `UseGuard` | `_applied` rereads the owner, receipt, backup and exact overlay, then compares current owner and receipt HEADs. |
| `Verify`, `launchAdmitted` | `verify_launch` performs read-only app/fixture admission; downstream launch must require that admission. |
| `Restore`, `writes`, `overlay` | `restore` validates before writing original backup bytes. The Boolean abstracts exact restoration checked by filesystem tests. |

Source identities for this correspondence:

- `../entrypoint_probe.py`: SHA256 `dccc31d7d4f7e41f51e44790c4a6f1f6b26dfa401223184bab32618805a70675`.
- `../reviewed.json`: SHA256 `7c7b739fd6002047bf7aafc427a15d5295d7b1ff8e469d704dc025d83c6869a4`.
- `../test_entrypoint_probe.py`: SHA256 `4d53cf0c68a6d11ccc05ec7993eed17d95b35a5fc864256586cfef46fae802ee`.
- `EntrypointProvenance.tla`: SHA256 `80e756d66e557a706f7e5ca5256308b104b80c2e1b934532419bbc774d393cb8`.

The earlier 27-test receipts retain test SHA256
`d2c79097cd7c6488009c40122b857cb8a7cf42e7a7559076e4e57748105a6a27`.
The current 32-test suite preserves all 27 original tests and adds ownership,
path and artifact refusal cases. The helper and formal models are unchanged.

## Restore reachability correction

The original assignment `launchAdmitted' = admitted /\ operation = "verify"`
parsed as `(launchAdmitted' = admitted) /\ (operation = "verify")`, as confirmed
by the official SANY semantic graph. Its extra action guard disabled every
`Restore` transition. The neighboring `IF` expressions parsed as intended.
Parenthesizing the Boolean right-hand side restores both invocations without
changing admission guards, invariants, configurations or helper code.

The original positive check completed with 308 generated and 164 distinct states,
but its restore safety obligation was vacuous: the drift-restore negative also
completed without the required violation. The corrected positive check completed
with 452 generated and 236 distinct states at depth 6. Its state dump contains
72 restore outcomes: 8 admitted and 64 refused. A successful restore has matching
valid owner/receipt HEADs, `writes = 2`, `overlay = FALSE` and no launch admission;
owner-only and receipt-only drift refusals retain `writes = 1` and the overlay.
The shared `Invoke` action coverage increases from 72 to 144 taken transitions;
the state dump distinguishes restore from verify because TLC merges their alias
locations in action coverage.

All eleven provenance configurations produced their expected results using TLC
2.19, JAR SHA256 `936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`,
and Temurin 1.8.0_462 with one worker. In particular, removing receipt/owner
equality now produces exit 12 and `RestorationGate` after apply, owner drift and
an admitted restore (65 generated, 57 distinct states). Correct-guard owner-drift
and receipt-drift restore refusal witnesses also reach their named targets.

## Bounds and assumptions

There are two valid HEAD partitions and one invalid partition, one apply, at most
one owner edit, one receipt edit and one identity loss in any order, followed by
one verify **or** restore. Initial identity and artifact validity each range over
Boolean. At most five non-stuttering transitions precede a terminal self-loop.
Every helper call is exclusive and synchronous with successful filesystem
operations; environment edits occur between calls. There is no fairness or
liveness claim, state constraint, view, symmetry or disabled deadlock check.

The finite claim excludes edits within a call, concurrent writers, partial I/O,
crashes, repeated verify/restore sequences and native application execution.
Multiple apply writes are not modeled as a crash-atomic filesystem transaction.
Metadata equality does not authenticate Git objects, prove the live Git HEAD,
or detect coordinated matching owner/receipt edits. Concrete byte/path checks
remain covered by the current 32 filesystem/protocol tests, not by enumerating them here.

## Registered matrix

The shipping positive configurations use one singular `INVARIANT` declaration
per property so the generic binding checker recognizes every active obligation.
Their ordered property lists, constants and `Spec` are equivalent to the reviewed
configurations. The existing routing module and its three original configs are
unchanged. Positive log stems are unique across the full application registry.

| Configuration | Expected result |
| --- | --- |
| `../EntrypointObservationPositive.cfg` | Exhaustive pass: five routing/observation invariants. |
| `../initial-host-inverse.cfg` | `ExpectedObservation` violation. |
| `../final-host-inverse.cfg` | `ExpectedObservation` violation. |
| `EntrypointProvenancePositive.cfg` | Exhaustive pass: all nine provenance invariants. |
| `negative-bad-head.cfg` | `ValidHeadAdmission` violation. |
| `negative-fixed-head.cfg` | `RecordedOwnerProvenance` violation. |
| `negative-skip-drift-launch.cfg` | `LaunchAdmissionGate` violation. |
| `negative-skip-drift-restore.cfg` | `RestorationGate` violation. |
| `witness-valid-new-head.cfg` | `NoValidNewHeadApply` violation. |
| `witness-bad-head-refusal.cfg` | `NoBadHeadRefusal` violation. |
| `witness-owner-drift-launch-refusal.cfg` | `NoOwnerDriftLaunchRefusal` violation. |
| `witness-owner-drift-restore-refusal.cfg` | `NoOwnerDriftRestoreRefusal` violation. |
| `witness-receipt-drift-launch-refusal.cfg` | `NoReceiptDriftLaunchRefusal` violation. |
| `witness-receipt-drift-restore-refusal.cfg` | `NoReceiptDriftRestoreRefusal` violation. |

Positive cases require exit 0 and exhaustive completion. Every remaining case
requires exit 12 and its exact named invariant violation. The six witness cases
establish reachability with correct guards and are not safety-mutant kills.
Drift witnesses keep both unequal HEADs, other identity gates and verify artifacts
valid, so refusal specifically exercises the receipt/current-owner comparison.

Run the existing application matrix with the pinned TLC JAR and Lean executable:

```text
python scripts/check_formal.py --tlc-jar <pinned-jar> --lean <lean-executable> --logs <owned-results-directory>
```

Run the helper suite without compiling or launching a native artifact:

```text
uv run --no-sync python -m unittest discover -s formal/agent-api-measurements/entrypoint -p "test_*.py" -v
```

The generic matrix records model/config hashes and distinct-state counts, checks
all registry-bound positive properties and requires named negative outcomes.
Actual TLC completion, action coverage, source synchronization and independent
integration review must be recorded before formal closure is claimed.
