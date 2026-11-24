"""Verify native CLI identity, source drift, atomic publication and distribution wiring."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

if __package__:
    from .native_package_fixture import executable, pe_image
else:
    from native_package_fixture import executable, pe_image

ROOT = Path(__file__).resolve().parents[1]
BUILD_SCRIPT = ROOT/"scripts/build_cli.py"
FIRST = "3a"*32
SECOND = "17"*32
OLD = b"previous verified executable"


def load_builder():
    spec = importlib.util.spec_from_file_location("cli_builder_under_test", BUILD_SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CliArtifactTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.exe = self.root/"light-whisper-cli.exe"
        self.exe.write_bytes(OLD)
        self.package = self.root/"package.json"
        self.package.write_text('{"version":"1.8.1"}')
        self.builder = load_builder()
        self.builder.OUTPUT_EXE = self.exe
        self.builder.PACKAGE_JSON = self.package
        self.current = FIRST
        self.enterContext(mock.patch.object(self.builder,"source_fingerprint",create=True,
                                           side_effect=lambda component:self.current))

    def test_native_artifact_is_verified_without_a_sidecar(self):
        self.exe.write_bytes(executable("cli",FIRST))
        try:
            report=self.builder.verify_cli()
        except Exception as error:
            self.fail(f"current stamped AMD64 native CLI must verify: {error}")
        self.assertEqual(report["sha256"],hashlib.sha256(self.exe.read_bytes()).hexdigest())
        self.assertEqual(report["size"],self.exe.stat().st_size)
        self.assertEqual(report["version"],"1.8.1")
        self.assertEqual(Path(report["path"]),self.exe)

    def test_rejects_missing_invalid_stale_and_tampered_artifacts(self):
        other=self.root/"target.exe"
        payload=bytearray(pe_image())
        damaged=bytearray(executable("cli",FIRST))
        damaged[512]^=1
        x86=bytearray(payload)
        x86[132:134]=b"\x4c\x01"
        for data in (None,b"",b"bad",pe_image(),executable("cli",SECOND),executable("engine",FIRST),
                     executable("cli",FIRST,payload=b"MZ"),executable("cli",FIRST,payload=x86),
                     executable("cli",FIRST)[:-1],damaged):
            with self.subTest(kind=None if data is None else (len(data),hashlib.sha256(data).hexdigest()[:12])):
                if data is not None:
                    other.write_bytes(data)
                with self.assertRaises(Exception):
                    self.builder.verify_cli(other)
        self.assertEqual(self.exe.read_bytes(),OLD)

    def staged(self, component, staging):
        self.assertEqual(component,"cli")
        self.assertNotEqual(staging,self.exe)
        out=Path(staging)/"light-whisper-cli.exe"
        out.parent.mkdir(parents=True,exist_ok=True)
        out.write_bytes(executable(component,self.current))
        return out

    def build(self,effect):
        with mock.patch.object(self.builder,"build_binary",create=True,side_effect=effect):
            return self.builder.build_cli()

    def test_atomic_publish_reverifies_actual_staged_image(self):
        untouched=self.root/"keep.txt"
        untouched.write_bytes(b"keep")
        self.assertEqual(self.build(self.staged),self.exe)
        self.assertEqual(self.exe.read_bytes(),executable("cli",FIRST))
        self.assertEqual(untouched.read_bytes(),b"keep")

    def test_failed_build_or_bad_stage_preserves_existing_artifact(self):
        def invalid(component,stage):
            path=self.staged(component,stage)
            path.write_bytes(b"broken")
            return path
        for effect in (subprocess.CalledProcessError(23,["cargo","build"]),invalid):
            with self.subTest(case=str(effect)):
                with self.assertRaises(Exception):
                    self.build(effect)
                self.assertEqual(self.exe.read_bytes(),OLD)
                self.assertEqual(set(self.root.iterdir()),{self.exe,self.package})

    def test_changed_inputs_cannot_publish_even_matching_new_stage(self):
        def changed(component,stage):
            self.current=SECOND
            return self.staged(component,stage)
        with self.assertRaises(Exception):
            self.build(changed)
        self.assertEqual(self.exe.read_bytes(),OLD)

    def test_changed_inputs_after_verification_or_replace_error_keep_last_version(self):
        verify=self.builder.verify_cli
        def drift(path=None):
            result=verify(path)
            self.current=SECOND
            return result
        with mock.patch.object(self.builder,"verify_cli",side_effect=drift), self.assertRaises(Exception):
            self.build(self.staged)
        self.assertEqual(self.exe.read_bytes(),OLD)
        self.current=FIRST
        with mock.patch.object(os,"replace",side_effect=PermissionError("locked")),self.assertRaises(Exception):
            self.build(self.staged)
        self.assertEqual(self.exe.read_bytes(),OLD)
        self.assertEqual(set(self.root.iterdir()),{self.exe,self.package})

    def test_cli_status_json_for_build_verify_and_failure(self):
        self.exe.write_bytes(executable("cli",FIRST))
        for argv in ([],["--verify"],["--verify",str(self.exe)]):
            buf=io.StringIO()
            with contextlib.redirect_stdout(buf),mock.patch.object(self.builder,"build_cli",return_value=self.exe):
                self.assertEqual(self.builder.main(argv),0)
            self.assertEqual(json.loads(buf.getvalue())["sha256"],hashlib.sha256(self.exe.read_bytes()).hexdigest())
        self.exe.write_bytes(b"invalid native binary")
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err):
            self.assertNotEqual(self.builder.main(["--verify"]),0)
        self.assertEqual(out.getvalue(),"")
        self.assertTrue(err.getvalue().strip())


class SourceFingerprintTests(unittest.TestCase):
    def test_source_drift_and_line_endings_across_every_runtime_dependency(self):
        import native_runtime_build
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)
            script=root/"scripts/native_runtime_build.py"
            script.parent.mkdir()
            shutil.copy2(native_runtime_build.__file__,script)
            spec=importlib.util.spec_from_file_location("isolated_native_build",script)
            native=importlib.util.module_from_spec(spec)
            spec.loader.exec_module(native)
            common=["rust-runtime/Cargo.toml","rust-runtime/Cargo.lock"]
            engine=["src-tauri/windows-app-manifest.xml","scripts/build_engine.py"]
            cli=["package.json","scripts/build_cli.py"]
            for crate in ("cli","engine","native-asr","reuse","vad"):
                paths=cli if crate=="cli" else engine
                for name in ("Cargo.toml","src/lib.rs","build.rs"):
                    paths.append(f"rust-runtime/crates/{crate}/{name}")
            for path in common+engine+cli:
                dest=root/path
                dest.parent.mkdir(parents=True,exist_ok=True)
                dest.write_bytes(b"baseline\n")
            for component,inputs in (("engine",engine),("cli",cli)):
                initial=native.source_fingerprint(component)
                for path in common+inputs:
                    dest=root/path
                    with self.subTest(component=component,path=path):
                        dest.write_bytes(b"baseline\r\n")
                        self.assertEqual(native.source_fingerprint(component),initial)
                        dest.write_bytes(b"changed\n")
                        self.assertNotEqual(native.source_fingerprint(component),initial)
                        dest.write_bytes(b"baseline\n")

class CliDeliveryWiringTests(unittest.TestCase):
    def test_script_verify_cli_entrypoint_and_unicode_stdout(self):
        from native_runtime_build import source_fingerprint
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/"语音-command.exe"
            path.write_bytes(executable("cli",source_fingerprint("cli")))
            completed=subprocess.run([sys.executable,str(BUILD_SCRIPT),"--verify",str(path)],capture_output=True,
                                     env=dict(os.environ,PYTHONIOENCODING="cp1252:strict"),timeout=20)
            self.assertEqual(completed.returncode,0,completed.stderr)
            self.assertEqual(Path(json.loads(completed.stdout)["path"]),path)

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
