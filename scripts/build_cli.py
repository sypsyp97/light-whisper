"""Build or verify the standalone Rust command-line client."""
import argparse
import json
import os
from pathlib import Path
import sys
import tempfile

from native_runtime_build import ROOT, build_binary, source_fingerprint, verify_executable

PACKAGE_JSON = ROOT / "package.json"
OUTPUT_EXE = ROOT / "src-tauri/resources/light-whisper-cli.exe"


def verify_cli(path=None):
    path = Path(path if path is not None else OUTPUT_EXE).resolve()
    verified = verify_executable(path, "cli", source_fingerprint("cli"))
    return {**verified, "version": json.loads(PACKAGE_JSON.read_bytes())["version"], "path": str(path)}


def build_cli():
    before = source_fingerprint("cli")
    OUTPUT_EXE.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".cli-build-", dir=OUTPUT_EXE.parent) as temporary:
        staged = build_binary("cli", Path(temporary))
        verify_cli(staged)
        if source_fingerprint("cli") != before:
            raise ValueError("CLI inputs changed during the build; rebuild the CLI")
        os.replace(staged, OUTPUT_EXE)
    return OUTPUT_EXE


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
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
