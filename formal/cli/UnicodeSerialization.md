# Python CLI JSON string serialization

The promise is preservation of decoded Python JSON strings across the owned
client/backend byte protocol, including strings which the public JSONL parser
already admits. A backend may reject strings it cannot use. Silent replacement
before that backend receives the request violates the promise.

The [CPython 3.11 JSON documentation](https://docs.python.org/3.11/library/json.html#character-encodings)
describes its default acceptance of unpaired surrogate code points and the
interoperability limits. Its [`ensure_ascii` documentation](https://docs.python.org/3.11/library/json.html#json.dump)
specifies ASCII escaping of incoming non-ASCII characters. The tests supply valid
ASCII JSON bytes containing escapes such as `\ud800`; they never submit invalid
raw UTF-8 or surrogate command-line arguments.

The reviewed regression baseline is `scripts/light_whisper_cli.py` SHA-256
`e78cad5614d73341c93731d9b38998ee35a1019dbcb3147c13ea5a6159a74d77`, in helper
snapshot `f8d4a2378076523a67aa7d45ef4457cdd5796768`. At this baseline, `_send`
uses `ensure_ascii=False`, and `EngineClient.__enter__` gives its child stdin
UTF-8 text encoding with `errors="replace"`. Literal unpaired surrogates become
question marks before the backend decodes the JSON. The legacy negative uses
`EnsureASCII = FALSE`. The desired positive uses `TRUE`. The mutation changing
this argument from false to true is a potential fix, not the legacy fault.

| Obligation / data owner | Production binding | Executable evidence | Model binding |
| --- | --- | --- | --- |
| Preserve nested strings and string keys through a real child pipe | `EngineClient.request -> _send`; stdin encoding in `EngineClient.__enter__` | `test_engine_client_preserves_scalar_unicode_values_and_keys`; `test_engine_client_preserves_unpaired_and_mixed_json_values_and_keys` | `source`, `location`; `Encode`, `Pipe`, `Decode`; `DecodedStringsPreserved` |
| Preserve accepted context, language, ordered empty/repeated hot words; valid audio file before dispatch | `main` transcribe fields; `run_session -> json.loads -> session_request -> client.request` | `test_source_cli_transcribe_preserves_scalar_context_and_hot_words`; `test_source_cli_session_preserves_escaped_strings_and_external_ids` | Same per-string transform; list/object traversal and admission covered by executable tests |
| Preserve backend opaque fields, nested string keys and external integer identity | `_receive -> json.loads`; `run_session` response merge; `session_reply`; final `main` JSON output | `test_source_cli_preserves_escaped_backend_response_fields`; session test uses repeated negative 2049-bit caller IDs separate from wire IDs | Response/output preservation and correlation are executable controls; no new identity theorem is claimed |

`test_cli_unicode_serialization.py` reuses the maintained `BackendCase` resource
owner. Its maintained synthetic backend reads JSON through a real Python stdin
pipe and ASCII-escapes its replies and request log, so surrogate-bearing evidence
does not fail in fixture encoding. Source CLI tests copy the current production
script byte for byte, place the synthetic backend at that copy's normal engine
path, then execute that script as a child process. One-shot transcription uses
scalar Unicode argv; session transcription uses ASCII-escaped JSONL bytes. A
small owned WAV is syntactically valid; the synthetic backend never decodes it.
Tests compare decoded echo and decoded request log with the original values,
check clean return/exit logging, and reap/close owned child processes and workers.

The finite model covers five representative strings: an empty string, a scalar
ASCII/BMP/supplementary/question-mark mixture, an unpaired high surrogate, an
unpaired low surrogate, and a scalar/surrogate mixture with high and low separated
by a scalar. Each has key/value locations. Tokens represent semantic Python
characters; the supplementary scalar's escaped token abbreviates two JSON
surrogate escapes which decode back to that scalar. Lexical JSON scaffolding,
mandatory quote/backslash escaping, object recursion and arbitrary string lengths
are abstracted, with their selected concrete cases retained in executable tests.
This is not a theorem that arbitrary adjacent surrogate code units remain separate
through CPython's combining of escaped high/low pairs.

`Encode` distinguishes ASCII escape tokens from literal tokens. `Pipe` preserves
ASCII escapes and scalar literals, but substitutes a question mark for each
literal unpaired surrogate as the actual UTF-8 replacement encoder does.
`Decode` recovers each token's semantic character. The oracle is independent
sequence equality against the original input, rather than a second codec.
Initialization, encoding, transport and decoding are separate phases. This pure
finite value transform claims no asynchronous progress or scheduling property;
existing transport/session models retain those ownership obligations. Terminal
states explicitly stutter. No constraints, symmetry, VIEW or fairness are used.

The positive config activates `TypeOK` and `DecodedStringsPreserved`. The lossy
negative must violate `DecodedStringsPreserved`, with scalar-only inputs retained
as passing controls. The four witness configs must violate `WitnessNotReached`
after decoding each input class without violating preservation. Witness failures
are reachability evidence, not safety-control kills. SANY preflight checks module
syntax/semantics; it does not execute configs or establish these TLC verdicts.

The single registry owner should add `cli/UnicodeSerialization` to the maintained
generic `scripts/check_formal.py` registration. Its positive run is
`cli/UnicodeSerialization.cfg`; its actions are `Encode`, `Pipe`, `Decode`;
its contract properties are `DecodedStringsPreserved` and those actions. Register
`cli/UnicodeSerializationLossyNegative.cfg` as a negative expecting
`DecodedStringsPreserved`, plus `Scalar`, `High`, `Low`, `Mixed` witness configs
expecting `WitnessNotReached`. Bind this document, the test module and the source
anchors above in that contract. Do not add a separate formal runner.

No native Rust/serde JSON acceptance, ASR inference, microphone, GPU, provider,
installed application, user state, unbounded frame or exhaustive Unicode proof
is claimed. Main owns accepted source/model correspondence, production changes,
assembled GREEN, actual TLC runs, measured coverage and source mutation receipts.

## Current target-specific admission

At reviewed CLI SHA256
`d1a9f86c0bcc3419ad7f1b1872a24a7e44d11bb552bb0860eda34f67884ac09b`,
`EngineClient` is AST-identical to the preceding reviewed source. Direct-client
and independent-engine requests retain escaped unpaired-surrogate preservation.
App callers instead reject invalid scalar strings before acquisition/send and
admit scalar strings only within the exact physical wire cap. That rejection is
visible input failure, not replacement inside `Pipe`. The unchanged finite
`Encode`/`Pipe`/`Decode` theorem applies to strings that reach the client sender;
it does not prove which app inputs are admitted. [DesktopAdmission](DesktopAdmission.md)
checks the separate Unicode/size guards. Its engine-isolation control and the
unchanged five serialization tests retain the independent-engine promise. See
[companion correspondence](README.md#desktop-admission-source-correspondence).
No model/configuration or historical receipt is rebound by this prose update.
