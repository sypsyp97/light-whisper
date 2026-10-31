"""RE-USE package provenance through real validators; fake DLL bytes only."""

import hashlib
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock

import build_engine
import build_r2t2_runtime
import build_reuse_runtime


class ReusePackagingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.resources = self.root / "resources"
        self.resources.mkdir()
        # Only the resource root is replaced; file lists and validators are real.
        for name in build_engine.ADD_DATA_FILES:
            if name.endswith(".py"):
                (self.resources / name).write_bytes((build_engine.RESOURCES_DIR / name).read_bytes())
        self.enterContext(mock.patch.object(build_engine, "RESOURCES_DIR", self.resources))
        self.archive = self.root / "engine.tar.xz"
        self.manifests = {}
        for backend in ("cpu", "cuda"):
            directory = self.resources / "r2t2-native" / backend
            directory.mkdir(parents=True)
            payloads = {name: f"synthetic {backend} {name}".encode() for name in ("audiocpp.dll", "reuse.dll")}
            for name, data in payloads.items():
                (directory / name).write_bytes(data)
            manifest = {
                "source": build_r2t2_runtime.SOURCE_URL,
                "build_id": build_engine.R2T2_BUILD_ID,
                "revision": build_engine.R2T2_REVISION,
                "patch_sha256": hashlib.sha256(build_engine.R2T2_PATCH.read_bytes()).hexdigest(),
                "backend": backend,
                "reuse_source_sha256": build_reuse_runtime.source_fingerprint(),
                "files": {name: {"size": len(data), "sha256": hashlib.sha256(data).hexdigest()}
                          for name, data in payloads.items()},
            }
            self.manifests[backend] = manifest
            self.write_manifest(backend, manifest)

    def write_manifest(self, backend, manifest):
        path = self.resources / "r2t2-native" / backend / "runtime-manifest.json"
        path.write_text(json.dumps(manifest), encoding="utf-8")

    def package(self):
        with tarfile.open(self.archive, "w:xz") as archive:
            for path in sorted(self.resources.rglob("*")):
                if path.is_file():
                    archive.add(path, arcname=f"_internal/{path.relative_to(self.resources).as_posix()}")

    def assert_rejected_locally_and_in_archive(self):
        with self.subTest(validator="local"), self.assertRaises(RuntimeError):
            build_engine.validate_r2t2_runtime()
        self.package()
        with self.subTest(validator="archive"), self.assertRaises(RuntimeError):
            build_engine.verify_archive_sources(self.archive)

    def test_valid_cpu_cuda_reuse_packs_and_archive_are_accepted(self):
        expected_hash = hashlib.sha256()
        for path in (Path(build_reuse_runtime.ROOT) / "scripts/native/reuse_native.cpp",
                     Path(build_reuse_runtime.__file__)):
            expected_hash.update(path.read_bytes().replace(b"\r\n", b"\n"))
        self.assertEqual(build_reuse_runtime.source_fingerprint(), expected_hash.hexdigest())
        self.assertEqual(build_engine.validate_r2t2_runtime(), self.resources / "r2t2-native")
        self.package()
        build_engine.verify_archive_sources(self.archive)

    def test_missing_reuse_in_either_backend_is_rejected_even_with_consistent_file_table(self):
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                library = self.resources / "r2t2-native" / backend / "reuse.dll"
                original = library.read_bytes()
                manifest = json.loads(json.dumps(self.manifests[backend]))
                manifest["files"].pop("reuse.dll")
                library.unlink()
                self.write_manifest(backend, manifest)
                try:
                    self.assert_rejected_locally_and_in_archive()
                finally:
                    library.write_bytes(original)
                    self.write_manifest(backend, self.manifests[backend])

    def test_missing_or_stale_reuse_source_fingerprint_in_either_backend_is_rejected(self):
        for backend in ("cpu", "cuda"):
            for fingerprint in (None, "0" * 64):
                with self.subTest(backend=backend, fingerprint=fingerprint):
                    manifest = json.loads(json.dumps(self.manifests[backend]))
                    if fingerprint is None:
                        manifest.pop("reuse_source_sha256")
                    else:
                        manifest["reuse_source_sha256"] = fingerprint
                    self.write_manifest(backend, manifest)
                    try:
                        self.assert_rejected_locally_and_in_archive()
                    finally:
                        self.write_manifest(backend, self.manifests[backend])

    def test_physical_reuse_dll_must_be_listed_in_each_backend_manifest(self):
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                manifest = json.loads(json.dumps(self.manifests[backend]))
                manifest["files"].pop("reuse.dll")
                self.write_manifest(backend, manifest)
                try:
                    self.assert_rejected_locally_and_in_archive()
                finally:
                    self.write_manifest(backend, self.manifests[backend])


if __name__ == "__main__":
    unittest.main()
