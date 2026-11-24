"""Build the native executables and bind their verified PE bytes to release sources."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT / "rust-runtime"
NAMES = {"cli": "light-whisper-cli", "engine": "engine"}
STAMP = b"LIGHT_WHISPER_RUST_BUILD_V1\0"


def source_fingerprint(component):
    crates = ["cli"] if component == "cli" else ["engine", "native-asr", "reuse", "vad"]
    files = [WORKSPACE / "Cargo.toml", WORKSPACE / "Cargo.lock", Path(__file__).resolve(),
             ROOT / f"scripts/build_{component}.py"]
    if component == "cli":
        files.append(ROOT / "package.json")
    else:
        files.append(ROOT / "src-tauri/windows-app-manifest.xml")
    for name in crates:
        crate = WORKSPACE / "crates" / name
        files.append(crate / "Cargo.toml")
        files.extend(crate.glob("build.rs"))
        for directory in ("src", "native"):
            files.extend(p for p in (crate / directory).rglob("*") if p.is_file())
    digest = hashlib.sha256()
    for path in sorted(files, key=lambda p: p.relative_to(ROOT).as_posix()):
        content = path.read_bytes().replace(b"\r\n", b"\n")
        digest.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
        digest.update(hashlib.sha256(content).digest())
    return digest.hexdigest()


def check_pe(data):
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Expected a Windows x64 PE executable")
    pe = struct.unpack_from("<I", data, 60)[0]
    if not 64 <= pe <= len(data) - 26 or data[pe:pe+4] != b"PE\0\0":
        raise ValueError("Invalid PE header")
    machine, sections = struct.unpack_from("<HH", data, pe+4)
    optional, flags, magic = struct.unpack_from("<HHH", data, pe+20)
    if (machine != 0x8664 or not sections or optional < 112 or magic != 0x20b
            or not flags & 2 or flags & 0x2000
            or pe + 24 + optional + sections * 40 > len(data)):
        raise ValueError("Expected an executable AMD64 PE32+ image")


def footer(component, fingerprint, image_hash):
    return STAMP + component.encode("ascii") + b"\0" + bytes.fromhex(fingerprint) + image_hash


def stamp_executable(path, component, fingerprint):
    data = path.read_bytes()
    check_pe(data)
    with path.open("ab") as out:
        out.write(footer(component, fingerprint, hashlib.sha256(data).digest()))


def verify_image(data, component, fingerprint):
    trailer_size = len(footer(component, fingerprint, bytes(32)))
    image = memoryview(data)[:-trailer_size]
    check_pe(image)
    expected = footer(component, fingerprint, hashlib.sha256(image).digest())
    if data[-trailer_size:] != expected:
        raise ValueError(f"Stale or corrupt native {component} build; rebuild the executable")
    return {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def verify_executable(path, component, fingerprint):
    return verify_image(path.read_bytes(), component, fingerprint)


def build_binary(component, staging):
    fingerprint = source_fingerprint(component)
    name = NAMES[component]
    cmd = ["cargo", "rustc", "--locked", "--release", "--manifest-path", str(WORKSPACE / "Cargo.toml"),
           "--package", f"light-whisper-{component}", "--bin", name, "--message-format=json-render-diagnostics"]
    if component == "engine":
        cmd.extend(["--", "-C", "link-arg=/MANIFEST:EMBED", "-C",
                    f"link-arg=/MANIFESTINPUT:{ROOT / 'src-tauri/windows-app-manifest.xml'}"])
    env = os.environ.copy()
    flags = (env["CARGO_ENCODED_RUSTFLAGS"].split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS")
             else env.get("RUSTFLAGS", "").split())
    for path, stable in ((ROOT, "."), (Path(env.get("CARGO_HOME", Path.home() / ".cargo")), "/cargo"),
                         (Path.home(), "/home")):
        flags.append(f"--remap-path-prefix={path}={stable}")
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    result = subprocess.run(cmd, cwd=ROOT, env=env, check=True, stdout=subprocess.PIPE, encoding="utf-8",
                            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    executable = None
    for line in result.stdout.splitlines():
        event = json.loads(line)
        if (event.get("reason") == "compiler-artifact" and event["target"]["name"] == name
                and "bin" in event["target"]["kind"] and event.get("executable")):
            executable = Path(event["executable"])
    if executable is None:
        raise RuntimeError(f"Cargo produced no executable for {name}")
    if source_fingerprint(component) != fingerprint:
        raise ValueError(f"{component} inputs changed during compilation")
    staged = staging / f"{name}.exe"
    shutil.copyfile(executable, staged)
    stamp_executable(staged, component, fingerprint)
    return staged
