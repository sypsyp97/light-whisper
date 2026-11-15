"""Verification-only LLVM profile flushing in an owned native measurement mirror."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import shlex

from prepare import checked_source, digest, reparse


HOST = "src-tauri/src/agent_api/host.rs"
INSERT_BEFORE = 75
HOOK = '''            #[cfg(coverage)]
            {
                unsafe extern "C" {
                    fn __llvm_profile_write_file() -> std::os::raw::c_int;
                }
                // SAFETY: the instrumented binary links LLVM's profiling runtime with this C ABI.
                let status = unsafe { __llvm_profile_write_file() };
                if status != 0 {
                    eprintln!("Native coverage flush failed: {status}");
                }
            }
'''
ANCHOR = '''    app.run(|app, event| {
        if let tauri::RunEvent::Exit = event {
            shutdown(app);
            let state = app.state::<AppState>();
            if let Err(error) =
                tauri::async_runtime::block_on(services::funasr_service::stop_server(state.inner()))
            {
                eprintln!("Agent host shutdown failed: {error}");
            }
        }
    });
'''


def plain_path(path: Path, *, exists=False) -> Path:
    path = Path(os.path.abspath(path))
    for component in (path, *path.parents):
        if reparse(component):
            raise RuntimeError("measurement path traverses a link or reparse point: " + str(component))
    return path.resolve(strict=exists)


def ownership(path: Path, kind: str, mirror: Path) -> dict:
    try:
        data = json.loads(plain_path(path, exists=True).read_text(encoding="utf-8"))
        if data["kind"] == kind and plain_path(Path(data["mirror"]), exists=True) == mirror:
            return data
    except (OSError, ValueError, KeyError):
        pass
    raise RuntimeError("measurement ownership mismatch: " + str(path))


def exclusive_host(mirror: Path, source: Path) -> Path:
    host = checked_source(mirror, HOST)
    if host.stat().st_nlink != 1 or host.samefile(checked_source(source, HOST)):
        raise RuntimeError("mirror host has shared file identity or hardlinks to another file")
    return host


def apply_overlay(mirror: Path, cohort: dict, profiles: Path, environment: dict) -> dict:
    mirror = plain_path(mirror, exists=True)
    owner = ownership(mirror.parent / "ownership.json", "native-agent-api-measurement", mirror)
    source = plain_path(Path(owner["source_repo"]), exists=True)
    if mirror.is_relative_to(source) or source.is_relative_to(mirror):
        raise RuntimeError("measurement mirror must stay outside shipping source")
    if any(not environment.get(key) or not Path(environment[key]).is_absolute()
           for key in ("CARGO_TARGET_DIR", "CARGO_LLVM_COV_TARGET_DIR", "LLVM_PROFILE_FILE")):
        raise RuntimeError("coverage target and profile paths must be explicit and absolute")
    target = plain_path(Path(environment["CARGO_LLVM_COV_TARGET_DIR"]))
    if plain_path(Path(environment["CARGO_TARGET_DIR"])) != target:
        raise RuntimeError("coverage target environment mismatch")
    if target.is_relative_to(mirror) or target.is_relative_to(source):
        raise RuntimeError("coverage target must be external and outside source")
    ownership(target / "ownership.json", "direct-instrumentation-target", mirror)
    profiles = plain_path(profiles)
    if profiles.exists():
        raise RuntimeError("profile directory must be fresh; never merge previous profiles")
    if profiles.parent != target:
        raise RuntimeError("fresh profile directory must be directly inside the owned coverage target")
    template = profiles / "flush-%p-%m.profraw"
    if environment.get("LLVM_PROFILE_FILE") != str(template):
        raise RuntimeError("LLVM profile template must use the fresh owned profile directory")
    flags = shlex.split(environment.get("RUSTFLAGS", ""))
    if "--cfg=coverage" not in flags or not any(
            flags[index:index + 2] == ["-C", "instrument-coverage"] for index in range(len(flags))):
        raise RuntimeError("coverage cfg and instrument-coverage flags are required")
    host = exclusive_host(mirror, source)
    original = host.read_bytes()
    normalized = original.decode("utf-8").replace("\r\n", "\n")
    if normalized.count(ANCHOR) != 1 or normalized.splitlines()[65:76] != ANCHOR.splitlines():
        raise RuntimeError("exact graceful shutdown anchor missing, moved or duplicated")
    entries = [*cohort["source"], *cohort.get("tests", {}).values()]
    for entry in entries:
        relative, expected = entry["path"], entry["sha256"]
        if (owner["copied_source_sha256"].get(relative) != expected
                or digest(checked_source(mirror, relative)) != expected
                or digest(checked_source(source, relative)) != expected):
            raise RuntimeError("frozen source/test identity changed: " + relative)
    newline = "\r\n" if b"\r\n" in original else "\n"
    hook = HOOK.replace("\n", newline).encode()
    lines = original.splitlines(keepends=True)
    transformed = b"".join(lines[:INSERT_BEFORE - 1]) + hook + b"".join(lines[INSERT_BEFORE - 1:])
    count = len(HOOK.splitlines())
    receipt = {
        "kind": "native-llvm-profile-flush-overlay", "mirror": str(mirror),
        "source_repo": str(source), "observed_head": owner.get("observed_head"),
        "profile_directory": str(profiles), "llvm_profile_file": str(template),
        "rustflags": environment["RUSTFLAGS"], "target": str(target),
        "original_host_sha256": hashlib.sha256(original).hexdigest(),
        "transformed_host_sha256": hashlib.sha256(transformed).hexdigest(),
        "frozen_source_sha256": {e["path"]: e["sha256"] for e in cohort["source"]},
        "frozen_test_sha256": {e["path"]: e["sha256"] for e in cohort.get("tests", {}).values()},
        "line_map": [
            {"original": [1, INSERT_BEFORE - 1], "generated": [1, INSERT_BEFORE - 1]},
            {"original": None, "generated": [INSERT_BEFORE, INSERT_BEFORE + count - 1]},
            {"original": [INSERT_BEFORE, len(lines)], "generated": [INSERT_BEFORE + count, len(lines) + count]},
        ],
        "limit": "Post-flush callbacks, process exit and hard-killed processes remain unmeasured; overlay-only counters are not original coverage.",
    }
    profiles.mkdir()
    (profiles / "host.rs.original").write_bytes(original)
    (profiles / "overlay.diff").write_text("".join(difflib.unified_diff(
        normalized.splitlines(keepends=True), transformed.decode().replace("\r\n", "\n").splitlines(keepends=True),
        fromfile=HOST + ".original", tofile=HOST + ".coverage-overlay")), encoding="utf-8", newline="\n")
    (profiles / "ownership.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    host.write_bytes(transformed)
    return receipt


def restore(mirror: Path, profiles: Path) -> None:
    mirror, profiles = plain_path(mirror, exists=True), plain_path(profiles, exists=True)
    receipt = ownership(profiles / "ownership.json", "native-llvm-profile-flush-overlay", mirror)
    if plain_path(Path(receipt["profile_directory"]), exists=True) != profiles:
        raise RuntimeError("overlay profile ownership mismatch")
    host = exclusive_host(mirror, plain_path(Path(receipt["source_repo"]), exists=True))
    original = plain_path(profiles / "host.rs.original", exists=True).read_bytes()
    if (digest(host) != receipt["transformed_host_sha256"]
            or hashlib.sha256(original).hexdigest() != receipt["original_host_sha256"]):
        raise RuntimeError("overlay or original identity changed; refusing to overwrite later edits")
    host.write_bytes(original)


def original_line(line: int) -> int | None:
    count = len(HOOK.splitlines())
    if INSERT_BEFORE <= line < INSERT_BEFORE + count:
        return None
    return line if line < INSERT_BEFORE else line - count


def check_logs(evidence: Path) -> int:
    logs = sorted(plain_path(evidence, exists=True).rglob("*-stderr.txt"))
    if not logs:
        raise RuntimeError("native stderr logs missing; flush status not verified")
    for log in logs:
        if b"Native coverage flush failed:" in plain_path(log, exists=True).read_bytes():
            raise RuntimeError("native coverage flush failed; measurement invalid: " + str(log))
    return len(logs)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["apply", "restore", "check-logs"])
    parser.add_argument("--mirror", type=Path)
    parser.add_argument("--profiles", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--cohort", type=Path, default=Path(__file__).with_name("cohort.json"))
    args = parser.parse_args()
    if args.mode == "check-logs":
        if args.evidence is None:
            parser.error("--evidence required")
        print(f"checked {check_logs(args.evidence)} native stderr logs; no flush failure marker")
    else:
        if args.mirror is None or args.profiles is None:
            parser.error("--mirror and --profiles required")
        if args.mode == "restore":
            restore(args.mirror, args.profiles)
            print("original host bytes restored; evidence retained")
        else:
            receipt = apply_overlay(args.mirror, json.loads(args.cohort.read_text(encoding="utf-8")),
                                    args.profiles, dict(os.environ))
            print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
