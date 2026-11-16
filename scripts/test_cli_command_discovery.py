"""Frozen command alias discovers the desktop through the existing main seam."""

import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

from scripts import light_whisper_cli as cli


class CommandDiscoveryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="lw-command-discovery-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.install = self.directory / "custom 安装 root"
        self.alias = self.file(self.install / "bin/light-whisper.exe")
        self.desktop = self.file(self.install / "light-whisper.exe")
        self.resources = self.directory / "frozen-resources"
        self.resources.mkdir()
        (self.resources / "package.json").write_text('{"version":"test-only"}', encoding="utf-8")
        self.launches, self.requests = [], []
        owner = self

        class Broker:
            def __init__(self, command, timeout, env=None):
                self.sequence = 0
                owner.launches.append((command, env))

            def __enter__(self):
                return self

            def __exit__(self, *unused):
                return False

            def request(self, action, **fields):
                self.sequence += 1
                owner.requests.append((action, fields))
                return {"success": True}

        for patch in (
            mock.patch.object(cli, "FROZEN", True),
            mock.patch.object(cli, "ROOT", self.resources),
            mock.patch.object(sys, "executable", str(self.alias)),
            mock.patch.object(cli, "EngineClient", Broker),
            mock.patch.dict(os.environ, {
                "LOCALAPPDATA": str(self.directory / "local-appdata"),
                "LIGHT_WHISPER_DATA_DIR": str(self.directory / "isolated-config"),
            }),
        ):
            patch.start()
            self.addCleanup(patch.stop)

    def file(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        # Both binaries can contain this literal; the protocol marker cannot identify self.
        path.write_bytes(b"synthetic executable; never run; " + cli.AGENT_PROTOCOL)
        return path

    def invoke(self, arguments, records=""):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), mock.patch.object(sys, "stdin", io.StringIO(records)):
            code = cli.main(arguments)
        return code, [json.loads(line) for line in output.getvalue().splitlines()]

    def assert_desktop(self):
        self.assertTrue(self.launches)
        self.assertEqual(self.launches[-1][0], [str(self.desktop.resolve()), "--agent-client"])
        self.assertEqual(self.launches[-1][1]["LIGHT_WHISPER_DATA_DIR"],
                         str(self.directory / "isolated-config"))

    def test_bin_alias_routes_existing_commands_to_parent_desktop(self):
        commands = [(name,) for name in ("status", "stats", "models", "devices")]
        commands += [("engine", name) for name in ("get", "start", "restart")]
        commands.append(("engine", "set", cli.LOCAL_ENGINES[0]))
        for arguments in commands:
            with self.subTest(arguments=arguments):
                code, replies = self.invoke(list(arguments))
                self.assertEqual(code, 0)
                self.assertTrue(replies[0]["success"])
                self.assert_desktop()

    def test_bin_alias_transcribe_preserves_audio_and_options(self):
        audio = self.directory / "测试 audio with spaces.wav"
        audio.write_bytes(b"synthetic fixture; broker never decodes")
        code, _ = self.invoke(["transcribe", str(audio), "--language", "zh", "--context", "文 本",
                               "--hot-word", "词 一", "--hot-word", "word two"])
        self.assertEqual(code, 0)
        self.assert_desktop()
        self.assertEqual(self.requests, [("transcribe", {
            "audio_path": str(audio.resolve()), "options": {"language": "zh", "context": "文 本"},
            "hot_words": ["词 一", "word two"],
        })])

    def test_bin_alias_session_and_doctor_use_parent_desktop(self):
        code, replies = self.invoke(["session"], '{"action":"status","request_id":7}\n'
                                   '{"action":"exit","request_id":8}\n')
        self.assertEqual(code, 0)
        self.assertEqual([reply["request_id"] for reply in replies], [7, 8])
        self.assert_desktop()
        code, replies = self.invoke(["doctor"])
        self.assertEqual(code, 0)
        self.assertEqual(replies[0]["runtime"], str(self.desktop.resolve()))

    def test_missing_local_appdata_keeps_parent_desktop_discovery(self):
        with mock.patch.dict(os.environ):
            os.environ.pop("LOCALAPPDATA", None)
            code, replies = self.invoke(["doctor"])
            self.assertEqual(code, 0)
            self.assertEqual(replies[0]["runtime"], str(self.desktop.resolve()))
            code, replies = self.invoke(["status"])
            self.assertEqual(code, 0)
            self.assertTrue(replies[0]["success"])
            self.assert_desktop()

    def test_legacy_root_cli_keeps_neighbor_discovery_and_explicit_override(self):
        legacy = self.file(self.install / "light-whisper-cli.exe")
        explicit = self.file(self.directory / "explicit desktop.exe")
        with mock.patch.object(sys, "executable", str(legacy)):
            self.assertEqual(cli.app_path(cli.parser().parse_args(["status"])), self.desktop.resolve())
            self.assertEqual(cli.app_path(cli.parser().parse_args(
                ["--app-exe", str(explicit), "status"])), explicit.resolve())

    def test_self_only_reports_missing_desktop_before_launch(self):
        self.desktop.unlink()
        code, replies = self.invoke(["status"])
        self.assertEqual(code, 2)
        self.assertFalse(replies[0]["success"])
        self.assertTrue(replies[0].get("error"))
        self.assertEqual(self.launches, [])

    def test_nonfile_parent_uses_existing_release_fallback(self):
        self.desktop.unlink()
        self.desktop.mkdir()
        earlier_resources = self.directory / "00-frozen-resources"
        release = self.file(earlier_resources / "src-tauri/target/release/light-whisper.exe")
        # A valid candidate can sort before the alias; only its identity matters.
        self.assertLess(release.resolve(), self.alias.resolve())
        with mock.patch.object(cli, "ROOT", earlier_resources):
            self.assertEqual(cli.app_path(cli.parser().parse_args(["status"])), release.resolve())


if __name__ == "__main__":
    unittest.main()
