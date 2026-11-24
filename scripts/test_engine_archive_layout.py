import os
import shutil
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import build_engine


class EngineArchiveLayoutTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.engine_dir = self.root / "engine 源"

        self.payloads = {
            "z-equal.bin": b"Z" * 9,
            "_internal/模型.bin": bytes(range(64)),
            "a-equal.bin": b"A" * 9,
            "engine.exe": b"\x00\xffexe" * 7,
            "nested/empty.dat": b"",
            "nested/深层/tiny.txt": b"\x00\xff\x80",
        }
        for name, data in self.payloads.items():
            path = self.engine_dir / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.expected_order = [
            "_internal/模型.bin",
            "engine.exe",
            "a-equal.bin",
            "z-equal.bin",
            "nested/深层/tiny.txt",
            "nested/empty.dat",
        ]

    def assert_round_trip(self, compressor, *arguments):
        output = self.root / "engine.tar.xz"

        discovery = [self.engine_dir / "nested"] + [
            self.engine_dir / name for name in self.payloads
        ]
        with mock.patch.object(Path, "rglob", return_value=iter(discovery)):
            compressor(self.engine_dir, output, *arguments)

        with tarfile.open(output, mode="r:xz") as archive:
            members = archive.getmembers()
            names = [member.name for member in members]
            self.assertTrue(all(member.isfile() for member in members))
            self.assertCountEqual(names, self.payloads)
            contents = {}
            for member in members:
                with archive.extractfile(member) as source:
                    contents[member.name] = source.read()
            self.assertEqual(contents, self.payloads)
            self.assertEqual(names, self.expected_order)

    def test_python_preserves_contents_in_size_then_path_order(self):
        fallback = Path(r"C:\Program Files\7-Zip\7z.exe")
        original = Path.is_file
        def without_seven_zip(path):
            return False if path == fallback else original(path)
        with mock.patch.dict(os.environ, SEVEN_ZIP=""), mock.patch.object(shutil, "which", return_value=None), \
                mock.patch.object(Path, "is_file", without_seven_zip):
            self.assert_round_trip(build_engine.create_tar_xz)

    def test_7z_preserves_contents_in_size_then_path_order(self):
        seven_zip = os.environ.get("SEVEN_ZIP") or shutil.which("7z")
        fallback = Path(r"C:\Program Files\7-Zip\7z.exe")
        if not seven_zip and fallback.is_file():
            seven_zip = str(fallback)
        if seven_zip is None:
            self.skipTest("7-Zip is not installed")
        with mock.patch.dict(os.environ, SEVEN_ZIP=seven_zip):
            self.assert_round_trip(build_engine.create_tar_xz)


if __name__ == "__main__":
    unittest.main()
