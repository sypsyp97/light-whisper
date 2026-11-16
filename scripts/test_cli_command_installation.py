"""Real Windows PowerShell installer function; only disposable HKCU keys/files."""

import base64
import ntpath
import os
from pathlib import Path
import shutil
import stat
import subprocess
import tempfile
import unittest
import uuid

if os.name == "nt":
    import winreg


SCRIPT = Path(__file__).resolve().parents[1] / "src-tauri/resources/install_cli_command.ps1"


def ps_string(value):
    return "'" + str(value).replace("'", "''") + "'"


def normalized(token):
    return ntpath.normcase(ntpath.normpath(token.strip().strip('"').rstrip("\\/")))


def powershell(code, cwd=None):
    encoded = base64.b64encode(code.encode("utf-16le")).decode("ascii")
    return subprocess.run(["powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive",
                           "-ExecutionPolicy", "Bypass", "-EncodedCommand", encoded],
                          capture_output=True, text=True, errors="replace", timeout=20, cwd=cwd,
                          creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))


@unittest.skipUnless(os.name == "nt" and shutil.which("powershell.exe"), "Windows PowerShell required")
class CommandInstallationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="lw-command-install-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve() / "custom 安装 root"
        self.root.mkdir()
        self.source = self.root / "light-whisper-cli.exe"
        self.source.write_bytes(b"synthetic console executable; byte identity fixture\x00\xff")
        self.desktop = self.root / "light-whisper.exe"
        self.desktop.write_bytes(b"synthetic desktop; preserve name and contents")
        self.bin = self.root / "bin"
        self.alias = self.bin / "light-whisper.exe"
        self.key_name = "Software\\LightWhisperCliCommandTest-" + uuid.uuid4().hex
        key = winreg.CreateKeyEx(winreg.HKEY_CURRENT_USER, self.key_name, access=winreg.KEY_ALL_ACCESS)
        key.Close()
        self.addCleanup(winreg.DeleteKey, winreg.HKEY_CURRENT_USER, self.key_name)

    def set_path(self, value, kind=None):
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, self.key_name, 0, winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, "Path", 0, winreg.REG_EXPAND_SZ if kind is None else kind, value)

    def get_path(self):
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, self.key_name) as key:
            try:
                return winreg.QueryValueEx(key, "Path")
            except FileNotFoundError:
                return None

    def run_installer(self, *, uninstall=False, readonly=False, invoke=True, cwd=None):
        code = "$ErrorActionPreference = 'Stop'\n"
        code += f". {ps_string(SCRIPT)} -InstallRoot {ps_string(self.root)}\n"
        if invoke:
            writable = "$false" if readonly else "$true"
            code += f"$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey({ps_string(self.key_name)}, {writable})\n"
            code += "if ($null -eq $key) { throw 'Owned test key unavailable' }\ntry {\n"
            code += f"Invoke-CliCommandInstall -InstallRoot {ps_string(self.root)} -EnvironmentKey $key"
            code += " -Uninstall" if uninstall else ""
            code += "\n} finally { $key.Dispose() }\n"
        code += "exit 0\n"
        return powershell(code, cwd=cwd)

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)

    def assert_failure(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(result.stderr.strip(), "failure must be visible")

    def own_tokens(self, value):
        return [token for token in value.split(";") if normalized(token) == normalized(str(self.bin))]

    def foreign_tokens(self, value):
        return [token for token in value.split(";") if normalized(token) != normalized(str(self.bin))]

    def assert_original_files(self):
        self.assertEqual(self.desktop.read_bytes(), b"synthetic desktop; preserve name and contents")
        self.assertEqual(self.source.read_bytes(), b"synthetic console executable; byte identity fixture\x00\xff")

    def test_dot_source_only_declares_function(self):
        self.set_path("%TEMP%\\foreign")
        self.assert_success(self.run_installer(invoke=False))
        self.assertEqual(self.get_path(), ("%TEMP%\\foreign", winreg.REG_EXPAND_SZ))
        self.assertFalse(self.bin.exists())

    def test_install_long_expandable_path_and_reinstall_preserve_other_tokens(self):
        original = "%TEMP%\\foreign;;" + ";".join("C:\\foreign-" + str(i) * 30 for i in range(45)) + ";"
        self.assertGreater(len(original), 1024)
        self.set_path(original)
        self.assert_success(self.run_installer())
        self.assertTrue(self.alias.is_file(), "installer did not create console command alias")
        self.assertEqual(self.alias.read_bytes(), self.source.read_bytes())
        installed, kind = self.get_path()
        self.assertEqual(kind, winreg.REG_EXPAND_SZ)
        self.assertEqual(self.foreign_tokens(installed), original.split(";"))
        self.assertEqual(len(self.own_tokens(installed)), 1)
        self.assert_success(self.run_installer())
        self.assertEqual(self.get_path(), (installed, kind))
        self.assert_original_files()

    def test_install_collapses_only_own_case_quote_and_trailing_slash_duplicates(self):
        foreign = ["C:\\first", str(self.bin) + "-other", "", "%UNEXPANDED%\\last"]
        tokens = [foreign[0], '"' + str(self.bin).upper() + '\\"', foreign[1],
                  str(self.bin) + "\\", foreign[2], str(self.bin).lower(), foreign[3]]
        self.set_path(";".join(tokens), winreg.REG_SZ)
        self.assert_success(self.run_installer())
        value, kind = self.get_path()
        self.assertEqual(kind, winreg.REG_SZ)
        self.assertEqual(self.foreign_tokens(value), foreign)
        self.assertEqual(len(self.own_tokens(value)), 1)

    def test_relative_bin_token_remains_foreign_from_install_root(self):
        original = "bin;C:\\foreign"
        with self.subTest(operation="install"):
            self.set_path(original)
            self.assert_success(self.run_installer(cwd=self.root))
            value, kind = self.get_path()
            self.assertEqual(kind, winreg.REG_EXPAND_SZ)
            self.assertEqual(self.foreign_tokens(value), original.split(";"))
            self.assertEqual(len(self.own_tokens(value)), 1)
        with self.subTest(operation="uninstall"):
            self.set_path(original + ";" + str(self.bin))
            self.assert_success(self.run_installer(uninstall=True, cwd=self.root))
            self.assertEqual(self.get_path(), (original, winreg.REG_EXPAND_SZ))
        self.assert_original_files()

    def test_empty_and_absent_path_accept_one_command_entry(self):
        for initial in (None, ""):
            with self.subTest(initial=initial):
                if initial is not None:
                    self.set_path(initial, winreg.REG_SZ)
                self.assert_success(self.run_installer())
                observed = self.get_path()
                self.assertIsNotNone(observed, "installer did not create the command PATH entry")
                value, kind = observed
                self.assertEqual(len(self.own_tokens(value)), 1)
                self.assertEqual(self.foreign_tokens(value), [])
                if initial is not None:
                    self.assertEqual(kind, winreg.REG_SZ)

    def test_missing_cli_fails_before_path_publication(self):
        self.set_path("C:\\foreign")
        self.source.unlink()
        self.assert_failure(self.run_installer())
        self.assertEqual(self.get_path(), ("C:\\foreign", winreg.REG_EXPAND_SZ))
        self.assertFalse(self.alias.exists())

    def test_copy_failure_does_not_publish_path(self):
        self.set_path("C:\\foreign")
        self.alias.mkdir(parents=True)
        self.assert_failure(self.run_installer())
        self.assertEqual(self.get_path(), ("C:\\foreign", winreg.REG_EXPAND_SZ))
        self.assertTrue(self.alias.is_dir())
        self.assert_original_files()

    def test_registry_write_failure_is_visible_and_preserves_foreign_path(self):
        self.set_path("%TEMP%\\foreign", winreg.REG_SZ)
        self.assert_failure(self.run_installer(readonly=True))
        self.assertEqual(self.get_path(), ("%TEMP%\\foreign", winreg.REG_SZ))
        self.assert_original_files()

    def test_install_and_uninstall_refuse_bin_junction_without_touching_target(self):
        foreign = self.root.parent / "foreign junction target"
        foreign.mkdir()
        foreign_alias = foreign / "light-whisper.exe"
        foreign_alias.write_bytes(b"foreign executable; preserve")
        created = powershell("$ErrorActionPreference = 'Stop'\n"
                             f"New-Item -ItemType Junction -Path {ps_string(self.bin)} "
                             f"-Target {ps_string(foreign)} | Out-Null\n")
        self.assert_success(created)

        def unlink_owned_junction():
            if self.bin.exists() and self.bin.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                os.rmdir(self.bin)

        self.addCleanup(unlink_owned_junction)
        self.set_path("C:\\foreign")
        for uninstall in (False, True):
            with self.subTest(uninstall=uninstall):
                self.assert_failure(self.run_installer(uninstall=uninstall))
                self.assertEqual(foreign_alias.read_bytes(), b"foreign executable; preserve")
                self.assertEqual(self.get_path(), ("C:\\foreign", winreg.REG_EXPAND_SZ))

    def test_uninstall_removes_all_own_entries_and_file_preserves_foreign_files_and_data(self):
        self.bin.mkdir()
        self.alias.write_bytes(self.source.read_bytes())
        foreign_file = self.bin / "foreign.txt"
        foreign_file.write_bytes(b"foreign bin file")
        data = self.root / "user-data.json"
        data.write_bytes(b'{"synthetic":"keep"}')
        foreign = ["%TEMP%\\first", str(self.bin) + "-other", "", "C:\\last", ""]
        tokens = [foreign[0], str(self.bin), foreign[1], foreign[2],
                  '"' + str(self.bin).upper() + '\\"', foreign[3], foreign[4]]
        self.set_path(";".join(tokens), winreg.REG_SZ)
        self.assert_success(self.run_installer(uninstall=True))
        self.assertEqual(self.get_path(), (";".join(foreign), winreg.REG_SZ))
        self.assertFalse(self.alias.exists())
        self.assertEqual(foreign_file.read_bytes(), b"foreign bin file")
        self.assertEqual(data.read_bytes(), b'{"synthetic":"keep"}')
        self.assert_success(self.run_installer(uninstall=True))
        self.assertEqual(self.get_path(), (";".join(foreign), winreg.REG_SZ))
        self.assert_original_files()

    def test_uninstall_empty_bin_and_missing_path_is_idempotent(self):
        self.bin.mkdir()
        self.alias.write_bytes(self.source.read_bytes())
        for _ in range(2):
            self.assert_success(self.run_installer(uninstall=True))
            self.assertIsNone(self.get_path())
            self.assertFalse(self.bin.exists(), "empty owned bin directory remains")
            self.assert_original_files()


if __name__ == "__main__":
    unittest.main()
