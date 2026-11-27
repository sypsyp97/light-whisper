import io
import runpy
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch


class EngineEntrypointTests(unittest.TestCase):
    def test_download_uses_reference_module_in_frozen_host(self):
        entrypoint = Path(__file__).with_name("engine.py")
        engine = "confucius4-r2t2"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            internal = root / "_internal"
            internal.mkdir()
            for location in (root, internal):
                (location / "download_models.py").write_text(
                    "def main(*, engine): print(__file__, engine)\n", encoding="utf-8"
                )
            with (
                patch.object(sys, "path", [str(root), *sys.path]),
                patch.object(sys, "frozen", True, create=True),
                patch.object(sys, "executable", str(root / "host.exe")),
                patch.object(sys, "argv", [str(entrypoint), "download", "--engine", engine]),
                patch.dict(sys.modules),
                redirect_stdout(io.StringIO()) as output,
            ):
                sys.modules.pop("download_models", None)
                runpy.run_path(str(entrypoint), run_name="__main__")
            self.assertEqual(output.getvalue().strip(), f"{root / 'download_models.py'} {engine}")
