"""Desktop engine selection through the public CLI, with isolated user state."""

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

from scripts import test_light_whisper_cli as support


QWEN = "qwen3-asr-0.6b"
R2T2 = "confucius4-r2t2"
SECRET = "TEST_ONLY_CONFIG_TOKEN_DO_NOT_PRINT_7f246"


def setUpModule():
    support.setUpModule()


class ConfigurationTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.config_dir = Path(os.environ["LIGHT_WHISPER_DATA_DIR"])
        self.audio = self.directory / "synthetic audio.wav"
        self.audio.write_bytes(b"test-only audio; fake backend does not decode")

    def save_config(self, value, directory=None):
        directory = self.config_dir if directory is None else directory
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / "engine.json"
        path.write_text(json.dumps(value), encoding="utf-8")
        return path

    def invoke(self, arguments):
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = support.cli.main(arguments)
        self.assertIs(type(code), int)
        self.assertEqual(len(output.getvalue().splitlines()), 1,
                         (output.getvalue(), errors.getvalue()))
        result = json.loads(output.getvalue())
        self.assertIsInstance(result, dict)
        self.assertIs(type(result.get("success")), bool)
        self.assertNotIn(SECRET, output.getvalue() + errors.getvalue())
        return code, result

    def command(self, action):
        return [action, str(self.audio)] if action == "transcribe" else [action]

    def launch_fake(self):
        original = support.cli.EngineClient
        self.launches, self.clients = [], []

        def launch(command, timeout, env=None):
            self.launches.append((list(command), timeout, dict(env or {})))
            client = original([support.BACKEND_PYTHON, str(self.script)], timeout=timeout,
                              env={**(env or {}), "CLI_TEST_MODE": "normal",
                                   "CLI_TEST_RECORD": str(self.record)})
            self.clients.append(client)
            return client

        return mock.patch.object(support.cli, "EngineClient", side_effect=launch)

    def assert_doctor_selection(self, engine, source, local=True, directory=None):
        with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")), \
             mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("write")):
            code, result = self.invoke(["doctor"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertEqual(result["engine"], engine)
        self.assertEqual(result.get("engine_source"), source)
        expected_dir = self.config_dir if directory is None else directory
        self.assertEqual(result.get("config_dir"), str(expected_dir.resolve()))
        self.assertIs(result.get("local_engine_supported"), local)
        self.assertIs(result["model_load_checked"], False)
        return result

    def test_saved_r2t2_is_reported_instead_of_hardcoded_qwen(self):
        path = self.save_config({"engine": R2T2, "api_token": SECRET})
        before = path.read_bytes()
        self.assert_doctor_selection(R2T2, "desktop_config")
        self.assertEqual(path.read_bytes(), before, "doctor changed desktop configuration")

    def test_missing_file_or_engine_key_uses_default_without_creating_config(self):
        self.assertFalse(self.config_dir.exists())
        self.assert_doctor_selection(QWEN, "default")
        self.assertFalse(self.config_dir.exists(), "doctor created the desktop data directory")
        self.save_config({"api_token": SECRET})
        self.assert_doctor_selection(QWEN, "default")

    def test_saved_local_engines_and_legacy_alias_reach_real_protocol_dispatch(self):
        for saved, expected in ((QWEN, QWEN), (R2T2, R2T2), ("qwen3-asr-1.7b", R2T2)):
            self.save_config({"engine": saved})
            for action in ("status", "transcribe"):
                with self.subTest(saved=saved, action=action), self.launch_fake():
                    code, result = self.invoke(["--python", sys.executable, *self.command(action)])
                    self.assertEqual(code, 0)
                    self.assertTrue(result["success"])
                    self.assertEqual(result["request"]["action"], action)
                    self.assertEqual(self.launches[0][0][-2:], ["--engine", expected])
                    self.assert_reaped(self.clients[0])

    def test_legacy_alias_is_reported_as_local_r2t2(self):
        self.save_config({"engine": "qwen3-asr-1.7b"})
        self.assert_doctor_selection(R2T2, "desktop_config")

    def test_saved_cloud_engine_is_reported_without_loading_models(self):
        for engine in ("glm-asr", "alibaba-asr"):
            with self.subTest(engine=engine):
                self.save_config({"engine": engine, "api_token": SECRET})
                self.assert_doctor_selection(engine, "desktop_config", local=False)

    def test_saved_cloud_engine_blocks_operations_before_child_spawn(self):
        for engine in ("glm-asr", "alibaba-asr"):
            self.save_config({"engine": engine, "api_token": SECRET})
            for action in ("status", "transcribe"):
                with self.subTest(engine=engine, action=action), \
                     mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as launch:
                    code, result = self.invoke(["--python", sys.executable, *self.command(action)])
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    self.assertIn("--engine", result["error"])
                    self.assertIn("local", result["error"].lower())
                    launch.assert_not_called()

    def test_explicit_local_engine_overrides_saved_local_cloud_and_unknown_values(self):
        for saved in (QWEN, R2T2, "qwen3-asr-1.7b", "glm-asr", "alibaba-asr", SECRET):
            self.save_config({"engine": saved, "api_token": SECRET})
            for explicit in (QWEN, R2T2):
                with self.subTest(saved=saved, explicit=explicit):
                    with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as launch:
                        code, result = self.invoke(["--engine", explicit, "doctor"])
                    self.assertEqual(code, 0)
                    self.assertTrue(result["success"])
                    self.assertEqual(result["engine"], explicit)
                    self.assertEqual(result.get("engine_source"), "argument")
                    self.assertIs(result.get("local_engine_supported"), True)
                    self.assertEqual(result.get("config_dir"), str(self.config_dir.resolve()))
                    launch.assert_not_called()
                    with self.launch_fake():
                        code, result = self.invoke(["--python", sys.executable,
                                                    "--engine", explicit, "status"])
                    self.assertEqual(code, 0)
                    self.assertTrue(result["success"])
                    self.assertEqual(self.launches[0][0][-1], explicit)
                    self.assert_reaped(self.clients[0])

    def test_unknown_or_nonstring_saved_engine_is_a_safe_configuration_error(self):
        for engine in (SECRET, "", None, 17, [], {}):
            self.save_config({"engine": engine, "api_token": SECRET})
            for action in ("doctor", "status", "transcribe"):
                with self.subTest(engine=engine, action=action), \
                     mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as launch:
                    code, result = self.invoke(["--python", sys.executable, *self.command(action)])
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    self.assertIn("engine", result["error"].lower())
                    self.assertIn("engine.json", result["error"])
                    launch.assert_not_called()

    def test_malformed_and_nonobject_config_remain_errors_with_explicit_override(self):
        cases = ["{\"api_token\": \"" + SECRET + "\", broken", json.dumps(SECRET),
                 json.dumps([SECRET]), "null", "17", "true"]
        self.config_dir.mkdir()
        for contents in cases:
            (self.config_dir / "engine.json").write_text(contents, encoding="utf-8")
            for explicit in (None, R2T2):
                for action in ("doctor", "status", "transcribe"):
                    arguments = ["--python", sys.executable]
                    if explicit is not None:
                        arguments += ["--engine", explicit]
                    with self.subTest(contents=contents, explicit=explicit, action=action), \
                         mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as launch:
                        code, result = self.invoke([*arguments, *self.command(action)])
                        self.assertEqual(code, 2)
                        self.assertFalse(result["success"])
                        self.assertIn("engine.json", result["error"])
                        launch.assert_not_called()

    def test_environment_config_path_is_resolved_and_precedes_appdata(self):
        roaming = Path(os.environ["APPDATA"]) / "com.light-whisper.app"
        self.save_config({"engine": QWEN}, roaming)
        self.save_config({"engine": R2T2})
        unresolved = self.config_dir / ".." / self.config_dir.name
        with mock.patch.dict(os.environ, {"LIGHT_WHISPER_DATA_DIR": str(unresolved)}):
            self.assert_doctor_selection(R2T2, "desktop_config")

    @unittest.skipUnless(os.name == "nt", "Windows desktop APPDATA path")
    def test_windows_appdata_fallback_uses_desktop_identifier(self):
        directory = Path(os.environ["APPDATA"]) / "com.light-whisper.app"
        self.save_config({"engine": R2T2}, directory)
        with mock.patch.dict(os.environ):
            os.environ.pop("LIGHT_WHISPER_DATA_DIR", None)
            self.assert_doctor_selection(R2T2, "desktop_config", directory=directory)

    def test_runtime_data_directory_never_replaces_desktop_config_directory(self):
        path = self.save_config({"engine": R2T2, "api_token": SECRET})
        runtime_data = self.directory / "runtime-data"
        runtime_path = self.save_config({"engine": QWEN}, runtime_data)
        before = (path.read_bytes(), runtime_path.read_bytes())
        code, report = self.invoke(["--data-dir", str(runtime_data), "doctor"])
        self.assertEqual(code, 0)
        self.assertEqual(report["engine"], R2T2)
        self.assertEqual(report.get("config_dir"), str(self.config_dir.resolve()))
        with self.launch_fake():
            code, result = self.invoke(["--python", sys.executable,
                                        "--data-dir", str(runtime_data), "status"])
        self.assertEqual(code, 0)
        self.assertTrue(result["success"])
        self.assertEqual(self.launches[0][0][-1], R2T2)
        self.assertEqual(result["environment"]["LIGHT_WHISPER_DATA_DIR"], str(runtime_data.resolve()))
        self.assertEqual((path.read_bytes(), runtime_path.read_bytes()), before)
        self.assert_reaped(self.clients[0])


if __name__ == "__main__":
    unittest.main()
