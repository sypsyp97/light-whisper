"""Prepare a source-bound, disposable workspace without changing shipping files."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import stat
import subprocess
import tomllib


WORKSPACE = '''[workspace]
members = ["src-tauri", "formal/agent-api-security", "formal/agent-api-transport", "formal/agent-api-measurements/selector"]
resolver = "2"
'''
NATIVE_TARGET = '''
[target.'cfg(target_os = "windows")'.dev-dependencies.windows-sys]
version = "=0.59.0"
features = ["Win32_System_JobObjects"]

[[test]]
name = "agent_api_native_measurement"
path = "../formal/agent-api-measurements/native_measurement.rs"
'''


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify_dependency_identity(before: bytes, after: bytes) -> None:
    def registry(data):
        return {(p["name"], p["version"], p["source"], p.get("checksum"))
                for p in tomllib.loads(data.decode("utf-8")).get("package", []) if "source" in p}

    if registry(before) != registry(after):
        raise RuntimeError("dependency identity changed; review workspace lock before execution")


def reparse(path: Path) -> bool:
    try:
        information = path.lstat()
    except FileNotFoundError:
        return False
    return (stat.S_ISLNK(information.st_mode)
            or bool(getattr(information, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT))


def checked_source(repo: Path, relative: str) -> Path:
    name = Path(relative)
    if name.is_absolute() or name.drive or ".." in name.parts:
        raise RuntimeError("tracked path must be relative and stay inside source: " + relative)
    path = repo / name
    ancestor = repo
    for component in name.parts:
        ancestor = ancestor / component
        if reparse(ancestor):
            raise RuntimeError("tracked source traverses a link or reparse point: " + relative)
    if not path.resolve(strict=True).is_relative_to(repo):
        raise RuntimeError("tracked source is a link or outside checkout: " + relative)
    if not path.is_file():
        raise RuntimeError("tracked source is not a regular file: " + relative)
    return path


def prepare(repo: Path, output: Path, cohort: dict, tracked: list[str], *, observed_head=None) -> Path:
    repo = repo.resolve(strict=True)
    if output.exists() or reparse(output):
        raise RuntimeError("measurement output must be fresh")
    output = output.resolve()
    if output.is_relative_to(repo):
        raise RuntimeError("measurement output must be outside source checkout")
    for entry in [*cohort["source"], *cohort.get("tests", {}).values()]:
        if digest(checked_source(repo, entry["path"])) != entry["sha256"]:
            raise RuntimeError("production source identity changed: " + entry["path"])
    identities = {name: digest(checked_source(repo, name)) for name in sorted(set(tracked))}
    if not identities:
        raise RuntimeError("empty source selection")
    output.mkdir(parents=True)
    mirror = output / "repo"
    mirror.mkdir()
    for relative, expected in identities.items():
        source = checked_source(repo, relative)
        target = mirror / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        if digest(target) != expected:
            raise RuntimeError("source identity changed while copying: " + relative)
    (mirror / "Cargo.toml").write_text(WORKSPACE, encoding="utf-8", newline="\n")
    manifest = mirror / "src-tauri/Cargo.toml"
    manifest.write_bytes(manifest.read_bytes() + NATIVE_TARGET.encode())
    shutil.copy2(mirror / "src-tauri/Cargo.lock", mirror / "Cargo.lock")
    (output / "ownership.json").write_text(json.dumps({
        "kind": "native-agent-api-measurement", "source_repo": str(repo),
        "mirror": str(mirror), "baseline": cohort.get("baseline"),
        "observed_head": observed_head,
        "copied_source_sha256": identities,
        "shipping_manifest_sha256": digest(repo / "src-tauri/Cargo.toml"),
        "seed_lock_sha256": digest(repo / "src-tauri/Cargo.lock"),
        "mirror_manifest_sha256": digest(manifest),
    }, indent=2) + "\n", encoding="utf-8")
    return mirror


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["prepare", "check-lock"])
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--cohort", type=Path, default=Path(__file__).with_name("cohort.json"))
    args = parser.parse_args()
    repo = args.repo.resolve(strict=True)
    if args.mode == "check-lock":
        if args.output is None:
            parser.error("--output required")
        verify_dependency_identity((repo / "src-tauri/Cargo.lock").read_bytes(),
                                   (args.output / "repo/Cargo.lock").read_bytes())
        print("workspace dependency identities unchanged")
        return
    if args.output is None:
        parser.error("--output required")
    cohort = json.loads(args.cohort.read_text(encoding="utf-8"))
    observed_head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode("utf-8").split("\0")
    # These writer-owned files are intentionally untracked until root integrates them.
    owned = Path(__file__).resolve().parent
    tracked += [p.relative_to(repo).as_posix() for p in owned.rglob("*")
                if p.is_file() and "__pycache__" not in p.parts]
    mirror = prepare(repo, args.output, cohort, [p for p in tracked if p], observed_head=observed_head)
    # A shallow local fetch gives the existing native driver an independent HEAD.
    # No commit, remote publication, checkout, index write or source mutation occurs.
    subprocess.run(["git", "init", "--quiet", str(mirror)], check=True)
    subprocess.run(["git", "fetch", "--quiet", "--depth=1", str(repo), observed_head], cwd=mirror, check=True)
    subprocess.run(["git", "update-ref", "HEAD", observed_head], cwd=mirror, check=True)
    print(str(mirror))


if __name__ == "__main__":
    main()
