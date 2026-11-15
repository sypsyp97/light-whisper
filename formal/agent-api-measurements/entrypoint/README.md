# Test-only native entrypoint observation

The helper is confined to this directory. Existing native20 bodies, driver, models,
cohort, bridge and production source remain unchanged.

Contract: accept the original source or exactly one of the two reviewed generated
`lib.rs` variants in `reviewed.json`; reject unknown, combined or prefix edits.
Pin all ten cohort source files, all existing cohort tests, native driver/bridge,
path resolver, main entrypoint and overlay prerequisites before any source write
or process launch. Require an owned prepared mirror outside shipping source,
exclusive files, a fresh evidence directory and fresh external target.

`reviewed.json` establishes review identity through its 21 exact content pins
(`identity_basis: frozen-content`) and exact generated variant pins. A prepared
checkout's `ownership.json` must record `observed_head` as exactly 40 lowercase
hexadecimal characters. This per-run Git provenance may identify any prepared
checkout whose pinned content matches; no historical or future commit allowlist
is required. Missing or malformed HEAD metadata is refused before writes.

The overlay receipt retains the prepared owner's actual `observed_head`.
Before launch admission or restoration, recheck all content pins and require
the receipt HEAD to equal the current prepared owner HEAD. A changed owner or
receipt HEAD is refused without changing source, mirror, backup, evidence or
target files. HEAD provenance never substitutes for any content or ownership
gate. Historical authoring HEADs belong in external run evidence.

Insert one unconditional test observer immediately before the first normal
`tauri::Builder::default()` in the mirror. Keep the actual preceding routing
decisions byte-identical. The observer emits marker
`LIGHT_WHISPER_ENTRYPOINT_BOUNDARY_ONLY_1` with its process ID, flushes stdout and
exits 86 before GUI/plugins/AppState. It is a verification overlay, never shipping
code or evidence that a GUI was exercised. Restore the exact pre-overlay bytes;
refuse later edits or corrupted backups.
`OBSERVER` is an exact frozen unconditional Rust block, independently duplicated
in its contract test. Launch admission must reconstruct and compare the entire
expected overlay, refusing conditional or unreachable exits even if the receipt's
transformed hash has been rewritten. Receipts use `evidence/ownership.json`.

Client probe: explicit `LIGHT_WHISPER_DATA_DIR=""`, owned cwd, synthetic
APPDATA/LOCALAPPDATA/TEMP/TMP, readiness followed only by broker-local exit.
It detects generated `lib.rs:16:49` host-predicate inversion. An absent variable
is REFUSED: Windows `dirs::data_dir()` uses the known-folder API, ignoring an
APPDATA override. `get_data_dir()` maps an explicit empty variable to an empty
PathBuf; local exit never reads the descriptor/config. No absent-variable claim.

Desktop probes: original argv or `--minimized` with nonempty owned data must emit
the boundary marker and exit 86. Generated `lib.rs:29:49` instead selects the
existing window-cleared host; its readiness cannot pass this assertion. The host
source is pinned and remains windowless with no engine autoload.
All other argv are refused before `Process` construction. Both empty and nonempty
probe environments override data/Windows paths inside the owned scratch root.

Runtime uses existing NativeHost setup/process/job/teardown directly under a new
strict identity gate, plus existing test06 unchanged. Existing main/test05 source
guards remain unchanged. Supplemental results stay separate from raw
cargo-mutants statuses; refusals are never behavioral kills.

Public helper API: `apply_overlay(mirror, evidence, target)`,
`restore(mirror, evidence)`,
`verify_launch(mirror, evidence, app_exe, app_sha256, fixture_exe)`,
`probe_environment(environment, scratch, empty_data=...)`,
`client_probe(case, driver, executable)`,
`desktop_probe(case, driver, executable, arguments)`.
`verify_launch` returns accepted `app_sha256` and `fixture_sha256` identities.
Its positive unit case uses explicitly synthetic artifacts and temporarily
substitutes only `TRUSTED_FIXTURE_SHA256`; real native admission retains the
cohort's trusted fixture pin. No unit artifact is launched.

The 32 Python tests check exact admission, immutable prefix, reversible overlay,
identity/ownership drift refusal and rejection of wrong client/desktop outcomes.
The finite routing and provenance models map the counterexamples and record the
empty-only assumption. They are included in the completed application formal
check. Native entrypoint execution is reported separately in the
[current verification record](../../evidence/agent-cli-180.md). Helper unit tests
alone do not establish native execution.
Before-write refusal tests cover all 21 identity pins in both source and mirror,
source/mirror nesting, hardlinks, Windows directory junctions (including matching
tracked source bytes), existing evidence/target and all output overlap directions.
Junction cleanup removes exact links before deleting the owned temporary root.
The public-checkout unit cases use a public HEAD and an explicit synthetic HEAD,
all three exact source variants, malformed/missing metadata, owner/receipt HEAD
drift and every missing or corrupt copied-source pin. They compile or launch no
native artifact. The unchanged finite routing model abstracts identity admission
as `identityValid`; it does not prove the receipt HEAD comparison or metadata
validation.

Narrow command (from checkout):

```text
python -m unittest discover -s formal/agent-api-measurements/entrypoint -p "test_*.py" -v
```
