"""Build a tiny native engine bundle for Windows CLI process tests."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile

# Use the checked-in release footer writer against the synthetic PE.
sys.path.insert(0, str(Path(__file__).resolve().parents[5] / "scripts"))
from native_runtime_build import stamp_executable

peer, archive, mode = sys.argv[1:]
peer, archive = Path(peer), Path(archive)
source = hashlib.sha256(b"test-only native engine source").hexdigest()
stamp_executable(peer, "engine", source)
signed = peer.read_bytes()
if mode == "bad-image":
    signed = signed[:-1] + bytes([signed[-1] ^ 1])
asset_name = "_internal/test-asset.txt"
entries = {"engine.exe": signed, asset_name: b"bundled test asset\n"}
if mode == "linked-asset":
    entries["_internal/test-asset-copy.txt"] = entries[asset_name]
manifest = {"format": 1, "source_sha256": source, "files": {
    name: {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
    for name, data in entries.items()
}}
entries["engine-distribution.json"] = json.dumps(manifest).encode()
archive.parent.mkdir(parents=True, exist_ok=True)
with tarfile.open(archive, "w:xz", preset=0) as tar:
    for name, data in entries.items():
        info = tarfile.TarInfo(name)
        if mode == "linked-asset" and name == asset_name:
            # Target is a harmless regular sibling inside the test bundle.
            info.type = tarfile.SYMTYPE
            info.linkname = "test-asset-copy.txt"
            tar.addfile(info)
            continue
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))
if mode == "truncated":
    data = archive.read_bytes()
    archive.write_bytes(data[:len(data) // 2])
print(hashlib.sha256(archive.read_bytes()).hexdigest())
