

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
        from native_package_fixture import EngineFixture
        self.fixture=EngineFixture(self,build_engine,self.root)
        self.resources=self.fixture.resources
        self.archive=self.fixture.archive
        self.manifests={b:json.loads((self.resources/"r2t2-native"/b/"runtime-manifest.json").read_bytes())
                        for b in ("cpu","cuda")}

    def write_manifest(self, backend, manifest):
        path = self.resources / "r2t2-native" / backend / "runtime-manifest.json"
        path.write_text(json.dumps(manifest), encoding="utf-8")

    def package(self):
        members=self.fixture.native.copy()
        for backend in ("cpu","cuda"):
            directory=self.resources/"r2t2-native"/backend
            prefix=f"_internal/r2t2-native/{backend}/"
            original=json.loads((directory/"runtime-manifest.json").read_bytes())
            bundled=json.loads(members[prefix+"runtime-manifest.json"])
            bundled={**original,"files":dict(original["files"]),"shared_files":bundled["shared_files"]}
            for name in bundled["shared_files"]:
                bundled["files"].pop(name,None)
            members[prefix+"runtime-manifest.json"]=json.dumps(bundled).encode()
            if not (directory/"reuse.dll").is_file():
                members.pop(prefix+"reuse.dll")
        self.fixture.package(self.fixture.sources|members)

    def assert_rejected_locally_and_in_archive(self):
        with tempfile.TemporaryDirectory(dir=self.root) as tmp:
            with self.subTest(validator="local"),self.assertRaises((ValueError,RuntimeError)):
                build_engine.copy_native(Path(tmp))
        self.package()
        with self.subTest(validator="archive"),self.assertRaises((ValueError,RuntimeError)):
            build_engine.verify_archive_sources(self.archive)

    def test_valid_cpu_cuda_reuse_packs_and_archive_are_accepted(self):
        expected_hash = hashlib.sha256()
        for path in (Path(build_reuse_runtime.ROOT) / "scripts/native/reuse_native.cpp",
                     Path(build_reuse_runtime.__file__)):
            expected_hash.update(path.read_bytes().replace(b"\r\n", b"\n"))
        self.assertEqual(build_reuse_runtime.source_fingerprint(), expected_hash.hexdigest())
        stage=self.root/"stage"
        build_engine.stage_engine(stage)
        self.assertTrue(all((stage/"_internal/r2t2-native"/b/"reuse.dll").is_file() for b in ("cpu","cuda")))
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
