import importlib.util
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock


BUILD_SCRIPT = Path(__file__).with_name("build_engine.py")
OLD_ARCHIVE = b"known-good-engine-archive"


def load_build_engine_module():
    spec = importlib.util.spec_from_file_location("build_engine_under_test", BUILD_SCRIPT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load {BUILD_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class BuildEngineArchiveAtomicityTests(unittest.TestCase):
    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp_dir.cleanup)

        self.root = Path(self.temp_dir.name)
        self.resources_dir = self.root / "resources"
        self.resources_dir.mkdir()
        self.entry_script = self.resources_dir / "engine.py"
        self.entry_script.write_text("print('synthetic engine')\n", encoding="utf-8")
        self.output_archive = self.resources_dir / "engine.tar.xz"
        self.output_archive.write_bytes(OLD_ARCHIVE)
        self.dist_dir = self.resources_dir / "python-dist"
        self.package_dir = self.root / "fake-funasr"
        self.package_dir.mkdir()

        self.module = load_build_engine_module()
        self.module.PROJECT_ROOT = self.root
        self.module.RESOURCES_DIR = self.resources_dir
        self.module.DIST_DIR = self.dist_dir
        self.module.ENTRY_SCRIPT = self.entry_script
        self.module.OUTPUT_ARCHIVE = self.output_archive
        self.module.WINDOWS_MANIFEST = self.root / "missing-manifest.xml"

    def test_pyinstaller_failure_preserves_last_known_good_archive(self):
        failed_process = types.SimpleNamespace(returncode=23)

        with (
            mock.patch.object(self.module, "find_package_dir", return_value=self.package_dir),
            mock.patch.object(self.module.subprocess, "run", return_value=failed_process),
            self.assertRaises(SystemExit) as exit_context,
        ):
            self.module.main()

        self.assertEqual(exit_context.exception.code, 23)
        self.assertTrue(
            self.output_archive.exists(),
            "A failed rebuild must not delete the last known-good engine archive",
        )
        self.assertEqual(self.output_archive.read_bytes(), OLD_ARCHIVE)

    def test_compression_failure_uses_staging_and_preserves_published_archive(self):
        attempted_outputs = []

        def fake_pyinstaller(_cmd):
            engine_dir = self.dist_dir / "engine"
            engine_dir.mkdir(parents=True)
            (engine_dir / "engine.exe").write_bytes(b"synthetic executable")
            return types.SimpleNamespace(returncode=0)

        def fail_compression(_engine_dir, output):
            attempted_outputs.append(Path(output))
            Path(output).write_bytes(b"partial-new-archive")
            raise RuntimeError("synthetic compression failure")

        with (
            mock.patch.object(self.module, "find_package_dir", return_value=self.package_dir),
            mock.patch.object(self.module.subprocess, "run", side_effect=fake_pyinstaller),
            mock.patch.object(self.module, "strip_cuda_dlls", return_value=0.0),
            mock.patch.object(self.module, "strip_dev_artifacts", return_value=0.0),
            mock.patch.object(self.module, "strip_runtime_dirs", return_value=0.0),
            mock.patch.object(self.module, "strip_funasr_bytecode_cache", return_value=0.0),
            mock.patch.object(self.module, "validate_torch_cuda_deps", return_value=None),
            mock.patch.object(self.module, "create_tar_xz", side_effect=fail_compression),
            self.assertRaisesRegex(RuntimeError, "synthetic compression failure"),
        ):
            self.module.main()

        used_staging_path = bool(attempted_outputs) and attempted_outputs[0] != self.output_archive
        preserved_archive = (
            self.output_archive.exists()
            and self.output_archive.read_bytes() == OLD_ARCHIVE
        )
        self.assertTrue(
            used_staging_path and preserved_archive,
            "Compression must target a staging path and atomically replace the published "
            f"archive only after success; attempted={attempted_outputs}, "
            f"published_bytes={self.output_archive.read_bytes() if self.output_archive.exists() else None!r}",
        )


if __name__ == "__main__":
    unittest.main()
