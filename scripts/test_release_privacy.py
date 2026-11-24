import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest


CHECKER = Path(__file__).resolve().with_name("check_release_privacy.py")
TOKEN = "gh" + "p_" + "Q" * 36


def nsis_compiler():
    installed = shutil.which("makensis")
    if installed:
        return installed
    candidate = Path(os.environ.get("LOCALAPPDATA", "")) / "tauri" / "NSIS" / "makensis.exe"
    return str(candidate) if candidate.is_file() else None


@unittest.skipUnless(nsis_compiler(), "requires NSIS to build real installer fixtures")
class ReleasePrivacyTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.installer = self.root / "setup.exe"
        self.profile = r"R:\FixtureProfiles\BuildÉditor"
        self.env = os.environ | {"USERPROFILE": self.profile, "HOME": str(self.root / "fixture-home")}

    def build(self, *, binary=b"public desktop", extra_name="notice.txt", extra=b"public",
              engine_name="engine.exe", engine=b"public engine", corrupt_archive=False,
              keep_engine=True, include_cli=True, extra_link=False,
              compressor="/SOLID zlib", detail="Public release", uninstall_detail=None):
        (self.root / "desktop.bin").write_bytes(binary)
        (self.root / "cli.bin").write_bytes(b"public cli")
        (self.root / "extra.bin").write_bytes(extra)
        archive = self.root / "engine.tar.xz"
        with tarfile.open(archive, "w:xz") as tar:
            if keep_engine and engine_name != "engine.exe":
                tar.addfile(tarfile.TarInfo("engine.exe"))
            member = tarfile.TarInfo(engine_name)
            member.size = len(engine)
            tar.addfile(member, io.BytesIO(engine))
            if extra_link:
                symlink = tarfile.TarInfo("link")
                symlink.type = tarfile.SYMTYPE
                symlink.linkname = "engine.exe"
                tar.addfile(symlink)
        if corrupt_archive:
            archive.write_bytes(b"this is not a compressed tar archive")
        script = self.root / "fixture.nsi"
        script.write_text(
            'Name "Release Fixture"\nOutFile "setup.exe"\nRequestExecutionLevel user\n'
            f'SetCompressor {compressor}\nSection\nDetailPrint "{detail}"\nSetOutPath "$INSTDIR"\n'
            'File /oname=light-whisper.exe "desktop.bin"\n'
            + ('File /oname=light-whisper-cli.exe "cli.bin"\n' if include_cli else "") +
            'SetOutPath "$INSTDIR\\resources"\nFile "engine.tar.xz"\n'
            f'File /oname={extra_name} "extra.bin"\n'
            + ('WriteUninstaller "$INSTDIR\\Uninstall.exe"\n' if uninstall_detail is not None else "")
            + 'SectionEnd\n'
            + (f'Section "Uninstall"\nDetailPrint "{uninstall_detail}"\nSectionEnd\n'
               if uninstall_detail is not None else ""), encoding="utf-8")
        subprocess.run([nsis_compiler(), str(script)], cwd=self.root,
                       capture_output=True, check=True, timeout=45)

    def check(self):
        self.assertTrue(CHECKER.is_file(), "release checker entry point missing")
        return subprocess.run([sys.executable, str(CHECKER), str(self.installer)], cwd=self.root,
                              env=self.env, capture_output=True, timeout=60)

    def rejected(self, *redactions):
        result = self.check()
        self.assertNotEqual(result.returncode, 0, "release checker accepted private or corrupt data")
        self.assertNotIn(b"Traceback (most recent call last)", result.stderr)
        for private in redactions:
            self.assertNotIn(private.encode(), result.stdout + result.stderr)

    def test_clean_installer_with_real_engine_archive(self):
        self.build()
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_binary_http_prefix_and_real_wide_url_credentials(self):
        self.build(engine=b"\x7fPE\0http://" + b"\x01\xff\x02" * 80 + b"@\x80\x03")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        password = "synthetic-url-password"
        credential = "https://" + "fixture:" + password + "@example.invalid/"
        self.build(engine=credential.encode("utf-16-le"))
        self.rejected(password)

    def test_compressed_installer_instructions_are_checked(self):
        foreign_path = r"Y:\Users\HeaderOnlyFixture\build"
        self.build(detail="Public installer message")
        self.assertEqual(self.check().returncode, 0)
        for marker in (TOKEN, foreign_path):
            with self.subTest(kind="token" if marker == TOKEN else "path"):
                self.build(detail=marker)
                for encoding in ("utf-8", "utf-16-le", "utf-16-be"):
                    self.assertNotIn(marker.encode(encoding), self.installer.read_bytes())
                self.rejected(marker, "HeaderOnlyFixture")

    def test_unsupported_compression_layouts_fail_closed_even_with_public_data(self):
        for compressor in ("zlib", "/SOLID lzma"):
            with self.subTest(compressor=compressor):
                self.build(compressor=compressor)
                self.rejected()

    def test_compressed_uninstaller_instructions_are_checked(self):
        self.build(compressor="/SOLID zlib", uninstall_detail="Public uninstall message")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.build(compressor="/SOLID zlib", uninstall_detail=TOKEN)
        for encoding in ("utf-8", "utf-16-le", "utf-16-be"):
            self.assertNotIn(TOKEN.encode(encoding), self.installer.read_bytes())
        self.rejected(TOKEN)

    def test_fresh_extraction_after_rebuilding_same_installer(self):
        self.build()
        self.assertEqual(self.check().returncode, 0)
        self.build(extra=TOKEN.encode())
        self.rejected(TOKEN)

    def test_private_path_in_uncompressed_installer_stub(self):
        self.build()
        binary = bytearray(self.installer.read_bytes())
        marker = r"X:\Users\OuterFixture\src"
        start = binary.index(b"This program")
        self.assertLess(start + len(marker), int.from_bytes(binary[60:64], "little"))
        binary[start:start + len(marker)] = marker.encode()
        self.installer.write_bytes(binary)
        self.rejected(marker, "OuterFixture")

    def test_all_members_and_archive_names_are_checked(self):
        for label, kwargs in (("member bytes", {"extra": TOKEN.encode()}),
                              ("member name", {"extra_name": TOKEN}),
                              ("engine bytes", {"engine": TOKEN.encode()}),
                              ("engine name", {"engine_name": f"_internal/{TOKEN}.pyd"})):
            with self.subTest(layer=label):
                self.build(**kwargs)
                self.rejected(TOKEN)

    def test_generic_profiles_and_current_profile_in_binary_encodings(self):
        generic = r"W:\Users\ForeignFixtureOwner\source"
        for name, marker, encoding in (
                ("native", generic, "utf-8"), ("slash", generic.replace("\\", "/"), "utf-8"),
                ("escaped", generic.replace("\\", "\\\\"), "utf-8"),
                ("wide", generic, "utf-16-le"), ("wide big endian", generic, "utf-16-be"),
                ("actual profile", self.profile, "utf-8"), ("actual home", self.env["HOME"], "utf-8")):
            with self.subTest(case=name):
                self.build(engine=b"\0\xff" + marker.encode(encoding) + b"\0")
                self.rejected(marker, "ForeignFixtureOwner", "BuildÉditor")

    def test_corrupt_nested_archive_fails_closed(self):
        self.build(corrupt_archive=True)
        self.rejected()

    def test_malformed_installer_fails_closed(self):
        self.installer.write_bytes(b"MZ incomplete installer")
        self.rejected()

    def test_missing_required_members_and_nonregular_engine_members_fail_closed(self):
        for label, kwargs in (("cli", {"include_cli": False}),
                              ("engine", {"engine_name": "other.exe", "keep_engine": False}),
                              ("symlink", {"extra_link": True})):
            with self.subTest(member=label):
                self.build(**kwargs)
                self.rejected()

    def test_engine_token_spanning_read_boundary(self):
        # TAR's first member data starts after its 512-byte header.
        padding = 4 * 1024 * 1024 - 512 - len(TOKEN) // 2
        self.build(engine=b"." * padding + TOKEN.encode())
        self.rejected(TOKEN)

    def test_missing_installer_argument_fails_cleanly(self):
        result = subprocess.run([sys.executable, str(CHECKER)], cwd=self.root, env=self.env,
                                capture_output=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn(b"Traceback", result.stderr)

    def release_shell(self, names, lines, *, cwd=None):
        release = CHECKER.with_name("release.sh").read_text(encoding="utf-8")
        functions = []
        for name in names:
            definition = name + "() {"
            functions.append(definition + release.split("\n" + definition, 1)[1].split("\n}", 1)[0] + "\n}")
        shell = shutil.which("bash")
        if os.name == "nt":
            git = Path(shutil.which("git"))
            shell = str(git.parents[1] / "bin" / "bash.exe")
        return subprocess.run([shell, "-c", "\n".join(["set -euo pipefail", *functions,
                              'INSTALLER="$1"', *lines]), "release-test", self.installer.as_posix()],
                              cwd=cwd or CHECKER.parent.parent, env=self.env, capture_output=True, timeout=60)

    def test_release_preflight_requires_active_hooks_and_private_identity_check_before_auth(self):
        checkout = self.root / "checkout"
        checkout.mkdir()
        self.env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_ALLOW_PROTOCOL="file")
        def git(*args):
            subprocess.run(["git", *args], cwd=checkout, env=self.env, capture_output=True, check=True)
        git("init", "-q")
        git("config", "user.name", "Release Fixture")
        for name in (".githooks/pre-commit", ".githooks/pre-push", "scripts/git_privacy.py"):
            dest = checkout / name
            dest.parent.mkdir(exist_ok=True)
            shutil.copy2(CHECKER.parent.parent / name, dest)
        (checkout / "tauri.conf.json").write_text("{}", encoding="utf-8")
        for hooks, email, auth_expected in (
                ("inactive", "42+fixture@users.noreply.github.com", False),
                (".githooks", "private-fixture@example.invalid", False),
                (".githooks", "42+fixture@users.noreply.github.com", True)):
            with self.subTest(hooks=hooks, auth_expected=auth_expected):
                git("config", "core.hooksPath", hooks)
                git("config", "user.email", email)
                result = self.release_shell(
                    ("fail", "preflight"),
                    ['TAURI_CONF=tauri.conf.json', 'VERSION=0.0.1', 'ENGINE_MODE=--reuse-engine',
                     'find_seven_zip() { :; }', 'require_command() { :; }',
                     'gh() { echo authentication-called >&2; return 67; }', 'preflight'], cwd=checkout)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(b"authentication-called" in result.stderr, auth_expected)

    def test_release_shell_stops_before_publishing_private_installer(self):
        release = CHECKER.with_name("release.sh").read_text(encoding="utf-8")
        main = release.split("\nmain() {\n", 1)[1].split("\n}\n", 1)[0]
        self.assertLess(main.index("\n    verify_installer\n"), main.index("\n    commit_candidate\n"))
        self.assertLess(main.index("\n    verify_installer\n"), main.index("\n    publish_release "))
        for private in (False, True):
            with self.subTest(private=private):
                self.build(engine=TOKEN.encode() if private else b"public")
                result = self.release_shell(("fail", "find_seven_zip", "verify_installer"),
                                            ["verify_installer", "echo release-continued"])
                self.assertEqual(result.returncode == 0, not private)
                self.assertEqual(b"release-continued" in result.stdout, not private)
                self.assertNotIn(TOKEN.encode(), result.stdout + result.stderr)

    def test_changed_installer_blocks_tag_push_and_upload_after_verification(self):
        # All network and Git effects from publish_release are local shell mocks.
        mocks = '''
git() {
    case "$1" in
        rev-parse) echo fixture-sha ;;
        status|fetch) return 0 ;;
        tag|push) echo "external-effect-$1" ;;
        *) return 97 ;;
    esac
}
gh() { echo external-effect-upload; }
'''
        for replaced in (False, True):
            with self.subTest(replaced=replaced):
                self.build()
                after_wait = 'printf "changed" >> "$INSTALLER"' if replaced else ":"
                result = self.release_shell(("fail", "find_seven_zip", "verify_installer", "publish_release"),
                                            ["verify_installer", after_wait, mocks, "RELEASE_BRANCH=main",
                                             "TAG=v0.0.1", "RELEASE_NOTES=--generate-notes",
                                             "publish_release fixture-sha"])
                self.assertEqual(result.returncode == 0, not replaced, result.stderr)
                for effect in (b"external-effect-tag", b"external-effect-push", b"external-effect-upload"):
                    self.assertEqual(effect in result.stdout, not replaced)

    def test_upload_uses_verified_snapshot_when_original_changes_during_tag_push(self):
        mocks = '''
git() {
    case "$1" in
        rev-parse) echo fixture-sha ;;
        status|fetch|tag) return 0 ;;
        push) printf "changed during tag push" > "$ORIGINAL_INSTALLER" ;;
        *) return 97 ;;
    esac
}
gh() {
    [[ "$1 $2" == "release create" ]]
    cp -- "$4" "$RECORDED_BYTES"
    printf '%s\\n' "$(basename -- "$4")" > "$RECORDED_NAME"
    if command -v cygpath >/dev/null; then cygpath -aw "$4"; else realpath -- "$4"; fi > "$RECORDED_PATH"
}
'''
        self.build()
        expected = self.installer.read_bytes()
        for key, name in (("RECORDED_BYTES", "uploaded.bin"), ("RECORDED_NAME", "uploaded.name"),
                          ("RECORDED_PATH", "uploaded.path")):
            self.env[key] = (self.root / name).as_posix()
        result = self.release_shell(("fail", "find_seven_zip", "verify_installer", "publish_release"),
                                    ['ORIGINAL_INSTALLER="$INSTALLER"', "verify_installer", mocks,
                                     "RELEASE_BRANCH=main", "TAG=v0.0.1", "RELEASE_NOTES=--generate-notes",
                                     "publish_release fixture-sha"])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.installer.read_bytes(), b"changed during tag push")
        self.assertEqual((self.root / "uploaded.name").read_text().strip(), self.installer.name)
        self.assertTrue((self.root / "uploaded.bin").read_bytes() == expected, "gh received changed bytes")
        uploaded = Path((self.root / "uploaded.path").read_text().strip())
        self.assertFalse(uploaded.exists(), "upload snapshot remains after release shell exited")


if __name__ == "__main__":
    unittest.main()
