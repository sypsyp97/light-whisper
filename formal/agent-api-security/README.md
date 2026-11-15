# Windows private endpoint-file foundation

This is a small verification crate, not a second implementation. The integration
test and native child import `src-tauri/src/agent_api/secure_files.rs` by path.
The crate avoids building Tauri, uses production-pinned dependency versions, and
has no access to microphone, GPU, credentials, app data, or network services.

The frozen API is:

```rust
pub fn create_private_file(path: &Path) -> io::Result<File>;
pub fn open_private_file(path: &Path, write: bool) -> io::Result<File>;
pub fn ensure_private_dir(path: &Path) -> io::Result<()>;
pub fn publish_private_file(path: &Path, bytes: &[u8]) -> io::Result<()>;
```

Creation uses CREATE_NEW and returns a read/write, non-inheritable file handle.
Open always permits reads; `write = true` additionally permits writes, without
truncation. Returned handles omit FILE_SHARE_DELETE. Validation must inspect the
opened handle: owner equals the process TokenUser SID, a protected non-null DACL
is present, only current user/System full-access allow ACEs exist, no ACE inherits,
and the object is a regular non-reparse file. Existing unsafe objects fail closed.
Private directories apply the same ownership/DACL contract and reject reparse
points and regular files. Publication creates a private same-directory temporary
before writing bytes, replaces the destination atomically, and cleans its own
temporary on failure. Ordinary temporary-file inheritance and retaining an old
destination DACL are unsafe for this contract.

The declaration-only test seam is:

```rust
#[cfg(test)]
pub enum PublishPoint { BeforeFirstWrite, BeforeReplace }
#[cfg(test)]
pub fn publish_private_file_observed(
    path: &Path,
    bytes: &[u8],
    observe: &mut dyn FnMut(PublishPoint, &Path, &File),
) -> io::Result<()>;
```

Both publication entrypoints must route through one actual production core.
The observer receives that core's actual staging handle and path, immediately
before the first secret write and after complete staging, before replacing the
destination on the still-live DELETE-capable stage handle. A separate test implementation, synthetic
event replay or callback emitted only after writing does not satisfy this seam.

## Executed RED

The initial test slice had no production
module. The first compile therefore reported INTERFACE_PENDING: missing
`src-tauri/src/agent_api/secure_files.rs` (Windows OS error 3).

The original declaration-only baseline is
`87a40faab3929724a09cb484a9f311d241f72d26`. The revised frozen baseline is
`963ac3b2b65b257cdd93ffaddb9506579e3347a6`; it adds only the test enum and observed
function declaration above. All executable declarations return
`io::ErrorKind::Unsupported`. The semantic RED is scaffold-based new-feature
evidence, not a regression reproduced in the old desktop application.

From repository root on Windows:

```powershell
cargo test --offline --locked --manifest-path formal/agent-api-security/Cargo.toml --test private_files --no-run -j 2
cargo test --offline --locked --manifest-path formal/agent-api-security/Cargo.toml --test private_files -j 2 -- --test-threads=1 --nocapture
```

The collection/compile check passes. The behavior command collects 16 tests:
7 pass, 9 fail at the expected Unsupported results for creation, reopening,
directory creation, existing/absent/observed publication, cross-process lock setup,
and restricted-read setup (cargo exit 101). The negative-path tests passing on
Unsupported do not establish a working
implementation; positive tests prevent a reject-everything implementation from
passing the suite.

The functional private-file prerequisite is frozen in
`24930f10002fa0cf1f3eeaf02e764a0e7b6833b6`: all 16 native tests pass. Publication
uses `SetFileInformationByHandle(FileRenameInfoEx)` with replacement and POSIX
semantics flags on the live stage handle. It retains the staged object's identity
and old destination handles; the stage is closed after replacement. It does not
close the stage and call ordinary path-based MoveFileExW.

## Post-creation validation failure

The post-creation failure test scaffold retained the
functional private core and adds only this declaration returning Unsupported:

```rust
#[cfg(test)]
pub fn with_post_create_validation_error<T>(
    error: io::Error,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T>;
```

Its contract is a thread-local RAII scope: invoke the real operation; inject the
supplied error once after the next successful CREATE_NEW owns a File but before
post-creation validation; restore previous scope state on success, error and
unwind. A preexisting-path rejection must not consume the fault. Open and
directory validation are outside this injected failure boundary.

The six new tests assert exact native error propagation, removal of an empty
new file and empty publication stage, preservation of the old destination and
unrelated neighbor, preexisting-file identity/ACL preservation, one-shot behavior,
scope restoration after nested/normal/error/unwind exits, and cross-thread isolation.
While a fault is armed, actual read/write opens of an existing private file and
validation/creation of existing/new private directories must succeed without
consuming it; the next successful CREATE_NEW must still consume it once. Nested
unused inner error and caught-panic scopes separately restore an armed outer fault.
They call actual public
create and shared publication paths, without invalid handles or replacement
implementations. The 22-test scaffold run has 16 PASS and 6 expected seam-pending
failures (exit 101). This is not yet a runtime reproduction of the cleanup bug.
The test-only hook must be wired before the cleanup change, then the actual leaked
file/stage assertion failures must be recorded separately before GREEN.

The native source currently validates after CREATE_NEW, while publication records
staging ownership only after create returns successfully. A post-create validation
error therefore crosses an owned-artifact cleanup boundary. The model separates
successful creation, unvalidated empty artifact, validation result and cleanup;
the forgotten-unvalidated-ownership negative must fail CreationFailureCleansEmpty.

## Native schedules and limits

| Requirement | Observable check | Model binding |
| --- | --- | --- |
| Private before secret write | Inspect actual opened-file ACL, owner, handle flags and length before writing synthetic bytes under a broad parent | CreateTemp, WriteSecret, PrivateBeforeSecret |
| CREATE_NEW | Existing broad file retains bytes and ACL | Existing-file rejection; abstracted outside ordering model |
| Same opened-handle validation | Inspect returned file handle and read/write it; reject broad, null, unprotected and propagating ACL fixtures | Private predicate is the native-validation abstraction |
| Directory ownership | Create and reopen private directory; reject broad folder and ordinary file | Private prerequisite; no invented controller owner |
| Reparse rejection | A real NTFS junction is rejected without modifying its private target | Native-only object-type boundary |
| Private publication | Publish to an absent path; replace broad stale descriptor; verify complete new bytes/private ACL; repeat replacement | Publish, PublishedPrivate; both DestinationInitiallyExists cases |
| Private staging before first secret write | At actual write boundary verify private ACL/owner/handle, zero bytes, same-parent staging path distinct from destination, and matching path/handle object identity | CreateTemp, WriteSecret, PrivateBeforeSecret; LateTighten negative |
| Atomic publication boundary | Before replace, stage contains complete bytes while destination remains old/absent; final destination has stage's native file identity; old open handle retains old object/bytes | UnpublishedDestinationUnchanged, PublishedMovesStaging |
| Failure cleanup | Destination held without delete sharing; publication fails, old bytes and unrelated neighbor remain, owned temp absent | PublishFails, Cleanup, FailureCleansTemp, CleanupOwnsOnlyTemp |
| Failed post-create validation | Inject an exact error after CREATE_NEW; new file/stage absent, preexisting destination and unrelated file unchanged | CreateTemp, ValidateCreated, ValidationFails, CleanupCreationFailure, CreationFailureCleansEmpty |
| Host lock identity | One child holds actual File::try_lock; second child is blocked; rename reports sharing violation; successor acquires after explicit release | Acquire, Release, ReplaceLockPath, RejectLockPathReplacement, SingleHost |
| OS access control | Isolated child impersonates a restricted token, reads broad control, receives ERROR_ACCESS_DENIED for private file, reverts, reads private file normally | Native-only access-control check |

The independent native fixture test passes using Windows security APIs directly.
The restricted token has the same user identity with an Everyone restricting SID
and disabled maximum privileges. This is actual OS access denial, not proof of
access denial for another signed-in account. RevertToSelf runs on all normal,
error and unwind paths; failure aborts only the isolated child.

`private-file-probe fixture-capabilities -` measured:

- File symlink fixture: ERROR_PRIVILEGE_NOT_HELD (1314). Directory junctions are
  available and tested. A file-reparse-specific runtime guard check remains OPEN.
- A non-user built-in Administrators owner fixture: ERROR_INVALID_OWNER (1307).
  Wrong-owner runtime discrimination remains OPEN; no foreign SID is treated as
  evidence that another account was exercised.

The handle inspection does not deterministically race an adversary swapping the
path during production open. Source review must confirm that owner, DACL and type
validation all use the returned handle, and source controls must challenge those
guards. OS security-call failures that cannot be induced in this unprivileged
environment remain an explicit coverage limitation until measured on real code.

All test paths live under an independently created random temporary directory.
Children have explicit 10-second deadlines and are killed/reaped by owned Child
handles on error. Cleanup removes only the fixture directory and its children.

## Finite checks

```powershell
python formal/agent-api-security/check_model.py --jar <existing-tla2tools.jar>
```

SANY passes. The closure matrix has 19 cases: two exhaustive positives (existing
and absent destination), nine safety negative controls, one progress negative
control, and seven reachability witnesses. Each positive explores 718 generated
states, 270 distinct states and depth 12. Bounds remain two hosts,
two lock-file path generations, one publication, and four file-identity representatives. Every configured negative
fails its named intended invariant; the cleanup-stall control fails the configured
PublicationSettles temporal property. Witness violations demonstrate private
write, publication, failure cleanup, protected held-lock path and successor states.
The late-tighten witness explicitly reaches a private published final state after
a broad staging file has already held secret bytes. Its independent safety
negative fails PrivateBeforeSecret at the exposure state, preventing final ACL
inspection from hiding the unsafe interval. Direct-destination-write and copy
controls fail the visibility and staged-file-identity invariants respectively.

The model separates private creation, post-create validation and its failure cleanup,
secret write, late tightening, publish,
publication failure and cleanup. It assumes successful Windows ACL creation/validation means private,
and File::try_lock excludes simultaneous locks on the same underlying file.
These assumptions have separate native checks. Weak fairness for the publication
advance action assumes native operations eventually return; no scheduler or crash
durability promise is proved. Terminal stuttering is explicit; only the negative
cleanup-stall variant adds a failed-phase loop. No state constraints, symmetry
reductions or disabled deadlock checks are used.

The model is an abstract contract, not refinement proof. Its production binding
must be reviewed against the eventual implementation and source hash. No TCP,
controller, bearer-token generation, Python reader, installed-app, or GUI claim
belongs to this slice.

The real shared-core source negative is executed and discriminating. An owned
whole-repository copy uses the actual production module. After creating the stage,
the variant applies a broad NULL DACL before the actual first-write callback,
writes the secret, restores the private DACL after sync and uses the unchanged
live-handle atomic rename. WRITE_DAC is added only in that disposable variant so
the real Windows permission changes can execute. No copied mock implements
publication. On the unmodified source both observed tests PASS. On the variant,
the public final-private publication test still PASSes, while both observed tests
fail their pre-write `DACL must exist` assertion (exit 101). This closes the
final-ACL-only blind spot for that source identity; it is separate from framework
mutation scoring and must be refreshed when relevant source changes.
The observer sequence and zero-length assertion also reject omitted callbacks
and a first-write callback moved after secret bytes exist. Native file identities
and the retained old handle discriminate copying or rewriting the destination;
source review binds the replacement boundary to one same-directory
SetFileInformationByHandle FileRenameInfoEx replacement on the stage's live
DELETE-capable handle, with no preceding unlink or destination write.

## Measurement feasibility

The production cohort is the complete new `secure_files.rs` module and all four
entrypoints, including native error/cleanup branches. Final line/region/branch
coverage and source-mutant outcomes are not yet measured against the closure implementation.
Task-local cargo-llvm-cov 0.9.1, cargo-mutants 27.1.0 and an isolated nightly LLVM
toolchain are available without changing the global Rust toolchain.

Normal cargo-mutants manifest discovery skips the production file because the
`#[path]` target is outside this small crate's tree: `--list --json` returns `[]`
with an explicit outside-tree warning. The approved whole-repository disposable
copy solves that discovery boundary by adding a test-only root workspace with
the harness member and excluding src-tauri as a Cargo package. The actual source
remains at its original path, imported by the harness. `--file secure_files.rs`
finds 121 source mutants at the closure-hook declaration snapshot, and every
resolved mutant file equals the copied production module. A canonical full-path
glob returns zero because cargo-mutants retains the module's `../../` spelling;
the driver checks nonempty discovery and resolved source identity explicitly.
Discovery is not execution or a mutation score. Counts must be regenerated after
source changes; no root workspace file is exported to the application checkout.

```powershell
python formal/agent-api-security/private_measurement.py discover --output <new-owned-dir> --expect-source-sha256 <reviewed-source-hash> --mutants-tool <task-local-cargo-mutants.exe>
python formal/agent-api-security/private_measurement.py source-negative --output <new-owned-dir> --expect-source-sha256 <reviewed-source-hash>
```

The driver preserves receipts, source-control diff and logs outside the disposable
repo. It refuses source-hash drift and existing output directories. A source
control edits only the copied actual module; it never edits either shared role.
Heavy coverage and mutation execution require the coordinated single-worker slot
after the new cleanup slice is GREEN. Global Rust and application dependencies
remain unchanged.

Tool versions used for RED: Rust 1.93.0, Windows x86_64 MSVC, Java Temurin 8u462.
The existing tla2tools.jar SHA256 is
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
Runner output contains source/model/config hashes, exact commands and per-case
expected outcomes. The fault scope remains declaration-only at this checkpoint;
its later actual error-path RED and final implementation identity need distinct
receipts before claiming closure of the post-create cleanup contract.
