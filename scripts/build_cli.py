import argparse
import hashlib
import json
import marshal
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import types


PROJECT_ROOT = Path(__file__).resolve().parents[1]
ENTRY_SCRIPT = PROJECT_ROOT / "scripts" / "light_whisper_cli.py"
PACKAGE_JSON = PROJECT_ROOT / "package.json"
OUTPUT_EXE = PROJECT_ROOT / "src-tauri" / "resources" / "light-whisper-cli.exe"


def verify_cli(path=None):
    from PyInstaller.archive.readers import CArchiveReader
    from PyInstaller.building.utils import get_code_object, replace_filename_in_code_object

    path = Path(path if path is not None else OUTPUT_EXE).resolve()
    with path.open("rb") as stream:
        header = stream.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            raise ValueError("CLI artifact is not a Windows executable")
        pe_offset = struct.unpack_from("<I", header, 0x3C)[0]
        if pe_offset < 64:
            raise ValueError("CLI artifact has an invalid PE header")
        stream.seek(pe_offset)
        if stream.read(4) != b"PE\0\0":
            raise ValueError("CLI artifact has an invalid PE signature")
    try:
        archive = CArchiveReader(str(path))
        packaged = archive.extract("package.json")
        if packaged != PACKAGE_JSON.read_bytes():
            raise ValueError("CLI package metadata is stale; rebuild the CLI")
        code = marshal.loads(archive.extract("light_whisper_cli"))
        expected = replace_filename_in_code_object(
            get_code_object("light_whisper_cli", str(ENTRY_SCRIPT), optimize=0),
            "light_whisper_cli.py",
        )
        if not isinstance(code, types.CodeType) or code != expected:
            raise ValueError("CLI entry code is stale; rebuild the CLI")
        pending = [code]
        while pending:
            current = pending.pop()
            if current.co_filename != "light_whisper_cli.py":
                raise ValueError("CLI entry code contains an unexpected build path")
            pending.extend(value for value in current.co_consts if isinstance(value, types.CodeType))
        version = json.loads(packaged)["version"]
    except Exception as error:
        raise ValueError(f"CLI archive verification failed: {error}") from error
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return {"version": version, "sha256": digest.hexdigest(),
            "size": path.stat().st_size, "path": str(path)}


def build_cli():
    inputs = (ENTRY_SCRIPT.read_bytes(), PACKAGE_JSON.read_bytes())
    OUTPUT_EXE.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".cli-build-", dir=OUTPUT_EXE.parent) as temporary:
        staging = Path(temporary)
        subprocess.run([
            sys.executable, "-m", "PyInstaller", "--noconfirm", "--clean",
            "--onefile", "--console", "--optimize", "0", "--name", "light-whisper-cli",
            "--distpath", str(staging / "dist"), "--workpath", str(staging / "work"),
            "--specpath", str(staging), "--add-data", f"{PACKAGE_JSON}{os.pathsep}.",
            str(ENTRY_SCRIPT),
        ], cwd=PROJECT_ROOT, check=True, stdout=sys.stderr,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        staged = staging / "dist" / "light-whisper-cli.exe"
        verify_cli(staged)
        if inputs != (ENTRY_SCRIPT.read_bytes(), PACKAGE_JSON.read_bytes()):
            raise ValueError("CLI inputs changed during the build; rebuild the CLI")
        os.replace(staged, OUTPUT_EXE)
    return OUTPUT_EXE


def main(argv=None):
    parser = argparse.ArgumentParser(description="Build or verify the standalone Light Whisper CLI")
    parser.add_argument("--verify", nargs="?", const="", metavar="EXE")
    args = parser.parse_args(argv)
    try:
        path = build_cli() if args.verify is None else (Path(args.verify) if args.verify else OUTPUT_EXE)
        print(json.dumps(verify_cli(path), ensure_ascii=True))
        return 0
    except Exception as error:
        print(f"CLI build failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
