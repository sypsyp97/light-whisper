

import importlib.metadata

import contextlib
import importlib.util
import io
import json
import runpy
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

from PyInstaller.utils import hooks


ROOT = Path(__file__).resolve().parents[1]


def load_builder():
    spec = importlib.util.spec_from_file_location("dependency_pruning_builder", ROOT / "scripts/build_engine.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class OnnxHookSelectionTests(unittest.TestCase):
    def setUp(self):
        self.builder = load_builder()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.builder.ENTRY_SCRIPT = self.root / "engine.py"
        self.builder.ENTRY_SCRIPT.write_text("# synthetic entry\n", encoding="utf-8")
        self.builder.DIST_DIR = self.root / "dist"
        self.builder.RESOURCES_DIR = self.root

        with (
            mock.patch.object(self.builder, "validate_r2t2_runtime", return_value=self.root / "r2t2-native"),
            mock.patch.object(importlib.metadata, "version", return_value="0.1.3"),
            mock.patch.object(self.builder.subprocess, "run", return_value=types.SimpleNamespace(returncode=23)) as run,
            contextlib.redirect_stdout(io.StringIO()),
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit) as stopped,
        ):
            self.builder.main()
        self.assertEqual(stopped.exception.code, 23)
        self.command = run.call_args.args[0]
        self.assertEqual(self.command[1:3], ["-m", "PyInstaller"])

    def selected_hook(self):

        directories = []
        for index, argument in enumerate(self.command):
            if argument == "--additional-hooks-dir":
                directories.append(Path(self.command[index + 1]))
            elif argument.startswith("--additional-hooks-dir="):
                directories.append(Path(argument.split("=", 1)[1]))
        for directory in directories:
            candidate = directory / "hook-onnxruntime.py"
            if candidate.is_file():
                return candidate
        contrib = importlib.util.find_spec("_pyinstaller_hooks_contrib")
        return Path(contrib.origin).parent / "stdhooks/hook-onnxruntime.py"

    def collect(self, binaries):
        with mock.patch.object(hooks, "collect_dynamic_libs", return_value=list(binaries)):
            return runpy.run_path(str(self.selected_hook()))["binaries"]

    def test_selected_hook_removes_standalone_core_at_onnx_capi(self):
        standalone = ("/wheel/onnxruntime/capi/onnxruntime.dll", "onnxruntime/capi")
        self.assertNotIn(standalone, self.collect([standalone]),
                         "The build-selected ORT hook still collects the standalone core DLL")

    def test_selected_hook_normalizes_windows_slashes_and_filename_case(self):
        standalone = (r"C:\wheel\onnxruntime\capi\ONNXRUNTIME.DLL", r"onnxruntime\capi")
        self.assertNotIn(standalone, self.collect([standalone]),
                         "The standalone-core filter must handle Windows paths and filename case")

    def test_selected_hook_preserves_pybind_providers_and_other_locations(self):
        retained = [
            ("/wheel/onnxruntime/capi/onnxruntime_pybind11_state.pyd", "onnxruntime/capi"),
            ("/wheel/onnxruntime/capi/onnxruntime_providers_shared.dll", "onnxruntime/capi"),
            ("/wheel/onnxruntime/capi/onnxruntime_providers_future.dll", "onnxruntime/capi"),
            ("/wheel/onnxruntime/capi/another_onnxruntime.dll", "onnxruntime/capi"),
            ("/wheel/onnxruntime/capi/onnxruntime.dll", "."),
            ("/wheel/onnxruntime/capi/onnxruntime.dll", "other/capi"),
            ("/wheel/onnxruntime/capi/onnxruntime.dll", "onnxruntime/capi/plugins"),
            ("/wheel/onnxruntime/capi/onnxruntime.dll", "onnxruntime/capi2"),
        ]
        self.assertCountEqual(self.collect(retained), retained)


class KaldiDuplicateTests(unittest.TestCase):
    def setUp(self):
        self.builder = load_builder()
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.engine = Path(temporary.name) / "engine"
        self.internal = self.engine / "_internal"
        self.local = self.internal / "kaldi_native_fbank/lib/kaldi-native-fbank-core.dll"
        self.common = self.internal / "kaldi-native-fbank-core.dll"
        self.local.parent.mkdir(parents=True)
        self.local.write_bytes(b"exact kaldi native core")

    def files(self):
        return {path.relative_to(self.engine).as_posix(): path.read_bytes()
                for path in self.engine.rglob("*") if path.is_file()}

    def test_identical_kaldi_copy_only_is_removed_and_counted_once(self):
        payload = self.local.read_bytes()
        self.common.write_bytes(payload)

        cuda = self.internal / "r2t2-native/cuda"
        cuda.mkdir(parents=True)
        shared = b"same CUDA runtime"
        (cuda / "cudart64_12.dll").write_bytes(shared)
        (self.internal / "cudart64_12.dll").write_bytes(shared)
        import hashlib
        (cuda / "runtime-manifest.json").write_text(json.dumps({"files": {
            "cudart64_12.dll": {"size": len(shared), "sha256": hashlib.sha256(shared).hexdigest()}
        }}), encoding="utf-8")
        protected = {
            "kaldi_native_fbank/LICENSE": b"kaldi license",
            "kaldi_native_fbank/lib/extra.dll": payload,
            "other/lib/kaldi-native-fbank-core.dll": payload,
            "onnxruntime/capi/onnxruntime_providers_shared.dll": b"provider plugin",
            "other_backend/native.dll": b"unrelated implementation",
            "hf_xet/xet.pyd": b"download transport",
            "r2t2-native/cuda/msvcp140.dll": b"older CRT local",
            "msvcp140.dll": b"newer CRT root!",
        }
        for relative, data in protected.items():
            path = self.internal / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        before = self.files()
        saved = self.builder.share_native_runtime_dlls(self.engine)
        self.assertFalse(self.local.exists(), "byte-identical packaged Kaldi copy remains")
        self.assertEqual(self.common.read_bytes(), payload)
        self.assertEqual(saved, (len(payload) + len(shared)) / (1024 * 1024))
        for relative in protected:
            key = "_internal/" + relative
            self.assertEqual(self.files()[key], before[key], key)
        after = self.files()
        self.assertEqual(self.builder.share_native_runtime_dlls(self.engine), 0.0)
        self.assertEqual(self.files(), after)

    def test_same_size_different_kaldi_bytes_are_preserved(self):
        self.common.write_bytes(b"X" * self.local.stat().st_size)
        before = self.files()
        self.assertEqual(self.builder.share_native_runtime_dlls(self.engine), 0.0)
        self.assertEqual(self.files(), before)

    def test_kaldi_copy_without_root_is_preserved(self):
        before = self.files()
        self.assertEqual(self.builder.share_native_runtime_dlls(self.engine), 0.0)
        self.assertEqual(self.files(), before)


if __name__ == "__main__":
    unittest.main()
