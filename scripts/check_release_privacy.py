"""Check a Windows release installer and its nested engine before publication."""

import lzma
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import zlib

from git_privacy import Blocked, WINDOWS_PROFILE, check_content, repository_markers


ENGINE = "resources/engine.tar.xz"


def check_bytes(data, markers):
    check_content(data, markers)
    plain = data.replace(b"\0", b"").lower()
    if (b"users" in plain or b"documents and settings" in plain) and WINDOWS_PROFILE.search(plain):
        raise Blocked("Windows user profile path")


def check_stream(stream, markers):
    tail = b""
    overlap = max(4096, 2 * max(map(len, markers), default=0))
    while chunk := stream.read(4 * 1024 * 1024):
        window = tail + chunk
        check_bytes(window, markers)
        tail = window[-overlap:]


def check_engine(path, markers):
    # The decoded TAR stream includes all filenames, extended headers and padding.
    with lzma.open(path, "rb") as stream:
        check_stream(stream, markers)
    with tarfile.open(path, "r|xz") as archive:
        found_engine = False
        for member in archive:
            if not member.isfile():
                raise ValueError("unexpected engine archive member")
            if member.name == "engine.exe":
                found_engine = True
        if not found_engine:
            raise ValueError("missing engine executable")


NSIS_SIGNATURE = bytes.fromhex("efbeadde") + b"NullsoftInst"
NSIS_HEADER = struct.Struct("<I16sII")


def check_solid_program(stream, available, markers, *, uninstaller=False):
    flags, magic, header_size, size = NSIS_HEADER.unpack(stream.read(NSIS_HEADER.size))
    if (magic != NSIS_SIGNATURE or bool(flags & 1) != uninstaller
            or not NSIS_HEADER.size < size <= available or not header_size):
        raise ValueError("invalid NSIS firstheader")
    # The solid zlib build stores the script and data in length-prefixed frames.
    # WriteUninstaller stores another compressed program as a data frame.
    with tempfile.TemporaryFile() as decoded:
        inflater = zlib.decompressobj(-15)
        remaining = size - NSIS_HEADER.size
        while remaining:
            block = stream.read(min(64 * 1024, remaining))
            if not block:
                raise EOFError
            remaining -= len(block)
            while block:
                decoded.write(inflater.decompress(block, 1024 * 1024))
                if decoded.tell() > 0xffffffff:
                    raise ValueError("decoded size exceeds NSIS frame offsets")
                block = inflater.unconsumed_tail
        if not inflater.eof:
            raise ValueError("incomplete NSIS stream")
        end = decoded.tell()
        decoded.seek(0)
        check_stream(decoded, markers)
        decoded.seek(0)
        while decoded.tell() < end:
            first = decoded.tell() == 0
            length, = struct.unpack("<I", decoded.read(4))
            start = decoded.tell()
            if length > end - start or (first and length != header_size):
                raise ValueError("invalid NSIS frame")
            prefix = decoded.read(min(length, NSIS_HEADER.size))
            if prefix[4:20] == NSIS_SIGNATURE:
                if uninstaller:
                    raise ValueError("unexpected nested uninstall program")
                decoded.seek(start)
                check_solid_program(decoded, length, markers, uninstaller=True)
            decoded.seek(start + length)
        if not end:
            raise ValueError("empty NSIS program")


def check_installer_programs(installer, seven_zip, markers):
    listing = subprocess.check_output([seven_zip, "l", "-slt", "-tNsis", "--", str(installer)],
                                      stderr=subprocess.DEVNULL).split(b"----------", 1)[0]
    fields = dict(line.split(b" = ", 1) for line in listing.splitlines() if b" = " in line)
    if fields.get(b"Method") != b"Deflate" or fields.get(b"Solid") != b"+":
        raise ValueError("unsupported NSIS layout")
    offset = int(fields[b"Embedded Stub Size"])
    with installer.open("rb") as stream:
        stream.seek(offset)
        check_solid_program(stream, installer.stat().st_size - offset, markers)


def verify(installer):
    seven_zip = shutil.which("7z") or shutil.which("7zz")
    if not seven_zip:
        seven_zip = str(Path(os.environ.get("ProgramFiles", "")) / "7-Zip" / "7z.exe")
    # Read this clone's local denylist even if invoked from the installer directory.
    os.chdir(Path(__file__).resolve().parents[1])
    markers = repository_markers()
    with installer.open("rb") as stream:
        check_stream(stream, markers)
    check_installer_programs(installer, seven_zip, markers)
    with tempfile.TemporaryDirectory(prefix="lw-release-") as tmp:
        root = Path(tmp)
        subprocess.run([seven_zip, "x", "-tNsis", "-y", "-o" + tmp, "--", str(installer)],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        files = set()
        for path in root.rglob("*"):
            name = path.relative_to(root).as_posix()
            check_bytes(name.encode(), markers)
            if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
                raise ValueError("unexpected installer link")
            if path.is_file():
                files.add(name)
                with path.open("rb") as stream:
                    check_stream(stream, markers)
        if not {"light-whisper.exe", "light-whisper-cli.exe", ENGINE} <= files:
            raise ValueError("missing installer payload")
        check_engine(root / ENGINE, markers)


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("expected installer argument")
        verify(Path(sys.argv[1]).resolve(strict=True))
    except Blocked as error:
        sys.exit(f"Installer privacy check blocked publication: {error}.")
    except (OSError, EOFError, ValueError, KeyError, struct.error, zlib.error, lzma.LZMAError,
            tarfile.TarError, subprocess.SubprocessError):
        sys.exit("Installer privacy check could not verify the entire payload; publication blocked.")
    print("Installer and engine privacy checks passed.")
