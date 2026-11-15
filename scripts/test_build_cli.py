"""CLI artifact contracts. Synthetic PE/CArchives are not executable-runtime proof."""

import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

from PyInstaller.archive.readers import CArchiveReader
from PyInstaller.archive.writers import CArchiveWriter
import PyInstaller.archive.writers as archive_writers


ROOT = Path(__file__).resolve().parents[1]
BUILD_SCRIPT = ROOT / "scripts/build_cli.py"
ENTRY_NAME = "light_whisper_cli"
OLD_EXE = b"previous verified artifact: preserve byte for byte"
SOURCE = b'''"""Optimization and nested-filename fixture."""
def command(value):
    """Keep this docstring at optimize=0."""
    assert value is not None
    return [item for item in (value,)]
'''


def load_builder():
    spec = importlib.util.spec_from_file_location("cli_builder_under_test", BUILD_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def synthetic_archive(destination, source, package, *, entry=True, metadata=True,
                      optimize="s", private_filename=None):
    """Real CArchiveWriter output behind a synthetic, non-runnable PE header."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as temporary:
        archive = Path(temporary) / "fixture.pkg"
        entries = []
        if entry:
            entries.append((ENTRY_NAME, str(source), True, optimize))
        if metadata:
            entries.append(("package.json", str(package), True, "x"))
        original = archive_writers.replace_filename_in_code_object

        def filenames(code, filename):
            code = original(code, filename)
            if private_filename == "root":
                return code.replace(co_filename=r"C:\private-build\user\light_whisper_cli.py")
            if private_filename == "nested":
                constants = tuple(value.replace(co_filename=r"C:\private-build\nested.py")
                                  if isinstance(value, types.CodeType) else value
                                  for value in code.co_consts)
                return code.replace(co_consts=constants)
            return code

        with mock.patch.object(archive_writers, "replace_filename_in_code_object", filenames):
            CArchiveWriter(str(archive), entries, "python311.dll")
        header = bytearray(132)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 0x3C, 128)
        header[128:132] = b"PE\0\0"
        destination.write_bytes(bytes(header) + archive.read_bytes())
    return destination


class CliArtifactTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cli-package-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "scripts/light_whisper_cli.py"
        self.source.parent.mkdir()
        self.source.write_bytes(SOURCE)
        self.package = self.root / "package.json"
        self.package.write_bytes(b'{"name":"light-whisper","version":"1.8.0"}\n')
        self.output = self.root / "src-tauri/resources/light-whisper-cli.exe"
        self.output.parent.mkdir(parents=True)
        self.module = load_builder()
        self.module.PROJECT_ROOT = self.root
        self.module.ENTRY_SCRIPT = self.source
        self.module.PACKAGE_JSON = self.package
        self.module.OUTPUT_EXE = self.output

    def invoke(self, function, *args):
        try:
            return function(*args)
        except NotImplementedError as error:
            self.fail(f"Declared CLI packaging contract remains unimplemented: {error}")

    def archive(self, path=None, **kwargs):
        return synthetic_archive(path or self.output, self.source, self.package, **kwargs)

    def rejected(self, path=None):
        with self.assertRaises(Exception) as raised:
            self.module.verify_cli(path or self.output)
        self.assertNotIsInstance(raised.exception, NotImplementedError,
                                 "declaration-only failure is not artifact rejection")
        self.assertTrue(str(raised.exception).strip(), "artifact failure must explain its cause")

    def report(self, value, path):
        self.assertIsInstance(value, dict)
        self.assertEqual(value["version"], "1.8.0")
        self.assertEqual(value["sha256"], hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertEqual(value["size"], path.stat().st_size)
        self.assertEqual(Path(value["path"]).resolve(), path.resolve())

    def test_fixture_is_an_actual_readable_carchive(self):
        self.archive()
        reader = CArchiveReader(str(self.output))
        self.assertEqual(reader.extract("package.json"), self.package.read_bytes())
        import marshal
        code = marshal.loads(reader.extract(ENTRY_NAME))
        self.assertIsInstance(code, types.CodeType)
        self.assertEqual(code.co_filename, "light_whisper_cli.py")

    def test_verify_current_archive_without_external_manifest(self):
        self.archive()
        self.report(self.invoke(self.module.verify_cli), self.output)
        self.assertEqual([path.name for path in self.output.parent.iterdir()], [self.output.name])

    def test_verify_explicit_path_reads_that_artifact(self):
        custom = self.archive(self.root / "custom.exe")
        self.output.write_bytes(b"invalid default artifact")
        self.report(self.invoke(self.module.verify_cli, custom), custom)

    def test_missing_empty_non_pe_and_corrupt_archive_are_rejected(self):
        self.rejected()
        for data in (b"", b"not a PE", b"MZ" + b"corrupt archive" * 20):
            with self.subTest(data=data[:20]):
                self.output.write_bytes(data)
                self.rejected()
        self.archive()
        self.output.write_bytes(b"NO" + self.output.read_bytes()[2:])
        self.rejected()

    def test_missing_entry_or_package_are_rejected(self):
        for option in ({"entry": False}, {"metadata": False}):
            with self.subTest(option=option):
                self.archive(**option)
                self.rejected()

    def test_stale_entry_code_is_rejected(self):
        self.archive()
        self.source.write_bytes(SOURCE + b"\nSTALE_CHECK = 1\n")
        self.rejected()

    def test_optimized_entry_is_rejected(self):
        for level in ("s1", "s2"):
            with self.subTest(level=level):
                self.archive(optimize=level)
                self.rejected()

    def test_package_requires_exact_bytes_even_when_version_matches(self):
        self.archive()
        self.package.write_bytes(b'{ "name": "light-whisper", "version": "1.8.0" }\n')
        self.rejected()

    def test_old_packaged_version_is_rejected(self):
        self.archive()
        self.package.write_bytes(b'{"name":"light-whisper","version":"1.8.1"}\n')
        self.rejected()

    def test_root_and_nested_private_code_filenames_are_rejected(self):
        for location in ("root", "nested"):
            with self.subTest(location=location):
                self.archive(private_filename=location)
                self.rejected()

    @staticmethod
    def option(command, flag):
        for index, argument in enumerate(command):
            if argument == flag:
                return command[index + 1]
            if str(argument).startswith(flag + "="):
                return str(argument).split("=", 1)[1]
        raise AssertionError(f"PyInstaller command missing {flag}: {command}")

    def build_boundary(self, *, failure=None, invalid=False, change=None, logs=False):
        self.commands = []

        def run(command, **kwargs):
            command = list(map(str, command))
            self.commands.append(command)
            self.assertEqual(command[:3], [sys.executable, "-m", "PyInstaller"])
            self.assertIn("--onefile", command)
            self.assertIn("--console", command)
            self.assertIn(str(self.source), command)
            self.assertIn(str(self.package), " ".join(command))
            self.assertNotIn("--windowed", command)
            directory = Path(self.option(command, "--distpath")).resolve()
            self.assertTrue(directory.is_relative_to(self.output.parent.resolve()),
                            "build staging belongs beneath resources")
            name = self.option(command, "--name")
            staged = directory / (name if name.endswith(".exe") else name + ".exe")
            self.assertNotEqual(staged, self.output)
            if logs:
                sink = kwargs.get("stdout")
                if sink not in (subprocess.PIPE, subprocess.DEVNULL):
                    print("synthetic PyInstaller build log", file=sink or sys.stdout)
            if failure:
                raise failure
            if change:
                change()
            if invalid:
                staged.parent.mkdir(parents=True, exist_ok=True)
                staged.write_bytes(b"MZ-incomplete")
            else:
                self.archive(staged)
            return subprocess.CompletedProcess(command, 0, stdout="", stderr="")

        return mock.patch("subprocess.run", side_effect=run)

    def prepare_existing(self):
        self.output.write_bytes(OLD_EXE)
        self.unrelated = self.output.parent / "unrelated-build"
        self.unrelated.mkdir()
        (self.unrelated / "keep.txt").write_bytes(b"user-owned unrelated build state")

    def assert_clean(self, *, old=True):
        if old:
            self.assertEqual(self.output.read_bytes(), OLD_EXE)
        self.assertEqual((self.unrelated / "keep.txt").read_bytes(),
                         b"user-owned unrelated build state")
        self.assertEqual({path.name for path in self.output.parent.iterdir()},
                         {self.output.name, self.unrelated.name}, "owned stages must be removed")

    def build_rejected(self):
        with self.assertRaises(Exception) as raised:
            self.module.build_cli()
        self.assertNotIsInstance(raised.exception, NotImplementedError,
                                 "declaration-only failure is not build-failure handling")
        self.assertNotIsInstance(raised.exception, AssertionError,
                                 "fixture/command assertion is not production-failure handling")
        self.assertEqual(len(self.commands), 1, "exercise the actual build subprocess boundary")
        self.assertTrue(str(raised.exception).strip())
        self.assert_clean()

    def test_build_verifies_real_staged_archive_then_atomically_replaces(self):
        self.prepare_existing()
        original_replace = os.replace
        publications = []

        def replace(source, destination):
            self.assertEqual(Path(destination), self.output)
            self.assertEqual(self.output.read_bytes(), OLD_EXE)
            self.report(self.module.verify_cli(Path(source)), Path(source))
            publications.append(Path(source))
            return original_replace(source, destination)

        with self.build_boundary(), mock.patch("os.replace", side_effect=replace):
            self.assertEqual(self.invoke(self.module.build_cli), self.output)
        self.assertEqual(len(self.commands), 1)
        self.assertEqual(len(publications), 1)
        self.report(self.module.verify_cli(), self.output)
        self.assert_clean(old=False)

    def test_subprocess_failure_preserves_old_executable_and_unrelated_paths(self):
        self.prepare_existing()
        with self.build_boundary(failure=subprocess.CalledProcessError(3, "PyInstaller")):
            self.build_rejected()

    def test_incomplete_staged_artifact_preserves_old_executable(self):
        self.prepare_existing()
        with self.build_boundary(invalid=True):
            self.build_rejected()

    def test_entry_changed_during_build_rejects_even_matching_new_archive(self):
        self.prepare_existing()
        with self.build_boundary(change=lambda: self.source.write_bytes(SOURCE + b"CHANGED=1\n")):
            self.build_rejected()

    def test_package_changed_during_build_rejects_even_matching_new_archive(self):
        self.prepare_existing()
        with self.build_boundary(change=lambda: self.package.write_bytes(self.package.read_bytes() + b" \n")):
            self.build_rejected()

    def test_inputs_changed_after_verification_cannot_publish(self):
        self.prepare_existing()
        verify = self.module.verify_cli

        def changed_after_verify(path=None):
            report = verify(path)
            self.source.write_bytes(SOURCE + b"CHANGED_AFTER_VERIFY=1\n")
            return report

        with self.build_boundary(), mock.patch.object(self.module, "verify_cli", changed_after_verify):
            self.build_rejected()

    def test_atomic_replace_failure_preserves_old_executable_and_cleans_stage(self):
        self.prepare_existing()
        with self.build_boundary(), mock.patch("os.replace", side_effect=PermissionError("replace denied")):
            self.build_rejected()

    def test_main_verify_optional_path_and_bare_flag_print_one_json_report(self):
        for explicit in (False, True):
            with self.subTest(explicit=explicit):
                path = self.archive(self.root / "custom.exe" if explicit else None)
                stdout, stderr = io.StringIO(), io.StringIO()
                argv = ["--verify", str(path)] if explicit else ["--verify"]
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    code = self.invoke(self.module.main, argv)
                self.assertEqual(code, 0, stderr.getvalue())
                self.report(json.loads(stdout.getvalue()), path)

    def test_main_default_build_stdout_contains_only_json(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        with self.build_boundary(logs=True), contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = self.invoke(self.module.main, [])
        self.assertEqual(code, 0, stderr.getvalue())
        self.report(json.loads(stdout.getvalue()), self.output)
        self.assertIn("synthetic PyInstaller build log", stderr.getvalue())

    def test_main_failure_nonzero_clear_stderr_and_no_success_json(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = self.invoke(self.module.main, ["--verify"])
        self.assertNotEqual(code, 0)
        self.assertTrue(stderr.getvalue().strip())
        self.assertNotIn("Traceback", stderr.getvalue())
        self.assertEqual(stdout.getvalue(), "")

    def test_readable_archive_with_prefix_below_mz_is_rejected(self):
        self.prepare_existing()
        candidate = self.archive(self.root / "lower-prefix.exe")
        self.report(self.module.verify_cli(candidate), candidate)
        damaged = b"AA" + candidate.read_bytes()[2:]
        self.assertLess(damaged[:2], b"MZ")
        candidate.write_bytes(damaged)
        reader = CArchiveReader(str(candidate))
        self.assertEqual(reader.extract("package.json"), self.package.read_bytes())
        self.assertTrue(reader.extract(ENTRY_NAME))
        self.rejected(candidate)
        self.assertEqual(candidate.read_bytes(), damaged)
        self.assert_clean()

    def test_source_and_package_changes_after_verification_in_either_direction_cannot_publish(self):
        self.prepare_existing()
        package = self.package.read_bytes()
        verify = self.module.verify_cli
        cases = (
            ("source", "decreasing", self.source, b"\n" + SOURCE),
            ("source", "increasing", self.source, SOURCE + b"CHANGED_AFTER_VERIFY=1\n"),
            ("package", "decreasing", self.package, package.replace(b"{", b"{ ", 1)),
            ("package", "increasing", self.package, package + b" \n"),
        )
        for name, direction, target, changed in cases:
            with self.subTest(input=name, direction=direction):
                self.source.write_bytes(SOURCE)
                self.package.write_bytes(package)
                self.output.write_bytes(OLD_EXE)
                before = target.read_bytes()
                if direction == "decreasing":
                    self.assertLess(changed, before)
                else:
                    self.assertGreater(changed, before)
                verified = []

                def changed_after_verify(path=None):
                    report = verify(path)
                    self.report(report, Path(path))
                    verified.append(report)
                    target.write_bytes(changed)
                    return report

                with self.build_boundary(), mock.patch.object(self.module, "verify_cli", changed_after_verify):
                    self.build_rejected()
                self.assertEqual(len(verified), 1, "change inputs only after actual staged verification")
                self.assertEqual(target.read_bytes(), changed)

    def test_higher_root_and_nested_private_code_filenames_are_rejected(self):
        import marshal

        self.prepare_existing()
        private = r"z:\private-build\user\light_whisper_cli.py"
        self.assertGreater(private, "light_whisper_cli.py")
        original = archive_writers.replace_filename_in_code_object
        for location in ("root", "nested"):
            with self.subTest(location=location):
                def filenames(code, filename):
                    code = original(code, filename)
                    if location == "root":
                        return code.replace(co_filename=private)
                    constants = tuple(value.replace(co_filename=private)
                                      if isinstance(value, types.CodeType) else value
                                      for value in code.co_consts)
                    return code.replace(co_consts=constants)

                with mock.patch.object(archive_writers, "replace_filename_in_code_object", filenames):
                    candidate = self.archive(self.root / f"higher-{location}.exe")
                reader = CArchiveReader(str(candidate))
                self.assertEqual(reader.extract("package.json"), self.package.read_bytes())
                code = marshal.loads(reader.extract(ENTRY_NAME))
                exposed = ([code] if location == "root" else
                           [value for value in code.co_consts if isinstance(value, types.CodeType)])
                self.assertTrue(exposed)
                self.assertTrue(all(value.co_filename == private for value in exposed))
                self.rejected(candidate)
                self.assert_clean()

    def test_nonzero_builder_exit_rejects_even_a_valid_staged_archive(self):
        self.prepare_existing()
        with self.build_boundary() as builder:
            build = builder.side_effect

            def failed_after_staging(command, **kwargs):
                build(command, **kwargs)
                staged = Path(self.option(command, "--distpath")) / "light-whisper-cli.exe"
                self.report(self.module.verify_cli(staged), staged)
                completed = subprocess.CompletedProcess(command, 3)
                if kwargs.get("check", False):
                    completed.check_returncode()
                return completed

            builder.side_effect = failed_after_staging
            self.build_rejected()

    def test_verify_unicode_path_can_print_json_to_strict_cp1252_stdout(self):
        self.prepare_existing()
        candidate = self.archive(self.root / "\u4e2d\u6587" / "cli.exe")
        stderr = io.StringIO()
        with io.BytesIO() as raw_stdout:
            with io.TextIOWrapper(raw_stdout, encoding="cp1252", errors="strict", write_through=True) as stdout:
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    code = self.invoke(self.module.main, ["--verify", str(candidate)])
                stdout.flush()
                payload = raw_stdout.getvalue()
        self.assertEqual(code, 0, stderr.getvalue())
        self.assertEqual(stderr.getvalue(), "")
        self.assertEqual(len(payload.splitlines()), 1)
        self.report(json.loads(payload.decode("cp1252", "strict")), candidate)
        self.assert_clean()

    def test_main_archive_failure_preserves_the_missing_member_cause(self):
        self.prepare_existing()
        for missing, options in ((ENTRY_NAME, {"entry": False}), ("package.json", {"metadata": False})):
            with self.subTest(missing=missing):
                candidate = self.archive(self.root / "missing-member.exe", **options)
                archive_bytes = candidate.read_bytes()
                stdout, stderr = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    code = self.invoke(self.module.main, ["--verify", str(candidate)])
                self.assertNotEqual(code, 0)
                self.assertEqual(stdout.getvalue(), "")
                self.assertNotIn("Traceback", stderr.getvalue())
                self.assertIn(missing, stderr.getvalue(), "retain the actual missing archive member cause")
                self.assertEqual(candidate.read_bytes(), archive_bytes)
                self.assert_clean()


class CliDeliveryWiringTests(unittest.TestCase):
    def test_real_script_verify_entrypoint_uses_real_archive_reader(self):
        with tempfile.TemporaryDirectory() as directory:
            artifact = synthetic_archive(Path(directory) / "cli.exe",
                                         ROOT / "scripts/light_whisper_cli.py", ROOT / "package.json")
            result = subprocess.run([sys.executable, str(BUILD_SCRIPT), "--verify", str(artifact)],
                                    capture_output=True, text=True, encoding="utf-8", timeout=15)
            self.assertEqual(result.returncode, 0, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report["version"], json.loads((ROOT / "package.json").read_bytes())["version"])
            self.assertEqual(report["sha256"], hashlib.sha256(artifact.read_bytes()).hexdigest())

    def test_tauri_installs_cli_beside_application(self):
        config = json.loads((ROOT / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))
        self.assertEqual(config["bundle"]["resources"].get("resources/light-whisper-cli.exe"),
                         "light-whisper-cli.exe")

    def test_distribution_verifies_cli_before_frontend_build(self):
        scripts = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["scripts"]
        command = scripts["build:distribution"]
        verify = re.search(r"(?:python|python\.exe)\s+scripts/build_cli\.py\s+--verify\b", command)
        self.assertIsNotNone(verify, "distribution entrypoint must verify the CLI artifact")
        frontend = command.rfind("pnpm build")
        self.assertGreater(frontend, verify.end())
        self.assertIn("&&", command[verify.end():frontend])

    def test_distribution_entrypoint_stops_before_frontend_when_cli_verification_fails(self):
        command = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["scripts"]["build:distribution"]
        bash = (Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
                if os.name == "nt" else shutil.which("bash"))
        self.assertTrue(bash and Path(bash).is_file(), "Git Bash (Windows) or Bash is required")
        harness = '''
set -euo pipefail
node() { printf 'ENGINE_VERIFY\\n'; }
python() { printf 'CLI_VERIFY\\n'; return "$CLI_VERIFY_RESULT"; }
uv() { shift; if [[ "${1:-}" == --no-sync || "${1:-}" == --locked ]]; then shift; fi; "$@"; }
pnpm() { printf 'FRONTEND_BUILD\\n'; }
'''
        for status in (0, 23):
            with self.subTest(status=status):
                result = subprocess.run([str(bash), "-c", harness + command], cwd=ROOT,
                                        env=dict(os.environ, CLI_VERIFY_RESULT=str(status)),
                                        capture_output=True, text=True, encoding="utf-8", timeout=15)
                events = result.stdout.splitlines()
                self.assertIn("CLI_VERIFY", events, "execute the configured distribution command")
                if status:
                    self.assertNotEqual(result.returncode, 0)
                    self.assertNotIn("FRONTEND_BUILD", events)
                else:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertLess(events.index("CLI_VERIFY"), events.index("FRONTEND_BUILD"))

    def test_build_rs_checks_cli_before_tauri_and_tracks_artifact(self):
        source = (ROOT / "src-tauri/build.rs").read_text(encoding="utf-8")
        call = re.search(r"select_cli_executable\s*\(\s*&manifest_dir\s*,\s*allow_placeholder\s*\)", source)
        self.assertIsNotNone(call, "Cargo entrypoint must use the actual tested selector")
        self.assertLess(call.start(), source.index("tauri_build::try_build"))
        self.assertIn("resources/light-whisper-cli.exe", source)

    def test_release_rebuilds_cli_with_reused_engine_before_distribution(self):
        source = (ROOT / "scripts/release.sh").read_text(encoding="utf-8")
        functions = re.findall(r"^([a-z_]+)\(\) \{\n(.*?)^\}", source, re.M | re.S)
        selected = [(name, body) for name, body in functions if "scripts/build_cli.py" in body]
        self.assertTrue(selected, "release must rebuild current CLI even when reusing the engine")
        main = dict(functions)["main"]
        self.assertTrue(any(name == "build_installer" or
                            (re.search(rf"^\s*{name}\s*$", main, re.M) and
                             main.index(name) < main.index("build_installer"))
                            for name, _ in selected))
        for _, body in selected:
            cli = re.search(r"(?:python|python\.exe)\s+scripts/build_cli\.py(?:\s|$)", body)
            self.assertIsNotNone(cli, "release must invoke the public CLI builder")
            self.assertNotIn("--verify", body[cli.start():].splitlines()[0])
            prefix = body[:cli.start()]
            self.assertEqual(len(re.findall(r"\bif\b", prefix)), len(re.findall(r"\bfi\b", prefix)),
                             "CLI rebuild must be outside engine-mode conditionals")


if __name__ == "__main__":
    unittest.main()
