"""App/engine routing through main; native broker/server remains a separate contract."""

import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import test_cli_session as sessions
from scripts import test_light_whisper_cli as support


QWEN = "qwen3-asr-0.6b"


def setUpModule():
    support.setUpModule()


class TargetTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.script.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        self.config_dir = Path(os.environ["LIGHT_WHISPER_DATA_DIR"]).resolve()
        self.repository = self.directory / "synthetic-repository"
        self.repository.mkdir()
        (self.repository / "package.json").write_text('{"version":"test-only"}', encoding="utf-8")
        self.local_appdata = self.directory / "local-appdata"
        self.app = self.directory / "显式 app.exe"
        self.app.write_bytes(b"synthetic executable; never executed; LIGHT_WHISPER_AGENT_PROTOCOL_1")
        self.audio = self.directory / "synthetic 音频.wav"
        self.audio.write_bytes(b"synthetic audio; fake broker never decodes")
        for patch in (
            mock.patch.object(support.cli, "ROOT", self.repository),
            mock.patch.object(support.cli, "DEFAULT_PYTHON", Path(support.BACKEND_PYTHON)),
            mock.patch.dict(os.environ, {"LOCALAPPDATA": str(self.local_appdata)}),
        ):
            patch.start()
            self.addCleanup(patch.stop)
        self.launches, self.clients = [], []

    def invoke(self, arguments, records=None):
        output, errors = io.StringIO(), io.StringIO()
        source = "" if records is None else "".join(json.dumps(record) + "\n" for record in records)
        with mock.patch.object(sys, "stdin", io.StringIO(source)), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = support.cli.main(arguments)
        self.assertIs(type(code), int)
        responses = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertTrue(all(isinstance(value, dict) and type(value.get("success")) is bool
                            for value in responses), output.getvalue())
        return code, responses, errors.getvalue()

    def single(self, arguments):
        code, replies, errors = self.invoke(arguments)
        self.assertEqual(len(replies), 1, (replies, errors))
        return code, replies[0]

    def launch_fake(self, mode="normal"):
        original = support.cli.EngineClient
        self.launches, self.clients = [], []

        def launch(command, timeout, env=None):
            self.launches.append((list(command), timeout, dict(env or {})))
            client = original([support.BACKEND_PYTHON, str(self.script)], timeout=timeout,
                              env={**(env or {}), "CLI_TEST_MODE": mode,
                                   "CLI_TEST_RECORD": str(self.record)})
            self.clients.append(client)
            return client

        return mock.patch.object(support.cli, "EngineClient", side_effect=launch)

    def assert_closed(self):
        self.assertEqual(len(self.clients), 1)
        client = self.clients[0]
        self.assert_reaped(client)
        self.assertTrue(client.child.stdin.closed)
        self.assertTrue(client.child.stdout.closed)
        for worker in (client.reader, client.sender):
            if worker is not None:
                self.assertFalse(worker.is_alive())

    def requests(self):
        return [event["request"] for event in self.recorded() if event["event"] == "request"]

    def candidate(self, kind):
        return {
            "release": self.repository / "src-tauri/target/release/light-whisper.exe",
            "debug": self.repository / "src-tauri/target/debug/light-whisper.exe",
            "installed": self.local_appdata / "轻语 Whisper/light-whisper.exe",
        }[kind]

    def create_candidate(self, kind):
        path = self.candidate(kind)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"synthetic executable; never executed; LIGHT_WHISPER_AGENT_PROTOCOL_1")
        return path

    def test_default_and_explicit_target_selection_are_reported_by_main(self):
        cases = [([], "app"), (["--target", "app"], "app"),
                 (["--target", "engine"], "engine"),
                 (["--app-exe", str(self.app)], "app"),
                 (["--engine", QWEN], "engine"),
                 (["--python", support.BACKEND_PYTHON], "engine"),
                 (["--engine-exe", support.BACKEND_PYTHON], "engine"),
                 (["--data-dir", str(self.directory / "runtime")], "engine"),
                 (["--threads", "7", "--timeout", ".4"], "app")]
        for arguments, expected in cases:
            with self.subTest(arguments=arguments), \
                 mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
                code, result = self.single([*arguments, "doctor"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(result.get("target"), expected)
                parsed = support.cli.parser().parse_args([*arguments, "doctor"])
                self.assertEqual(support.cli.resolve_target(parsed), expected)

    def test_incompatible_targets_fail_before_starting_any_child(self):
        independent = [["--engine", QWEN], ["--python", support.BACKEND_PYTHON],
                       ["--engine-exe", support.BACKEND_PYTHON],
                       ["--data-dir", str(self.directory / "runtime")]]
        cases = [["--target", "app", *flags] for flags in independent]
        cases += [["--app-exe", str(self.app), *flags] for flags in independent]
        cases += [["--target", "engine", "--app-exe", str(self.app)]]
        for arguments in cases:
            for action in ("doctor", "status", "session"):
                with self.subTest(arguments=arguments, action=action), \
                     mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as child:
                    code, result = self.single([*arguments, action])
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    self.assertTrue(result.get("error"))
                    child.assert_not_called()
                    parsed = support.cli.parser().parse_args([*arguments, action])
                    with self.assertRaises(ValueError):
                        support.cli.resolve_target(parsed)

    def test_doctor_reports_target_surface_without_loading_or_writing(self):
        missing = self.directory / "missing app.exe"
        surfaces = (("app", "running desktop instance"),
                    ("engine", "separate local engine process; not the running desktop instance"))
        for target, surface in surfaces:
            arguments = ["--target", target]
            if target == "app":
                arguments += ["--app-exe", str(missing)]
            with self.subTest(target=target), \
                 mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")), \
                 mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("temporary data")), \
                 mock.patch.object(Path, "mkdir", side_effect=AssertionError("mkdir")):
                code, result = self.single([*arguments, "doctor"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(result.get("target"), target)
            self.assertEqual(result["surface"], surface)
            self.assertIs(result["model_load_checked"], False)
            self.assertFalse(self.config_dir.exists())

    def test_explicit_app_uses_broker_command_and_preserves_inherited_environment(self):
        inherited = {key: str(index + 17) for index, key in enumerate(support.ENV_KEYS[:3])}
        inherited.update(HF_HUB_OFFLINE="0", TRANSFORMERS_OFFLINE="0", CLI_TARGET_TEST="inherited")
        unresolved = self.config_dir / ".." / self.config_dir.name
        unresolved_app = self.app.parent / ".." / self.app.parent.name / self.app.name
        inherited["LIGHT_WHISPER_DATA_DIR"] = str(unresolved)
        with mock.patch.dict(os.environ, inherited), self.launch_fake("noise"), \
             mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("temporary data")):
            code, result = self.single(["--target", "app", "--app-exe", str(unresolved_app),
                                        "--threads", "7", "--timeout", ".4", "status"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertEqual(result["request"]["action"], "status")
        self.assertNotIn("wrong", result, "stale/future broker replies escaped correlation")
        command, timeout, environment = self.launches[0]
        self.assertEqual(command, [str(self.app.resolve()), "--agent-client"])
        self.assertEqual(timeout, .4)
        self.assertEqual(environment["LIGHT_WHISPER_DATA_DIR"], str(self.config_dir))
        for key, value in inherited.items():
            if key != "LIGHT_WHISPER_DATA_DIR":
                self.assertEqual(environment.get(key), value)
        self.assertFalse(self.config_dir.exists(), "Python broker path created desktop data")
        self.assert_closed()
        parsed = support.cli.resolve_engine_selection(support.cli.parser().parse_args(
            ["--target", "app", "--app-exe", str(unresolved_app), "status"]))
        self.assertEqual(support.cli.app_command(parsed), command)

    def test_app_discovery_prefers_release_then_debug_then_installed(self):
        installed = self.create_candidate("installed")
        for kind in ("installed", "debug", "release"):
            expected = installed if kind == "installed" else self.create_candidate(kind)
            with self.subTest(kind=kind), self.launch_fake():
                code, result = self.single(["--target", "app", "status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], [str(expected.resolve()), "--agent-client"])
                self.assert_closed()

    def test_explicit_app_path_wins_and_auto_discovery_skips_directories(self):
        release = self.candidate("release")
        release.mkdir(parents=True)
        debug = self.create_candidate("debug")
        self.create_candidate("installed")
        for flags, expected in (([], debug), (["--app-exe", str(self.app)], self.app)):
            with self.subTest(flags=flags), self.launch_fake():
                code, result = self.single(["--target", "app", *flags, "status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], [str(expected.resolve()), "--agent-client"])
                self.assert_closed()

    def test_missing_or_nonfile_app_is_configuration_error_before_spawn(self):
        for flags in ([], ["--app-exe", str(self.directory / "missing.exe")],
                      ["--app-exe", str(self.directory)]):
            if flags:
                self.create_candidate("release")
            with self.subTest(flags=flags), \
                 mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as child:
                code, result = self.single(["--target", "app", *flags, "status"])
                self.assertEqual(code, 2)
                self.assertFalse(result["success"])
                self.assertTrue(result.get("error"))
                child.assert_not_called()

    def test_default_target_reaches_app_and_saved_cloud_selection_is_allowed(self):
        expected = self.create_candidate("release")
        self.config_dir.mkdir()
        config = self.config_dir / "engine.json"
        for engine in (QWEN, "glm-asr", "alibaba-asr"):
            config.write_text(json.dumps({"engine": engine}), encoding="utf-8")
            before = config.read_bytes()
            with self.subTest(engine=engine), self.launch_fake():
                code, result = self.single(["status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], [str(expected.resolve()), "--agent-client"])
                self.assertEqual(config.read_bytes(), before)
                self.assert_closed()

    def test_app_transcribe_forwards_existing_request_fields(self):
        with self.launch_fake():
            code, result = self.single(["--target", "app", "--app-exe", str(self.app),
                                        "transcribe", str(self.audio), "--language", "zh",
                                        "--context", "测试 context"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertEqual(result["request"], {
            "action": "transcribe", "request_id": 1,
            "audio_path": str(self.audio.resolve()),
            "options": {"language": "zh", "context": "测试 context"},
        })
        self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
        self.assert_closed()

    def test_app_session_reuses_one_broker_and_closes_only_that_broker(self):
        desktop = subprocess.Popen([support.BACKEND_PYTHON, "-c", "import sys; sys.stdin.read()"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        records = [{"request_id": 71, "action": "transcribe", "audio": str(self.audio),
                    "hot_words": ["术语"]}, {"request_id": 72, "action": "stats"}]
        for ending in ([], [{"request_id": 73, "action": "exit"}]):
            self.record.unlink(missing_ok=True)
            self.launches, self.clients = [], []
            with self.subTest(ending=ending), self.launch_fake():
                code, replies, _ = self.invoke(["--target", "app", "--app-exe", str(self.app),
                                                "--timeout", ".4", "session"], records + ending)
                self.assertEqual(code, 0)
                self.assertEqual([reply["request_id"] for reply in replies], [71, 72] + ([73] if ending else []))
                self.assertTrue(all(reply["success"] for reply in replies))
                self.assertEqual(replies[1]["stats"]["transcription_count"], 1)
                self.assertEqual(len(self.launches), 1)
                self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
                self.assertEqual([request["action"] for request in self.requests()], ["transcribe", "stats", "exit"])
                self.assertEqual(self.requests()[0]["hot_words"], ["术语"])
                self.assert_closed()
                self.assertIsNone(desktop.poll(), "CLI cleanup terminated the separate synthetic desktop")

    def test_empty_or_exit_only_app_session_does_not_start_broker(self):
        for records in ([], [{"request_id": 81, "action": "exit"}]):
            with self.subTest(records=records), \
                 mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as child:
                code, replies, _ = self.invoke(["--target", "app", "--app-exe", str(self.app), "session"], records)
                self.assertEqual(code, 0)
                self.assertEqual(replies, [] if not records else [{"success": True, "request_id": 81}])
                child.assert_not_called()

    def test_app_startup_failure_is_bounded_and_reaps_broker(self):
        with self.launch_fake("startup_false"):
            code, result = self.single(["--target", "app", "--app-exe", str(self.app),
                                        "--timeout", ".4", "status"])
        self.assertEqual(code, 1)
        self.assertFalse(result["success"])
        self.assertTrue(result.get("error"))
        self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
        self.assertEqual(self.launches[0][1], .4)
        self.assert_closed()

    def test_explicit_engine_keeps_cpu_offline_and_temporary_data_contract(self):
        inherited = {key: "19" for key in support.ENV_KEYS[:3]}
        inherited.update(HF_HUB_OFFLINE="0", TRANSFORMERS_OFFLINE="0")
        with mock.patch.dict(os.environ, inherited), self.launch_fake():
            code, result = self.single(["--target", "engine", "--python", support.BACKEND_PYTHON, "status"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        command, _, environment = self.launches[0]
        self.assertEqual(command[-3:], ["serve", "--engine", QWEN])
        self.assertNotIn("--agent-client", command)
        for key in support.ENV_KEYS[:3]:
            self.assertEqual(environment[key], "2")
        for key in support.ENV_KEYS[3:5]:
            self.assertEqual(environment[key], "1")
        data = Path(environment["LIGHT_WHISPER_DATA_DIR"])
        self.assertNotEqual(data, self.config_dir)
        self.assertFalse(data.exists())
        self.assert_closed()


if __name__ == "__main__":
    unittest.main()
