"""Exercise native source preparation with real local Git repositories."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import build_r2t2_runtime as builder


class NativeSourceCheckoutTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        attributes = self.root / "empty-attributes"
        attributes.write_bytes(b"")
        config = self.root / "caller.gitconfig"
        config.write_text(
            "[core]\n\tautocrlf = true\n\tsafecrlf = false\n"
            f"\tattributesFile = {attributes.as_posix()}\n"
            "[user]\n\tname = Native source test\n\temail = native-source@example.invalid\n"
            "[commit]\n\tgpgSign = false\n",
            encoding="utf-8",
        )
        environment = {key: value for key, value in os.environ.items()
                       if not key.upper().startswith("GIT_")}
        environment.update({
            "GIT_CONFIG_GLOBAL": str(config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_ATTR_NOSYSTEM": "1",
            "GIT_AUTHOR_DATE": "2000-01-01T00:00:00+00:00",
            "GIT_COMMITTER_DATE": "2000-01-01T00:00:00+00:00",
        })
        self.enterContext(mock.patch.dict(os.environ, environment, clear=True))

        self.upstream = self.root / "upstream"
        self.git(self.root, "init", "--initial-branch=main", self.upstream)
        self.git(self.upstream, "config", "core.autocrlf", "false")
        self.header = Path("external/ggml/include/ggml.h")
        header = self.upstream / self.header
        header.parent.mkdir(parents=True)
        original = (b"// upstream CRLF section\r\n"
                    b"int retained = 1;\r\n"
                    b"\n"
                    b"enum lowering {\n"
                    b"    DEFAULT = 0,\n"
                    b"    EXISTING = 3,\n"
                    b"};\r\n"
                    b"\r\n"
                    b"// model file types\r\n"
                    b"enum type {\r\n"
                    b"    UNKNOWN = -1,\r\n"
                    b"};\r\n")
        self.expected = original.replace(
            b"    EXISTING = 3,\n", b"    EXISTING = 3,\n    EXACT_HEAD = 4,\r\n",
        )
        self.lf_file = Path("pure_lf.h")
        original_lf = b"#pragma once\nint existing = 0;\n"
        self.expected_lf = original_lf + b"int added = 1;\n"
        header.write_bytes(original)
        (self.upstream / self.lf_file).write_bytes(original_lf)
        (self.upstream / "README").write_bytes(b"unchanged upstream file\n")
        self.git(self.upstream, "add", ".")
        self.git(self.upstream, "commit", "-m", "Pinned synthetic upstream")
        self.revision = self.git(self.upstream, "rev-parse", "HEAD").decode().strip()
        self.assertEqual(self.git(self.upstream, "show", f"HEAD:{self.header.as_posix()}"), original)
        self.assertEqual(self.git(self.upstream, "show", f"HEAD:{self.lf_file.as_posix()}"), original_lf)

        header.write_bytes(self.expected)
        (self.upstream / self.lf_file).write_bytes(self.expected_lf)
        self.patch = self.root / "reviewed.patch"
        # Reviewed patches are stored with LF, as required by .gitattributes.
        self.patch.write_bytes(self.git(
            self.upstream, "diff", "--binary", "--unified=4", "HEAD",
        ).replace(b"\r\n", b"\n"))
        header.write_bytes(original)
        (self.upstream / self.lf_file).write_bytes(original_lf)
        self.enterContext(mock.patch.multiple(
            builder, SOURCE_URL=str(self.upstream), SOURCE_REVISION=self.revision, PATCH=self.patch,
        ))

    @staticmethod
    def git(directory, *arguments):
        return subprocess.run(
            ["git", "-C", str(directory), *map(str, arguments)],
            check=True, capture_output=True,
        ).stdout

    def assert_prepared(self, source):
        self.assertEqual(self.git(source, "rev-parse", "HEAD").decode().strip(), self.revision)
        self.assertEqual((source / self.header).read_bytes(), self.expected)
        self.assertEqual((source / self.lf_file).read_bytes(), self.expected_lf)
        self.assertEqual((source / "README").read_bytes(), b"unchanged upstream file\n")
        self.assertEqual(self.git(source, "diff", "--binary", "--unified=4", "HEAD")
                         .replace(b"\r\n", b"\n"), self.patch.read_bytes())

    def test_fresh_checkout_preserves_mixed_endings_with_caller_autocrlf(self):
        self.assertEqual(self.git(self.root, "config", "--get", "core.autocrlf").strip(), b"true")
        source = self.root / "fresh-source"

        builder.prepare_source(source)
        self.assert_prepared(source)
        builder.prepare_source(source)
        self.assert_prepared(source)

    def test_existing_pristine_checkout_preserves_mixed_patch_context(self):
        source = self.root / "pristine-source"
        self.git(self.root, "clone", "-c", "core.autocrlf=false", self.upstream, source)

        builder.prepare_source(source)

        self.assert_prepared(source)

    def test_existing_source_rejects_unreviewed_changes_without_resetting_them(self):
        source = self.root / "existing-source"
        self.git(self.root, "clone", self.upstream, source)
        # Seed a previously accepted checkout from the frozen byte expectations.
        (source / self.header).write_bytes(self.expected)
        (source / self.lf_file).write_bytes(self.expected_lf)
        (source / "README").write_bytes(b"unchanged upstream file\n")
        builder.prepare_source(source)
        self.assert_prepared(source)
        foreign_change = b"user change outside the reviewed patch\n"
        (source / "README").write_bytes(foreign_change)

        with self.assertRaises(RuntimeError):
            builder.prepare_source(source)

        self.assertEqual((source / "README").read_bytes(), foreign_change)
        self.assertEqual((source / self.header).read_bytes(), self.expected)
        self.assertEqual((source / self.lf_file).read_bytes(), self.expected_lf)


if __name__ == "__main__":
    unittest.main()
