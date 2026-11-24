import importlib.metadata
import importlib.util
import hashlib
import io
import json
import tarfile
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock
from native_package_fixture import EngineFixture, executable
import build_r2t2_runtime as native_builder
from build_reuse_runtime import source_fingerprint


BUILD_SCRIPT = Path(__file__).with_name("build_engine.py")
OLD_ARCHIVE = b"known-good-engine-archive"


def load_build_engine_module():
    spec = importlib.util.spec_from_file_location("build_engine_under_test", BUILD_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load {BUILD_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class EngineArchiveSourceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.archive = self.root / "engine.tar.xz"
        self.module = load_build_engine_module()
        self.fixture = EngineFixture(self,self.module,self.root)
        self.archive = self.fixture.archive
        self.native_members = self.fixture.native
        self.sources = self.fixture.sources

    def dependency_names(self):
        return ("_internal/r2t2-native/cpu/ggml-LICENSE.txt",
                "_internal/r2t2-native/cuda/ggml-LICENSE.txt","cudart64_12.dll")

    def changed_manifest(self, field, change, *, backend=None):
        backend = backend or ("cuda" if field == "shared_files" else "cpu")
        name = f"_internal/r2t2-native/{backend}/runtime-manifest.json"
        manifest = json.loads(self.native_members[name])
        manifest[field] = change(manifest[field])
        return {**self.native_members, name: json.dumps(manifest).encode()}

    def package(self, sources, *, native_members=None, extra_members=(), prepend_members=()):
        self.fixture.package(sources | (self.native_members if native_members is None else native_members),
                             extra=extra_members,prepend=prepend_members)

    def test_current_stamped_native_archive_verifies(self):
        self.package(self.sources)
        try:
            self.module.verify_archive_sources(self.archive)
        except Exception as error:
            self.fail(f"current native engine archive must verify: {error}")

    def test_stale_engine_code_or_missing_executable_is_rejected(self):
        for content in (None,executable("engine","00"*32),executable("cli",self.fixture.fingerprint)):
            files=self.sources.copy()
            if content is None:
                del files["engine.exe"]
            else:
                files["engine.exe"]=content
            self.package(files)
            with self.assertRaises(Exception):
                self.module.verify_archive_sources(self.archive)

    def test_changed_checkout_requires_a_rebuilt_engine(self):
        self.package(self.sources)
        self.fixture.fingerprint="00"*32
        with self.assertRaises(Exception):
            self.module.verify_archive_sources(self.archive)

    def test_xz_header_without_an_archive_is_rejected(self):
        self.archive.write_bytes(b"\xfd7zXZ\0")
        with self.assertRaises(Exception):
            self.module.verify_archive_sources(self.archive)

    def test_duplicate_executable_cannot_override_a_verified_image(self):
        image=self.sources["engine.exe"]
        item=tarfile.TarInfo("ENGINE.EXE")
        item.size=len(image)
        self.package(self.sources,extra_members=[(item,image)])
        with self.assertRaises(Exception):
            self.module.verify_archive_sources(self.archive)

    def test_current_native_runtimes_and_shared_files_pass(self):
        self.package(self.sources)
        self.module.verify_archive_sources(self.archive)

    def test_single_case_variant_backend_dependency_passes(self):
        members = self.native_members.copy()
        members["_internal/r2t2-native/CPU/GGML-LICENSE.TXT"] = members.pop(
            "_internal/r2t2-native/cpu/ggml-LICENSE.txt"
        )
        self.package(self.sources, native_members=members)
        self.module.verify_archive_sources(self.archive)

    def test_single_case_variant_shared_dependency_and_root_passes(self):
        members = self.native_members.copy()
        members["CUDART64_12.DLL"] = members.pop("cudart64_12.dll")
        self.package(self.sources, native_members=members)
        self.module.verify_archive_sources(self.archive)

    def test_single_case_variant_native_manifest_passes(self):
        members = self.native_members.copy()
        members["_INTERNAL/R2T2-NATIVE/CPU/RUNTIME-MANIFEST.JSON"] = members.pop(
            "_internal/r2t2-native/cpu/runtime-manifest.json"
        )
        self.package(self.sources, native_members=members)
        self.module.verify_archive_sources(self.archive)

    def test_missing_manifest_dependency_is_rejected(self):
        for name in self.dependency_names():
            with self.subTest(name=name):
                self.package(self.sources, native_members={
                    key: data for key, data in self.native_members.items() if key != name
                })
                with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                    self.module.verify_archive_sources(self.archive)

    def test_modified_manifest_dependency_is_rejected(self):
        for name in self.dependency_names():
            for data in (b"X" * len(self.native_members[name]), self.native_members[name][:-1]):
                with self.subTest(name=name, size=len(data)):
                    self.package(self.sources, native_members={**self.native_members, name: data})
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_duplicate_manifest_dependency_is_rejected(self):
        for name in self.dependency_names():
            with self.subTest(name=name):
                data = self.native_members[name]
                member = tarfile.TarInfo(name)
                member.size = len(data)
                self.package(self.sources, extra_members=[(member, data)])
                with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                    self.module.verify_archive_sources(self.archive)

    def test_nonregular_manifest_dependency_is_rejected(self):
        for name in self.dependency_names():
            for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE):
                with self.subTest(name=name, kind=kind):
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "_internal/r2t2-native/cpu/audiocpp.dll"
                    self.package(self.sources, native_members={
                        key: data for key, data in self.native_members.items() if key != name
                    }, extra_members=[(member, None)])
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_nonregular_duplicate_cannot_follow_a_verified_dependency(self):
        for name in self.dependency_names():
            for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.DIRTYPE):
                with self.subTest(name=name, kind=kind):
                    member = tarfile.TarInfo(name)
                    member.type = kind
                    member.linkname = "_internal/r2t2-native/cpu/audiocpp.dll"
                    self.package(self.sources, extra_members=[(member, None)])
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_directory_duplicate_cannot_precede_a_verified_dependency(self):
        for name in self.dependency_names():
            with self.subTest(name=name):
                member = tarfile.TarInfo(name)
                member.type = tarfile.DIRTYPE
                self.package(self.sources, prepend_members=[(member, None)])
                with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                    self.module.verify_archive_sources(self.archive)

    def test_case_variant_native_dependency_cannot_override_a_verified_file(self):
        for name in self.dependency_names():
            prefix, _, filename = name.rpartition("/")
            aliases = [
                (f"{prefix}/" if prefix else "") + filename.upper(),
                name.replace("_internal/", "_INTERNAL/", 1),
            ]
            for backend in ("cpu", "cuda"):
                if f"/{backend}/" in name:
                    aliases.append(name.replace(f"/{backend}/", f"/{backend.upper()}/", 1))
            for alias in aliases:
                with self.subTest(name=name, alias=alias):
                    source = b"X" * len(self.native_members[name])
                    member = tarfile.TarInfo(alias)
                    member.size = len(source)
                    self.package(self.sources, extra_members=[(member, source)])
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_conflicting_shared_dependency_metadata_is_rejected(self):
        for field, value in (("size", 1), ("sha256", "0" * 64)):
            with self.subTest(field=field):
                members = self.changed_manifest("shared_files", lambda files: {
                    **files, "cudart64_12.dll": {**files["cudart64_12.dll"], field: value},
                })
                self.package(self.sources, native_members=members)
                with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                    self.module.verify_archive_sources(self.archive)

    def test_malformed_manifest_dependency_metadata_is_rejected(self):
        valid = {"size": 1, "sha256": hashlib.sha256(b"x").hexdigest()}
        malformed = (
            None, [], "metadata", {}, {"size": 1}, {"sha256": "a" * 64},
            {**valid, "size": -1}, {**valid, "size": True},
            {**valid, "size": 1.5}, {**valid, "size": "1"},
            {**valid, "sha256": None}, {**valid, "sha256": "a" * 63},
            {**valid, "sha256": "z" * 64},
        )
        for field in ("files", "shared_files"):
            for metadata in malformed:
                with self.subTest(field=field, metadata=metadata):
                    members = self.changed_manifest(field, lambda files: {
                        **files, "dependency.dll": metadata,
                    })


                    prefix = "_internal/r2t2-native/cpu" if field == "files" else "_internal"
                    members[f"{prefix}/dependency.dll"] = b"x"
                    self.package(self.sources, native_members=members)
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_malformed_manifest_file_tables_are_rejected(self):
        for field in ("files", "shared_files"):
            for table in (None, [], "files", 1):
                with self.subTest(field=field, table=table):
                    self.package(self.sources, native_members=self.changed_manifest(field, lambda _: table))
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_manifest_dependency_names_cannot_escape_their_directory(self):
        metadata = {"size": 1, "sha256": hashlib.sha256(b"x").hexdigest()}
        for field in ("files", "shared_files"):
            for name in ("../outside.dll", "/outside.dll", "nested/file.dll", "nested\\file.dll",
                         "C:\\outside.dll", "..", ".", ""):
                with self.subTest(field=field, name=name):
                    members = self.changed_manifest(field, lambda files: {**files, name: metadata})
                    prefix = "_internal/r2t2-native/cpu" if field == "files" else "_internal"
                    members[f"{prefix}/{name}"] = b"x"
                    self.package(self.sources, native_members=members)
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_manifest_dependencies_are_hashed_without_unbounded_reads(self):
        extracted = set()
        extractfile = tarfile.TarFile.extractfile

        def streaming_extractfile(archive, member):
            stream = extractfile(archive, member)
            if not member.name.endswith(".dll"):
                return stream
            extracted.add(member.name)

            class StreamingOnly:
                def __enter__(self):
                    return self

                def __exit__(self, *args):
                    stream.close()

                def read(self, size=-1):
                    self_test.assertGreaterEqual(size, 0, "DLL hashing must use bounded reads")
                    return stream.read(size)

                def readinto(self, buffer):
                    self_test.assertLessEqual(len(buffer), 8*1024*1024)
                    return stream.readinto(buffer)

                def __getattr__(self, name):
                    return getattr(stream, name)

            return StreamingOnly()

        self_test = self
        self.package(self.sources)
        with mock.patch.object(tarfile.TarFile, "extractfile", streaming_extractfile):
            self.module.verify_archive_sources(self.archive)
        self.assertTrue({name for name in self.native_members if name.endswith(".dll")}.issubset(extracted))

    def test_wrong_native_provenance_or_backend_is_rejected(self):
        for backend in ("cpu", "cuda"):
            for field, stale in (
                ("build_id", "light-whisper-r2t2-5"),
                ("revision", "0" * 40),
                ("patch_sha256", "0" * 64),
                ("backend", "cuda" if backend == "cpu" else "cpu"),
            ):
                with self.subTest(backend=backend, field=field):
                    name = f"_internal/r2t2-native/{backend}/runtime-manifest.json"
                    manifest = json.loads(self.native_members[name])
                    manifest[field] = stale
                    self.package(self.sources, native_members={
                        **self.native_members, name: json.dumps(manifest).encode(),
                    })
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_missing_native_manifest_or_library_is_rejected(self):
        for backend in ("cpu", "cuda"):
            for filename in ("runtime-manifest.json", "audiocpp.dll"):
                with self.subTest(backend=backend, filename=filename):
                    missing = f"_internal/r2t2-native/{backend}/{filename}"
                    self.package(self.sources, native_members={
                        name: source for name, source in self.native_members.items() if name != missing
                    })
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_same_size_modified_native_library_is_rejected(self):
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                name = f"_internal/r2t2-native/{backend}/audiocpp.dll"
                self.package(self.sources, native_members={
                    **self.native_members, name: b"X" * len(self.native_members[name]),
                })
                with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                    self.module.verify_archive_sources(self.archive)

    def test_duplicate_native_manifest_or_library_is_rejected(self):
        for backend in ("cpu", "cuda"):
            for filename in ("runtime-manifest.json", "audiocpp.dll"):
                with self.subTest(backend=backend, filename=filename):
                    name = f"_internal/r2t2-native/{backend}/{filename}"
                    source = self.native_members[name]
                    member = tarfile.TarInfo(name)
                    member.size = len(source)
                    self.package(self.sources, extra_members=[(member, source)])
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)

    def test_symlinked_native_manifest_or_library_is_rejected(self):
        for backend in ("cpu", "cuda"):
            for filename in ("runtime-manifest.json", "audiocpp.dll"):
                with self.subTest(backend=backend, filename=filename):
                    name = f"_internal/r2t2-native/{backend}/{filename}"
                    member = tarfile.TarInfo(name)
                    member.type = tarfile.SYMTYPE
                    member.linkname = "../../../outside"
                    self.package(
                        self.sources,
                        native_members={key: source for key, source in self.native_members.items() if key != name},
                        extra_members=[(member, None)],
                    )
                    with self.assertRaises((ValueError, RuntimeError, KeyError, TypeError, AttributeError)):
                        self.module.verify_archive_sources(self.archive)


class BuildEngineArchiveAtomicityTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)

        self.root = Path(self.temp_dir.name)
        self.enterContext(mock.patch.object(native_builder, "ROOT", self.root))

        def fake_reuse_build(backend, _source, _build, output):
            output.mkdir(parents=True, exist_ok=True)
            (output / "reuse.dll").write_bytes(f"synthetic {backend} enhancement library".encode())

        self.enterContext(mock.patch("build_reuse_runtime.build", side_effect=fake_reuse_build))
        self.module=load_build_engine_module()
        self.fixture=EngineFixture(self,self.module,self.root)
        self.resources_dir=self.fixture.resources
        self.output_archive=self.resources_dir/"engine.tar.xz"
        self.output_archive.write_bytes(OLD_ARCHIVE)
        self.module.OUTPUT_ARCHIVE=self.output_archive
        self.dist_dir=self.root/"previous-build"

    def test_stale_native_runtime_is_rejected_before_destroying_old_build(self):
        self.dist_dir.mkdir()
        sentinel = self.dist_dir / "keep.txt"
        sentinel.write_text("previous build")
        manifest_path = self.resources_dir / "r2t2-native/cpu/runtime-manifest.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["patch_sha256"] = "old patch"
        manifest_path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex((RuntimeError,ValueError), "Stale|invalid"):
            self.module.main()
        self.assertTrue(sentinel.is_file())
        self.assertEqual(self.output_archive.read_bytes(), OLD_ARCHIVE)

    def test_modified_native_library_is_rejected(self):
        library = self.resources_dir / "r2t2-native/cuda/audiocpp.dll"
        library.write_bytes(b"X" * library.stat().st_size)
        with self.assertRaisesRegex((RuntimeError,ValueError), "checksum"):
            self.module.copy_native(self.root/"staged")

    def test_unlisted_native_file_is_rejected(self):
        (self.resources_dir / "r2t2-native/cpu/stale.dll").write_bytes(b"old")
        with self.assertRaisesRegex((RuntimeError,ValueError), "Unexpected"):
            self.module.copy_native(self.root/"staged")

    def _native_build_fixture(self):
        source, build, redist = (self.root / name for name in ("source", "build", "redist"))
        for relative in ("LICENSE", "external/ggml/LICENSE", "external/sentencepiece/LICENSE"):
            path = source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("synthetic license")
        (build / "bin").mkdir(parents=True)
        (build / "bin/audiocpp.dll").write_bytes(b"new native library")
        for name in ("msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"):
            path = redist / "x64/CRT" / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic runtime")
        return source, build, {"VCToolsRedistDir": str(redist)}

    def test_native_rebuild_does_not_absorb_old_package_files(self):
        source, build, env = self._native_build_fixture()
        output = self.resources_dir / "r2t2-native/cpu"
        (output / "stale.dll").write_bytes(b"old")
        native_builder.copy_runtime(source, build, output, "cpu", None, env, "")
        self.assertFalse((output / "stale.dll").exists())
        manifest = json.loads((output / "runtime-manifest.json").read_text())
        self.assertEqual(set(manifest["files"]), {p.name for p in output.iterdir()} - {"runtime-manifest.json"})
        self.assertEqual((output / "audiocpp.dll").read_bytes(), b"new native library")

    def test_native_assembly_failure_preserves_previous_package(self):
        source, build, env = self._native_build_fixture()
        (source / "LICENSE").unlink()
        output = self.resources_dir / "r2t2-native/cpu"
        previous = {p.name: p.read_bytes() for p in output.iterdir()}
        with self.assertRaises(FileNotFoundError):
            native_builder.copy_runtime(source, build, output, "cpu", None, env, "")
        self.assertEqual({p.name: p.read_bytes() for p in output.iterdir()}, previous)

    def test_native_publish_failure_restores_previous_package(self):
        source, build, env = self._native_build_fixture()
        output = self.resources_dir / "r2t2-native/cpu"
        previous = {p.name: p.read_bytes() for p in output.iterdir()}
        rename = Path.rename
        def fail_publish(path, target):
            if path.name == "new":
                raise PermissionError("synthetic publish failure")
            return rename(path, target)
        with mock.patch.object(Path, "rename", fail_publish):
            with self.assertRaisesRegex(PermissionError, "synthetic publish failure"):
                native_builder.copy_runtime(source, build, output, "cpu", None, env, "")
        self.assertEqual({p.name: p.read_bytes() for p in output.iterdir()}, previous)

    def test_native_publish_refuses_unmanaged_output_directory(self):
        source, build, env = self._native_build_fixture()
        output = self.root / "unmanaged"
        output.mkdir()
        (output / "keep.txt").write_text("user file")
        with self.assertRaisesRegex(RuntimeError, "unmanaged"):
            native_builder.copy_runtime(source, build, output, "cpu", None, env, "")
        self.assertEqual((output / "keep.txt").read_text(), "user file")

    def test_native_failed_rollback_keeps_recoverable_backup(self):
        source, build, env = self._native_build_fixture()
        output = self.resources_dir / "r2t2-native/cpu"
        previous = {p.name: p.read_bytes() for p in output.iterdir()}
        rename = Path.rename
        def fail_publish_and_restore(path, target):
            if path.name in ("new", "previous"):
                raise PermissionError("synthetic locked target")
            return rename(path, target)
        with mock.patch.object(Path, "rename", fail_publish_and_restore):
            with self.assertRaisesRegex(RuntimeError, "Previous native package retained"):
                native_builder.copy_runtime(source, build, output, "cpu", None, env, "")
        backups = list(output.parent.glob(".r2t2-runtime-*/previous"))
        self.assertEqual(len(backups), 1)
        self.assertEqual({p.name: p.read_bytes() for p in backups[0].iterdir()}, previous)

    def test_locked_destination_preserves_prior_archive_and_cleans_staging(self):
        with mock.patch.object(self.module.os,"replace",side_effect=PermissionError("locked")):
            with self.assertRaises(PermissionError):
                self.module.main()
        self.assertEqual(self.output_archive.read_bytes(),OLD_ARCHIVE)
        self.assertFalse(list(self.resources_dir.glob(".engine-build-*")))

    def test_compiler_failure_preserves_last_known_good_archive(self):
        with mock.patch.object(self.module,"stage_engine",create=True,side_effect=RuntimeError("compiler failed")):
            with self.assertRaises(Exception):
                self.module.main()
        self.assertEqual(self.output_archive.read_bytes(),OLD_ARCHIVE)

    def test_compression_failure_uses_staging_and_preserves_published_archive(self):
        attempted=[]
        def fail(engine_dir,output):
            attempted.append(Path(output))
            output.write_bytes(b"unfinished")
            raise RuntimeError("compression failed")
        with mock.patch.object(self.module,"stage_engine",create=True),mock.patch.object(self.module,"create_tar_xz",side_effect=fail):
            with self.assertRaisesRegex(RuntimeError,"compression failed"):
                self.module.main()
        self.assertEqual(len(attempted),1)
        self.assertNotEqual(attempted[0],self.output_archive)
        self.assertFalse(attempted[0].exists())
        self.assertEqual(self.output_archive.read_bytes(),OLD_ARCHIVE)


if __name__=="__main__":
    unittest.main()
