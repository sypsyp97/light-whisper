

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

    def test_staging_ignores_ambient_legacy_python_packages(self):
        for module in LEGACY:
            path=self.resources/module/"__init__.py"
            path.parent.mkdir()
            path.write_bytes(b"# synthetic retired package")
        stage=self.root/"stage"
        self.module.stage_engine(stage)
        contents={p.relative_to(stage).as_posix() for p in stage.rglob("*")}
        self.assertFalse(any(module in path for path in contents for module in LEGACY))
        self.assertTrue(all((self.resources/module/"__init__.py").exists() for module in LEGACY))

    def test_archive_rejects_all_legacy_packages_and_dist_info_with_windows_normalization(self):
        self.package()
        self.module.verify_archive_sources(self.archive)
        for package in LEGACY:
            for owner in (package, f"{package}-0.1.3.dist-info", f"{package.replace('_', '-')}-0.1.3.dist-info"):
                for name in (f"_internal/{owner}/payload.dat", rf".\_INTERNAL\{owner.upper()}\PAYLOAD.DAT"):
                    with self.subTest(member=name):
                        self.package({**self.members, name: b"obsolete provider"})
                        with self.assertRaises((ValueError,RuntimeError)):
                            self.module.verify_archive_sources(self.archive)


class SharedCudaDependencyTests(unittest.TestCase):
    setUp = cpu_cuda_tests.CpuCudaPackageTests.setUp
    native_members = cpu_cuda_tests.CpuCudaPackageTests.native_members

    def test_cuda_shared_dependencies_have_source_bytes_and_root_manifest_entries(self):
        source=self.resources/"r2t2-native/cuda"
        original={path.name:path.read_bytes() for path in source.iterdir()}
        stage=self.root/"stage"
        self.module.copy_native(stage)
        directory=stage/"_internal/r2t2-native/cuda"
        manifest=json.loads((directory/"runtime-manifest.json").read_bytes())
        for name in ("cublas64_12.dll","cublasLt64_12.dll","cudart64_12.dll"):
            self.assertEqual((stage/name).read_bytes(),original[name])
            self.assertFalse((directory/name).exists())
            self.assertEqual(manifest["shared_files"][name]["sha256"],hashlib.sha256(original[name]).hexdigest())
        self.assertEqual({p.name:p.read_bytes() for p in source.iterdir()},original)


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
