# Desktop request admission contract

This model was introduced against the unfixed CLI SHA256
`1517896105d1f5f0b0fe7acb0aa430b4f597c0be7ac7268bf380025e74daf504`.
`Legacy` preserves that unfixed admission behavior. The current reviewed CLI is
`db2655d9beecc41de1b20338cb44c6af64dae72701d833b5459639d0cc55d9b7`;
`Correct` maps to its `validate_desktop_request` and two app-only caller guards.
Independent source correspondence review accepted that mapping. The baseline
identity and negative controls remain historical evidence. A completed finite
model run is still required and is not claimed by this source update.

The installed-command update changes only `app_path` from the earlier admission
review. The modeled admission helper and caller guards are unchanged. See the
[source correspondence](README.md#desktop-admission-source-correspondence) and
[command-entry contract](../cli-command-entry/README.md).

An app request containing an unpaired surrogate, or whose exact serialized
physical pipe record exceeds 65536 bytes, must fail before acquiring a broker or
calling its request method. One-shot output is one JSON failure with exit 2.
Session rejection preserves the caller's integer ID, consumes no internal request
ID, retains a live client, and continues to the next input. Accepted scalar
Unicode and exactly 65536 physical bytes remain accepted. Caller IDs are opaque
and do not contribute to the broker request budget. Independent engine Unicode
serialization remains governed by `UnicodeSerialization.tla` and its unchanged
five executable tests.

## Production correspondence

| Source boundary | Model | Executable control |
| --- | --- | --- |
| `main`: assembles one-shot fields before `request_client`; catches `ValueError` as JSON/exit 2 | `Validate`, `OneShotError` | `test_oneshot_invalid_desktop_input_is_json_exit_2_before_acquisition`, one-shot boundary test |
| `run_session`: field validation/error reply inside the per-record recovery region, before lazy acquisition | `Validate`, `LazyFirstRecord`, `CallerIdentity`, `SessionContinuation` | first-invalid and live-client recovery tests |
| `run_session`: one `ExitStack` client retained across operations and closed at exit/EOF | `Acquire`, `Reply`, `Close`, `LiveClientPreserved`, `ClientReused`, `OwnedCleanup` | per-input acquisition/write/close snapshots, following valid status/stats, explicit exit |
| `EngineClient.request`: increments internal `sequence`, then constructs `action`/`request_id` | `pending`, `sequence`, `Send`, `InternalSequence`, `DecisionMatches` | warmups 0/8/9; same payload at internal IDs 9 and 10; negative 618-character caller ID |
| `EngineClient._send`: `json.dumps(..., ensure_ascii=True)` plus logical LF | `Body`, `Digits`, `newline`, `WireBytes`, `NoInvalidEffects` | real production sender and `io.TextIOWrapper` over recording `BytesIO`; exact observed physical bytes |
| `EngineClient.__enter__`: `Popen(text=True, encoding="utf-8", errors="replace", bufsize=1)` | newline representatives 1 and 2 | fixture uses matching encoding/error/default-newline, write-through and line-buffering semantics; Windows writes CRLF |
| Rust `transport.rs`: `REQUEST_MAX = 64 * 1024`, bounded read includes terminating LF and any preceding CR | `Cap = 65536`; complete physical size | boundary cases 65536 and 65537, including actual newline translation |
| Rust `run_client`: `serde_json::from_slice::<Value>` rejects invalid scalar-string escapes; invalid/null-ID response cannot match Python pending ID | `high`/`low` invalid partitions; rejected before transport | high/low surrogate context, embedded language surrogate, surrogate hot word; no null-ID correlation workaround |
| `request_client`: app broker versus independent Python engine | target partition, `EngineUnchanged` | existing unchanged `test_cli_unicode_serialization.py` regression; new desktop scalar control |

The missing baseline guard was the intended source/model mismatch. The reviewed
helper now checks strict scalar UTF-8 encodability and ASCII-escaped physical
record size including `len(os.linesep)`. `main` calls it with internal ID 1 before
`request_client`; `run_session` calls it with the next internal ID before lazy
acquisition and request dispatch, inside per-record recovery. Local exit and the
independent engine skip it. `EngineClient`, including its sender, is AST-identical
to the baseline. The [companion correspondence](README.md#desktop-admission-source-correspondence)
records how existing models retain their narrower contracts.

## Finite domain and controls

There is one one-shot record, or two session records. Sessions begin fresh, or
with a retained client after eight/nine successful requests. The first record
represents scalar Unicode, high/low unpaired surrogates, ASCII-escaped overflow,
exact one-digit/two-digit-ID capacity and one-byte overflow. The second record is
a small valid status operation. Caller-ID widths are 1 or 618 bytes; the latter
matches the signed decimal `-(2 ** 2048 + 7)` used by executable tests. Unicode
text and JSON punctuation are not synthesized by TLA+: the body-length partition
includes their exact escaped byte contribution, independently measured by tests.
This is not a proof of every JSON encoder or serde decoder.

The positive config activates every declared safety invariant plus
`EventuallyFinished`. Each synchronous local transition and eventual bounded
cleanup is weakly fair. Broker responses, timeouts and native cleanup internals
remain the existing `EngineClient`/`Session` model boundaries. Validation has no
asynchronous gap or external effect; acquisition and send stay separate steps.
Terminal stuttering is intentional. No state constraints, symmetry, sampling or
deadlock suppression is used.

Fifteen negative configs retain the intended property while changing the named
mechanism: legacy admission, Unicode guard, byte cap, newline count, next-ID width,
caller-ID width, lazy acquisition, retained live owner, rejected-ID consumption,
reply identity, engine isolation, one-shot exit code, session continuation,
cleanup and local progress. `ClientReused` shares the live-owner control;
`DecisionMatches` checks both rejection and acceptance and has the caller-width
control. Six separate witness configs reach fresh/live recovery, Windows exact
capacity at ID 9, overflow at ID 10, independent-engine surrogate admission and
escaped overflow rejection. Witness failures are reachability evidence, not
safety-control kills. The model/config matrix is registered with the existing
generic checker; no runner or checker implementation is added.

## Test surface and evidence limits

The new test module loads the production CLI by its own repository-relative
absolute file path, independent of wrapper `PYTHONPATH`. It calls public `main`
and therefore the actual `run_session`. Only `request_client` acquisition is
replaced by a test-owned context holding a real `EngineClient`. Its production
request, sender and receiver methods execute against a recording physical text
pipe. The peer replies successfully even to inadmissible input; lack of preflight
therefore fails assertions deterministically without waiting for a broker timeout.
Input-iterator snapshots prove effects have not happened before the next record.

Synthetic files and configuration paths belong to a temporary directory. No
subprocess, desktop, microphone, model, network or Rust harness is used. This is
CLI caller/serialization evidence; actual native dispatch is a separate surface.
SANY only validates module syntax/semantics. Completed finite TLC configurations,
source coverage and mutation require their own receipts and are not claimed by
this correspondence update. Reviewed source correspondence is distinct from a
completed TLC run or evidence from native dispatch.
