"""Bounded private-file checks in an owned whole-repository disposable copy."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


SOURCE = Path("src-tauri/src/agent_api/secure_files.rs")
HARNESS = Path("formal/agent-api-security")
WORKSPACE = '[workspace]\nmembers = ["formal/agent-api-security"]\nexclude = ["src-tauri"]\nresolver = "2"\n'


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare(repo: Path, output: Path, expected: str) -> Path:
    if digest(repo / SOURCE) != expected:
        raise RuntimeError("production source hash changed; review before preparing")
    if output.exists():
        raise RuntimeError("output must be a new, explicitly owned directory")
    output.mkdir(parents=True)
    copied = output / "repo"
    copied.mkdir()
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).split(b"\0")
    for raw in tracked:
        if not raw:
            continue
        relative = Path(os.fsdecode(raw))
        source = repo / relative
        if not source.is_file() or source.is_symlink():
            raise RuntimeError("tracked source is absent or a link: " + str(relative))
        target = copied / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    # Include the current writer-owned test delta, never a copied production shim.
    for source in sorted((repo / HARNESS).rglob("*")):
        if not source.is_file() or "target" in source.relative_to(repo / HARNESS).parts:
            continue
        target = copied / source.relative_to(repo)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    (copied / "Cargo.toml").write_text(WORKSPACE, encoding="utf-8")
    shutil.copy2(copied / HARNESS / "Cargo.lock", copied / "Cargo.lock")
    (output / "ownership.json").write_text(json.dumps({
        "kind": "private-file-verification-disposable", "source_repo": str(repo),
        "copied_repo": str(copied), "source_sha256": expected,
    }, indent=2) + "\n", encoding="utf-8")
    if digest(copied / SOURCE) != expected:
        raise RuntimeError("copied production source hash mismatch")
    return copied


def run(command: list[str], copied: Path, output: Path, name: str) -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS="2", CARGO_NET_OFFLINE="true", CARGO_TARGET_DIR=str(output / "target"))
    result = subprocess.run(command, cwd=copied, env=env, text=True,
                            capture_output=True, timeout=90)
    (output / (name + ".stdout.log")).write_text(result.stdout, encoding="utf-8")
    (output / (name + ".stderr.log")).write_text(result.stderr, encoding="utf-8")
    return result


def replace_once(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise RuntimeError("source control anchor is not unique: " + old[:70])
    return source.replace(old, new, 1)


def late_tighten_variant(source: str) -> str:
    source = replace_once(source, "GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,",
                          "GetSecurityInfo, SetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,")
    source = replace_once(source, "FILE_TYPE_DISK, READ_CONTROL,", "FILE_TYPE_DISK, READ_CONTROL, WRITE_DAC,")
    source = replace_once(source, "GENERIC_READ | GENERIC_WRITE | READ_CONTROL | DELETE,",
                          "GENERIC_READ | GENERIC_WRITE | READ_CONTROL | DELETE | WRITE_DAC,")
    broad = '''        // Source negative only: expose staging before the actual first write.
        let negative_private = CurrentUser::get()?.descriptor()?;
        let negative_status = unsafe {
            SetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(), null_mut(), null_mut(), null_mut())
        };
        if negative_status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(negative_status as i32));
        }
'''
    source = replace_once(source, "        staging.owned = true;\n",
                          "        staging.owned = true;\n" + broad)
    tighten = '''        // Source negative only: restore private ACL after secret bytes exist.
        let mut negative_present = 0;
        let mut negative_defaulted = 0;
        let mut negative_dacl = null_mut();
        if unsafe { GetSecurityDescriptorDacl(negative_private.0,
            &mut negative_present, &mut negative_dacl, &mut negative_defaulted) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let negative_status = unsafe {
            SetSecurityInfo(file.as_raw_handle(), SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(), null_mut(), negative_dacl, null_mut())
        };
        if negative_status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(negative_status as i32));
        }
'''
    return replace_once(source, "        file.sync_all()?;\n", "        file.sync_all()?;\n" + tighten)


def cargo_test(filter_name: str, exact: bool = False) -> list[str]:
    command = ["cargo", "test", "--offline", "--locked", "--manifest-path",
               str(HARNESS / "Cargo.toml"), "--test", "private_files", "-j", "2", "--",
               filter_name, "--test-threads=1", "--nocapture"]
    if exact:
        command.append("--exact")
    return command


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["prepare", "discover", "source-negative"])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expect-source-sha256", required=True)
    parser.add_argument("--mutants-tool", type=Path)
    args = parser.parse_args()
    repo = args.repo.resolve(strict=True)
    output = args.output.resolve()
    if output == repo or repo in output.parents:
        raise RuntimeError("disposable output must be outside the source checkout")
    copied = prepare(repo, output, args.expect_source_sha256)
    receipt: dict[str, object] = {"mode": args.mode, "copied_repo": str(copied),
        "source_sha256": digest(copied / SOURCE), "driver_sha256": digest(Path(__file__)),
        "test_sha256": digest(copied / HARNESS / "tests/private_files.rs")}
    if args.mode == "discover":
        if args.mutants_tool is None:
            raise RuntimeError("--mutants-tool is required for actual discovery")
        command = [str(args.mutants_tool.resolve(strict=True)), "mutants", "--manifest-path",
                   str(HARNESS / "Cargo.toml"), "--file", SOURCE.name, "--list", "--json"]
        result = run(command, copied, output, "discovery")
        mutants = json.loads(result.stdout) if result.returncode == 0 else []
        represented = bool(mutants) and all(
            (copied / item["file"]).resolve() == (copied / SOURCE).resolve() for item in mutants)
        passed = bool(mutants) and represented and "outside of tree" not in result.stderr
        receipt.update(command=command, exit_code=result.returncode,
                       generated_mutants=len(mutants), actual_source_discovered=represented, passed=passed)
    elif args.mode == "source-negative":
        baseline_command = cargo_test("publication_observes_private_stage")
        baseline = run(baseline_command, copied, output, "baseline-observed")
        if baseline.returncode or "2 passed; 0 failed" not in baseline.stdout:
            raise RuntimeError("baseline observed publication is not GREEN")
        source_path = copied / SOURCE
        original = source_path.read_text(encoding="utf-8")
        mutated = late_tighten_variant(original)
        source_path.write_text(mutated, encoding="utf-8")
        (output / "late-tighten.diff").write_text("".join(difflib.unified_diff(
            original.splitlines(keepends=True), mutated.splitlines(keepends=True),
            fromfile=SOURCE.as_posix(), tofile=SOURCE.as_posix())), encoding="utf-8")
        public_command = cargo_test("publication_creates_absent_destination_with_private_complete_bytes", exact=True)
        public = run(public_command, copied, output, "mutant-public-final-private")
        observed_command = cargo_test("publication_observes_private_stage")
        observed = run(observed_command, copied, output, "mutant-observed-before-write")
        combined = observed.stdout + observed.stderr
        passed = (public.returncode == 0 and "1 passed; 0 failed" in public.stdout
                  and observed.returncode == 101 and "0 passed; 2 failed" in observed.stdout
                  and combined.count("DACL must exist") == 2 and "Unsupported" not in combined)
        receipt.update(baseline_command=baseline_command, baseline_exit=baseline.returncode,
                       mutant_source_sha256=digest(source_path), public_command=public_command,
                       public_exit=public.returncode, observed_command=observed_command,
                       observed_exit=observed.returncode, prewrite_acl_assertion_detected=passed, passed=passed)
    else:
        receipt["passed"] = True
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2))
    return 0 if receipt["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
