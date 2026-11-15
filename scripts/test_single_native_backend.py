

import hashlib
import importlib.metadata
import json
from pathlib import Path
import re
import shutil
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import test_cpu_cuda_backends as cpu_cuda_tests


ROOT = Path(__file__).resolve().parents[1]
LEGACY = ("transcribe_cpp", "transcribe_cpp_native", "transcribe_cpp_native_cu12")


class SingleNativePackageTests(unittest.TestCase):
    setUp = cpu_cuda_tests.CpuCudaPackageTests.setUp
    native_members = cpu_cuda_tests.CpuCudaPackageTests.native_members
    package = cpu_cuda_tests.CpuCudaPackageTests.package

    @staticmethod
    def flag_values(command, flag):
        return [command[index + 1] for index, value in enumerate(command) if value == flag]

    def test_main_excludes_installed_legacy_packages_before_collect_and_compression(self):
        wheel = self.root / "installed-wheel"
        retained = {"engine.exe": b"synthetic executable", "_internal/current/dependency.dll": b"current dependency"}
        retired = {
            f"_internal/{package}/{name}": b"installed legacy payload"
            for package in LEGACY for name in ("__init__.py", "native.dll", "LICENSE")
        }
        retired.update({f"_internal/{package}-0.1.3.dist-info/METADATA": b"installed metadata" for package in LEGACY})
        for relative, data in {**retained, **retired}.items():
            path = wheel / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        commands, captured = [], {}

        def snapshot(directory):
            return {path.relative_to(directory).as_posix(): path.read_bytes()
                    for path in directory.rglob("*") if path.is_file()}

        original = snapshot(wheel)

        def fake_collect(command):
            commands.append(command)
            self.assertEqual(command[:3], [sys.executable, "-m", "PyInstaller"])
            excluded = set(self.flag_values(command, "--exclude-module"))
            explicit = set(self.flag_values(command, "--collect-all") + self.flag_values(command, "--hidden-import"))
            metadata = {name.replace("-", "_") for name in self.flag_values(command, "--copy-metadata")}
            output = self.module.DIST_DIR / "engine"


            for relative, data in original.items():
                owner = relative.split("/")[1] if relative.startswith("_internal/") else ""
                package = owner.split("-0.1.3.dist-info")[0]
                if package in LEGACY and package in excluded and package not in explicit and package not in metadata:
                    continue
                destination = output / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(data)
            return SimpleNamespace(returncode=0)

        def compression(directory, output):
            captured.update(snapshot(directory))
            output.write_bytes(b"synthetic nonempty archive")
            return 1.0

        with mock.patch.object(self.module.subprocess, "run", side_effect=fake_collect), \
             mock.patch.object(self.module, "create_tar_xz", side_effect=compression):
            self.module.main()
        self.assertEqual(len(commands), 1)
        command = commands[0]
        self.assertTrue(set(LEGACY).issubset(self.flag_values(command, "--exclude-module")))
        for flag in ("--hidden-import", "--collect-all", "--copy-metadata"):
            self.assertFalse(set(LEGACY) & {value.replace("-", "_") for value in self.flag_values(command, flag)}, flag)
        self.assertEqual(captured, retained)
        self.assertEqual(snapshot(wheel), original)

    def test_main_never_installs_legacy_provider_when_distribution_is_absent(self):
        commands = []

        def run(command, **_kwargs):
            commands.append(command)
            return SimpleNamespace(returncode=23 if command[1:3] == ["-m", "PyInstaller"] else 0)

        with mock.patch.object(importlib.metadata, "version", side_effect=[
            importlib.metadata.PackageNotFoundError("transcribe-cpp-native-cu12"), "0.1.3",
        ]), mock.patch.object(self.module.shutil, "which", return_value="synthetic-uv"), \
             mock.patch.object(self.module.subprocess, "run", side_effect=run):
            with self.assertRaises(SystemExit) as stopped:
                self.module.main()
        self.assertEqual(stopped.exception.code, 23)
        self.assertEqual(len(commands), 1, f"packager attempted provider installation: {commands}")
        self.assertEqual(commands[0][:3], [sys.executable, "-m", "PyInstaller"])

    def test_archive_rejects_all_legacy_packages_and_dist_info_with_windows_normalization(self):
        self.package()
        self.module.verify_archive_sources(self.archive)
        for package in LEGACY:
            for owner in (package, f"{package}-0.1.3.dist-info", f"{package.replace('_', '-')}-0.1.3.dist-info"):
                for name in (f"_internal/{owner}/payload.dat", rf".\_INTERNAL\{owner.upper()}\PAYLOAD.DAT"):
                    with self.subTest(member=name):
                        self.package({**self.members, name: b"obsolete provider"})
                        with self.assertRaises(RuntimeError):
                            self.module.verify_archive_sources(self.archive)


class SharedCudaDependencyTests(unittest.TestCase):
    setUp = cpu_cuda_tests.CpuCudaPackageTests.setUp
    native_members = cpu_cuda_tests.CpuCudaPackageTests.native_members

    def test_missing_root_dependencies_stay_local_with_manifest_and_source_unchanged(self):
        source = self.resources / "r2t2-native/cuda"
        manifest = json.loads((source / "runtime-manifest.json").read_text())
        for name in ("cublas64_12.dll", "cublasLt64_12.dll", "cudart64_12.dll"):
            payload = f"current CUDA dependency {name}".encode()
            (source / name).write_bytes(payload)
            manifest["files"][name] = {"size": len(payload), "sha256": hashlib.sha256(payload).hexdigest()}
        (source / "runtime-manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        before = {path.name: path.read_bytes() for path in source.iterdir()}
        engine = self.root / "collected-engine"
        bundled = engine / "_internal/r2t2-native/cuda"
        shutil.copytree(source, bundled)
        self.assertEqual(self.module.share_native_runtime_dlls(engine), 0.0)
        self.assertEqual({path.name: path.read_bytes() for path in bundled.iterdir()}, before)
        self.assertEqual({path.name: path.read_bytes() for path in source.iterdir()}, before)
        self.assertFalse(any((engine / "_internal" / name).exists() for name in manifest["files"]))


class DependencyDeclarationTests(unittest.TestCase):
    def test_declared_and_locked_dependencies_have_no_legacy_transcribe_packages(self):

        project = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        lock = (ROOT / "uv.lock").read_text(encoding="utf-8")
        retired = {name.replace("_", "-") for name in LEGACY}
        requirements = re.findall(r'^\s*"([A-Za-z0-9_.-]+)[^"\n]*",?\s*$', project, re.MULTILINE)
        self.assertFalse(retired & {name.lower().replace("_", "-") for name in requirements})
        locked = re.findall(r'^name = "([^"]+)"$', lock, re.MULTILINE)
        self.assertFalse(retired & {name.lower().replace("_", "-") for name in locked})
        references = re.findall(r'\{ name = "([^"]+)"', lock)
        self.assertFalse(retired & {name.lower().replace("_", "-") for name in references})


if __name__ == "__main__":
    unittest.main()
