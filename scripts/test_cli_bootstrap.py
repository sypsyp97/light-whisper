"""Frozen resource/runtime and desktop model-cache parity through public main."""

import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import test_light_whisper_cli as support


QWEN = "qwen3-asr-0.6b"
AGENT_PROTOCOL = b"LIGHT_WHISPER_AGENT_PROTOCOL_1"
CLI_SOURCE = support.ROOT / "scripts/light_whisper_cli.py"


class BootstrapTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.layout_count = 0
        self.home = self.directory / "synthetic-home"
        self.inherited_cache = self.directory / "inherited-wrong-cache"
        environment = mock.patch.dict(os.environ, {
            "HOME": str(self.home), "USERPROFILE": str(self.home),
            "HF_HUB_CACHE": str(self.inherited_cache),
        })
        environment.start()
        self.addCleanup(environment.stop)
        os.environ.pop("HF_HOME", None)
        home = mock.patch.object(Path, "home", return_value=self.home)
        home.start()
        self.addCleanup(home.stop)

    def file(self, path, content=b"synthetic file; never executed"):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return path

    def app_file(self, path):
        return self.file(path, b"synthetic app fixture; never executed; " + AGENT_PROTOCOL)

    @contextlib.contextmanager
    def load_cli(self, *, frozen=False, python=False, script=False, engine=False, config=None):
        self.layout_count += 1
        base = self.directory / f"layout-{self.layout_count}"
        source = base / "source"
        assets = base / "frozen-assets"
        copied = self.file(source / "scripts/light_whisper_cli.py", CLI_SOURCE.read_bytes())
        self.file(source / "package.json", b'{"version":"source-fixture-1.2.3"}')
        self.file(assets / "package.json", b'{"version":"frozen-fixture-9.8.7"}')
        executable = self.file(base / "installed-cli/light-whisper-cli.exe")
        root = assets if frozen else source
        paths = {
            "base": base, "source": source, "assets": assets, "root": root,
            "cli": copied, "executable": executable,
            "neighbor": executable.with_name("light-whisper.exe"),
            "release": root / "src-tauri/target/release/light-whisper.exe",
            "debug": root / "src-tauri/target/debug/light-whisper.exe",
            "installed": base / "local-appdata/轻语 Whisper/light-whisper.exe",
            "config": base / "desktop-config",
            "python": root / ".venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python"),
            "script": root / "src-tauri/resources/engine.py",
        }
        paths["engine"] = paths["config"] / "engine/engine.exe"
        if python:
            self.file(paths["python"])
        if script:
            self.file(paths["script"], b"# synthetic engine script; never executed\n")
        if engine:
            self.file(paths["engine"])
        if config is not None:
            self.file(paths["config"] / "engine.json", json.dumps(config).encode("utf-8"))
        with mock.patch.dict(os.environ, {
            "LIGHT_WHISPER_DATA_DIR": str(paths["config"]),
            "APPDATA": str(base / "roaming"), "LOCALAPPDATA": str(base / "local-appdata"),
        }), mock.patch.object(sys, "frozen", frozen, create=True), \
             mock.patch.object(sys, "_MEIPASS", str(assets), create=True), \
             mock.patch.object(sys, "executable", str(executable)):
            loaded = runpy.run_path(str(copied), run_name="cli_bootstrap_fixture")
            yield loaded["main"].__globals__, paths

    def invoke(self, cli, arguments):
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = cli["main"](arguments)
        self.assertIs(type(code), int)
        self.assertEqual(len(output.getvalue().splitlines()), 1, (output.getvalue(), errors.getvalue()))
        result = json.loads(output.getvalue())
        self.assertIsInstance(result, dict)
        self.assertIs(type(result.get("success")), bool)
        return code, result

    @contextlib.contextmanager
    def fake_client(self, cli):
        original = cli["EngineClient"]
        self.launches, self.clients = [], []

        def launch(command, timeout, env=None):
            self.launches.append((list(command), timeout, dict(env or {})))
            client = original([support.BACKEND_PYTHON, str(self.script)], timeout=timeout,
                              env={**(env or {}), "CLI_TEST_MODE": "normal",
                                   "CLI_TEST_RECORD": str(self.record)})
            self.clients.append(client)
            return client

        with mock.patch.dict(cli, {"EngineClient": launch}):
            yield

    def assert_closed(self):
        self.assertEqual(len(self.clients), 1)
        self.assert_reaped(self.clients[0])
        self.assertTrue(self.clients[0].child.stdin.closed)
        self.assertTrue(self.clients[0].child.stdout.closed)

    def test_source_and_frozen_doctor_use_their_actual_resource_root_and_metadata(self):
        for frozen in (False, True):
            with self.subTest(frozen=frozen), self.load_cli(frozen=frozen) as (cli, paths), \
                 mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")), \
                 mock.patch.object(tempfile, "TemporaryDirectory", side_effect=AssertionError("temporary data")):
                code, result = self.invoke(cli, ["doctor"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(result["version"], "frozen-fixture-9.8.7" if frozen else "source-fixture-1.2.3")
                self.assertEqual(cli["ROOT"], paths["root"].resolve())
                self.assertEqual(result["repository"], None if frozen else str(paths["root"].resolve()))
                self.assertEqual(result.get("resource_root"), str(paths["root"].resolve()))
                self.assertIs(result.get("frozen"), frozen)
                self.assertEqual(result["engine_script"], str(paths["script"].resolve()))
                self.assertIs(result["engine_script_present"], False)
                self.assertIs(result["model_load_checked"], False)
                self.assertIs(result.get("app_protocol_supported"), False)
                self.assertFalse(paths["config"].exists())

    def test_frozen_neighbor_app_precedes_embedded_dev_and_installed_candidates(self):
        with self.load_cli(frozen=True) as (cli, paths):
            for key in ("neighbor", "release", "debug", "installed"):
                self.app_file(paths[key])
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(paths["neighbor"].resolve()), "--agent-client"])
            self.assert_closed()

    def test_frozen_missing_or_nonfile_neighbor_falls_through_to_installed_app(self):
        for neighbor_directory in (False, True):
            with self.subTest(neighbor_directory=neighbor_directory), self.load_cli(frozen=True) as (cli, paths):
                if neighbor_directory:
                    paths["neighbor"].mkdir()
                self.app_file(paths["installed"])
                with self.fake_client(cli):
                    code, result = self.invoke(cli, ["status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], [str(paths["installed"].resolve()), "--agent-client"])
                self.assert_closed()

    def test_source_app_order_ignores_cli_neighbor(self):
        with self.load_cli() as (cli, paths):
            for key in ("neighbor", "release", "debug", "installed"):
                self.app_file(paths[key])
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(paths["release"].resolve()), "--agent-client"])
            self.assert_closed()

    def test_source_missing_localappdata_has_no_default_app_and_never_spawns(self):
        for value in (None, ""):
            with self.subTest(localappdata=value), self.load_cli() as (cli, paths), mock.patch.dict(os.environ):
                if value is None:
                    os.environ.pop("LOCALAPPDATA", None)
                else:
                    os.environ["LOCALAPPDATA"] = value
                child = mock.Mock(side_effect=AssertionError("spawn"))
                with mock.patch.dict(cli, {"EngineClient": child}):
                    code, report = self.invoke(cli, ["doctor"])
                    self.assertEqual(code, 0)
                    self.assertIsNone(report["runtime"])
                    self.assertFalse(report["runtime_present"])
                    self.assertIs(report.get("app_protocol_supported"), False)
                    code, result = self.invoke(cli, ["status"])
                    self.assertEqual(code, 2)
                    self.assertFalse(result["success"])
                    child.assert_not_called()

    def test_frozen_explicit_app_wins_and_explicit_missing_path_never_falls_back(self):
        with self.load_cli(frozen=True) as (cli, paths):
            self.app_file(paths["neighbor"])
            explicit = self.app_file(paths["base"] / "explicit app.exe")
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["--app-exe", str(explicit), "status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(explicit.resolve()), "--agent-client"])
            self.assert_closed()
            missing = paths["base"] / "missing app.exe"
            with mock.patch.dict(cli, {"EngineClient": mock.Mock(side_effect=AssertionError("spawn"))}):
                code, result = self.invoke(cli, ["--app-exe", str(missing), "status"])
            self.assertEqual(code, 2)
            self.assertFalse(result["success"])

    def test_unmarked_app_is_rejected_before_spawn_without_skipping_to_another_instance(self):
        with self.load_cli() as (cli, paths):
            self.file(paths["release"], b"old app fixture without agent support")
            self.app_file(paths["installed"])
            for flags in ([], ["--app-exe", str(paths["release"])]):
                child = mock.Mock(side_effect=AssertionError("unexpected app launch"))
                with self.subTest(flags=flags), mock.patch.dict(cli, {"EngineClient": child}):
                    code, result = self.invoke(cli, [*flags, "status"])
                self.assertEqual(code, 2)
                self.assertFalse(result["success"])
                self.assertTrue("upgrade" in result["error"].lower() or "--app-exe" in result["error"])
                child.assert_not_called()

    def test_doctor_checks_app_protocol_marker_as_data_without_launching(self):
        for payload, supported in ((AGENT_PROTOCOL, True),
                                   (b"old unmarked app", False),
                                   (b"LIGHT_WHISPER_AGENT_PROTOCOL_2", False),
                                   (AGENT_PROTOCOL[:12] + b"X" + AGENT_PROTOCOL[12:], False)):
            with self.subTest(payload=payload), self.load_cli() as (cli, paths):
                self.file(paths["installed"], payload)
                with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                    code, report = self.invoke(cli, ["doctor"])
                self.assertEqual(code, 0)
                self.assertTrue(report["runtime_present"])
                self.assertIs(report.get("app_protocol_supported"), supported)

    def test_marker_after_large_prefix_is_detected_even_across_common_read_boundaries(self):
        with self.load_cli() as (cli, paths):
            # Position is a binary fixture property, independent of a scanner's block size.
            self.file(paths["installed"], b"\0" * (1_048_576 - 3) + AGENT_PROTOCOL + b"\0" * 17)
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(paths["installed"].resolve()), "--agent-client"])
            self.assert_closed()
            with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                code, report = self.invoke(cli, ["doctor"])
            self.assertEqual(code, 0)
            self.assertIs(report.get("app_protocol_supported"), True)

    def test_protocol_marker_in_non_utf8_binary_allows_doctor_and_broker_launch(self):
        with self.load_cli() as (cli, paths):
            self.file(paths["installed"], b"\xff\xfe" + AGENT_PROTOCOL + b"\x80")
            with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                code, report = self.invoke(cli, ["doctor"])
            self.assertEqual(code, 0)
            self.assertTrue(report["runtime_present"])
            self.assertIs(report.get("app_protocol_supported"), True)
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(paths["installed"].resolve()), "--agent-client"])
            self.assert_closed()

    def test_doctor_missing_or_nonfile_explicit_app_reports_protocol_unsupported(self):
        for directory in (False, True):
            with self.subTest(directory=directory), self.load_cli() as (cli, paths):
                explicit = paths["base"] / "missing-or-directory.exe"
                if directory:
                    explicit.mkdir()
                with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                    code, report = self.invoke(cli, ["--app-exe", str(explicit), "doctor"])
                self.assertEqual(code, 0)
                self.assertFalse(report["runtime_present"])
                self.assertIs(report.get("app_protocol_supported"), False)

    def test_default_engine_runtime_uses_source_pair_only_when_both_files_exist(self):
        for python, script in ((True, True), (False, True), (True, False), (False, False)):
            with self.subTest(python=python, script=script), \
                 self.load_cli(python=python, script=script, engine=True) as (cli, paths):
                expected = ([str(paths["python"].resolve()), str(paths["script"].resolve())]
                            if python and script else [str(paths["engine"].resolve())])
                with self.fake_client(cli):
                    code, result = self.invoke(cli, ["--target", "engine", "status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], expected + ["serve", "--engine", QWEN])
                self.assert_closed()
                with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                    code, result = self.invoke(cli, ["--target", "engine", "doctor"])
                self.assertEqual(code, 0)
                self.assertEqual(result["runtime"], expected[0])
                self.assertIs(result["runtime_present"], True)
                self.assertIs(result["engine_script_present"], script)

    def test_frozen_engine_works_with_installed_runtime_and_no_external_venv(self):
        with self.load_cli(frozen=True, engine=True) as (cli, paths):
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["--target", "engine", "status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            self.assertEqual(self.launches[0][0], [str(paths["engine"].resolve()), "serve", "--engine", QWEN])
            self.assert_closed()
            code, report = self.invoke(cli, ["--target", "engine", "doctor"])
            self.assertEqual(code, 0)
            self.assertEqual(report["runtime"], str(paths["engine"].resolve()))
            self.assertTrue(report["runtime_present"])
            self.assertFalse(report["engine_script_present"])

    def test_nonfile_source_pair_members_fall_back_to_installed_engine(self):
        for directory in ("python", "script"):
            with self.subTest(directory=directory), self.load_cli(
                    python=directory != "python", script=directory != "script", engine=True) as (cli, paths):
                paths[directory].mkdir(parents=True)
                with self.fake_client(cli):
                    code, result = self.invoke(cli, ["--target", "engine", "status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                self.assertEqual(self.launches[0][0], [str(paths["engine"].resolve()), "serve", "--engine", QWEN])
                self.assert_closed()

    def test_missing_or_nonfile_default_runtime_is_error_without_install_or_spawn(self):
        for engine_directory in (False, True):
            with self.subTest(engine_directory=engine_directory), self.load_cli() as (cli, paths):
                if engine_directory:
                    paths["engine"].mkdir(parents=True)
                with mock.patch.dict(cli, {"EngineClient": mock.Mock(side_effect=AssertionError("spawn"))}):
                    code, result = self.invoke(cli, ["--target", "engine", "status"])
                self.assertEqual(code, 2)
                self.assertFalse(result["success"])
                self.assertTrue(result.get("error"))
                self.assertFalse(paths["python"].exists())

    def test_explicit_runtime_paths_are_honored_without_fallback(self):
        with self.load_cli(python=True, script=True, engine=True) as (cli, paths):
            explicit = self.file(paths["base"] / "explicit runtime.exe")
            for flag in ("--python", "--engine-exe"):
                with self.subTest(flag=flag), self.fake_client(cli):
                    code, result = self.invoke(cli, [flag, str(explicit), "status"])
                    self.assertEqual(code, 0)
                    self.assertTrue(result["success"])
                    expected = [str(explicit.resolve())] + ([str(paths["script"].resolve())] if flag == "--python" else [])
                    self.assertEqual(self.launches[0][0], expected + ["serve", "--engine", QWEN])
                    self.assert_closed()
                for bad in (paths["base"] / "missing runtime.exe", paths["base"]):
                    with self.subTest(flag=flag, bad=bad), \
                         mock.patch.dict(cli, {"EngineClient": mock.Mock(side_effect=AssertionError("spawn"))}):
                        code, result = self.invoke(cli, [flag, str(bad), "status"])
                        self.assertEqual(code, 2)
                        self.assertFalse(result["success"])
                        code, report = self.invoke(cli, [flag, str(bad), "doctor"])
                        self.assertEqual(code, 0)
                        self.assertEqual(report["runtime"], str(bad.resolve()))
                        self.assertFalse(report["runtime_present"])

    def test_explicit_python_missing_script_does_not_fall_back_to_installed_engine(self):
        with self.load_cli(engine=True) as (cli, paths):
            explicit = self.file(paths["base"] / "explicit python.exe")
            with mock.patch.dict(cli, {"EngineClient": mock.Mock(side_effect=AssertionError("spawn"))}):
                code, result = self.invoke(cli, ["--python", str(explicit), "status"])
            self.assertEqual(code, 2)
            self.assertFalse(result["success"])

    def test_doctor_model_cache_precedence_expansion_and_invalid_values_match_desktop(self):
        custom = "~/custom models"
        cases = [({"models_dir": custom}, "~/hf home", self.home / "custom models", "desktop_config"),
                 ({}, "~/hf home", self.home / "hf home/hub", "HF_HOME"),
                 ({}, None, self.home / ".cache/huggingface/hub", "default"),
                 ({}, "", Path.cwd() / "hub", "HF_HOME"),
                 ({"models_dir": " "}, None, Path.cwd() / " ", "desktop_config")]
        cases += [({"models_dir": value}, "~/hf home", self.home / "hf home/hub", "HF_HOME")
                  for value in ("", None, 17, True, [], {})]
        for config, hf_home, expected, source in cases:
            with self.subTest(config=config, hf_home=hf_home), self.load_cli(config=config) as (cli, paths), \
                 mock.patch.dict(os.environ):
                if hf_home is None:
                    os.environ.pop("HF_HOME", None)
                else:
                    os.environ["HF_HOME"] = hf_home
                existed = expected.exists()
                for target in ("app", "engine"):
                    with mock.patch.object(subprocess, "Popen", side_effect=AssertionError("spawn")):
                        code, report = self.invoke(cli, ["--target", target, "doctor"])
                    self.assertEqual(code, 0)
                    self.assertEqual(report.get("models_dir"), str(expected.resolve()))
                    self.assertEqual(report.get("models_dir_source"), source)
                self.assertEqual(expected.exists(), existed, "doctor created a model cache")

    def test_model_cache_selection_reuses_the_already_read_desktop_config(self):
        custom = self.directory / "custom models"
        with self.load_cli(config={"models_dir": str(custom)}) as (cli, paths):
            original = Path.read_text
            reads = []

            def read(path, *args, **kwargs):
                if path == paths["config"] / "engine.json":
                    reads.append(path)
                return original(path, *args, **kwargs)

            with mock.patch.object(Path, "read_text", new=read):
                code, report = self.invoke(cli, ["doctor"])
            self.assertEqual(code, 0)
            self.assertEqual(report.get("models_dir"), str(custom.resolve()))
            self.assertEqual(len(reads), 1, "model selection reread mutable desktop configuration")

    def test_independent_child_uses_selected_cache_with_existing_offline_cpu_and_data_isolation(self):
        for kind in ("custom", "HF_HOME", "default"):
            config = {"models_dir": "~/custom models"} if kind == "custom" else {}
            with self.subTest(kind=kind), self.load_cli(python=True, script=True, config=config) as (cli, paths), \
                 mock.patch.dict(os.environ):
                if kind == "HF_HOME":
                    os.environ["HF_HOME"] = str(self.home / "hf-home")
                else:
                    os.environ.pop("HF_HOME", None)
                expected = (self.home / "custom models" if kind == "custom" else
                            self.home / "hf-home/hub" if kind == "HF_HOME" else
                            self.home / ".cache/huggingface/hub").resolve()
                before = (paths["config"] / "engine.json").read_bytes()
                with self.fake_client(cli):
                    code, result = self.invoke(cli, ["--target", "engine", "status"])
                self.assertEqual(code, 0)
                self.assertTrue(result["success"])
                environment = self.launches[0][2]
                self.assertEqual(environment.get("HF_HUB_CACHE"), str(expected))
                for key in support.ENV_KEYS[:3]:
                    self.assertEqual(environment[key], "2")
                for key in support.ENV_KEYS[3:5]:
                    self.assertEqual(environment[key], "1")
                data = Path(environment["LIGHT_WHISPER_DATA_DIR"])
                self.assertNotEqual(data, paths["config"])
                self.assertFalse(data.exists())
                self.assertEqual((paths["config"] / "engine.json").read_bytes(), before)
                self.assertFalse(expected.exists())
                self.assert_closed()

    def test_app_broker_preserves_inherited_cache_environment(self):
        with self.load_cli(config={"models_dir": "~/custom models"}) as (cli, paths), \
             mock.patch.dict(os.environ, {"HF_HOME": str(self.home / "hf-home"),
                                          "HF_HUB_OFFLINE": "0", "TRANSFORMERS_OFFLINE": "0"}):
            self.app_file(paths["installed"])
            before = dict(os.environ)
            with self.fake_client(cli):
                code, result = self.invoke(cli, ["status"])
            self.assertEqual(code, 0)
            self.assertTrue(result["success"])
            expected = {**before, "LIGHT_WHISPER_DATA_DIR": str(paths["config"].resolve())}
            self.assertTrue(self.launches[0][2] == expected,
                            "App broker environment differs from inherited environment")
            self.assert_closed()

    def test_real_session_entrypoint_decodes_raw_utf8_when_redirected_stdin_defaults_to_cp1252(self):
        with self.load_cli(script=True) as (cli, paths):
            paths["script"].write_text(support.BACKEND, encoding="utf-8")
            audio = self.file(paths["base"] / "测试 音频.wav", b"synthetic audio; backend never decodes")
            records = [{"request_id": 91, "action": "transcribe", "audio": str(audio),
                        "context": "中文测试 context", "hot_words": ["轻语", "术语"]},
                       {"request_id": 92, "action": "exit"}]
            completed = subprocess.run(
                [support.BACKEND_PYTHON, str(paths["cli"]), "--python", support.BACKEND_PYTHON,
                 "--timeout", ".5", "session"],
                input="".join(json.dumps(value, ensure_ascii=False) + "\n" for value in records).encode("utf-8"),
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5,
                env={**os.environ, "PYTHONIOENCODING": "cp1252", "CLI_TEST_MODE": "normal",
                     "CLI_TEST_RECORD": str(self.record)},
            )
            self.assertEqual(completed.returncode, 0, completed.stderr.decode("utf-8", errors="replace"))
            self.assertTrue(all(byte < 128 for byte in completed.stdout), "stdout JSON stopped being ASCII-safe")
            replies = [json.loads(line) for line in completed.stdout.splitlines()]
            self.assertEqual([reply["request_id"] for reply in replies], [91, 92])
            self.assertTrue(replies[0]["success"], replies[0])
            self.assertEqual(replies[0]["request"]["audio_path"], str(audio.resolve()))
            self.assertEqual(replies[0]["request"]["options"]["context"], "中文测试 context")
            self.assertEqual(replies[0]["request"]["hot_words"], ["轻语", "术语"])
            self.assertTrue(replies[1]["success"])


if __name__ == "__main__":
    unittest.main()
