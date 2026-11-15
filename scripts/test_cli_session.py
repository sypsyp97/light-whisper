"""Persistent independent-engine sessions through the public CLI JSONL seam.

Recoverable record/backend failures do not change a clean session's exit code.
Transport/startup failures are terminal because their operation outcome is unknown.
The backend is synthetic; these tests do not load a model or inspect desktop state.
"""

import contextlib
import io
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import threading
import unittest
from unittest import mock

from scripts import test_light_whisper_cli as support


SESSION_BACKEND = support.BACKEND.replace(
    "for line in sys.stdin:", "transcription_count = 0\nfor line in sys.stdin:"
).replace(
    '    data_dir = os.environ.get("LIGHT_WHISPER_DATA_DIR", "")',
    '    if request["action"] == "transcribe":\n'
    '        transcription_count += 1\n'
    '    data_dir = os.environ.get("LIGHT_WHISPER_DATA_DIR", "")',
).replace(
    "    emit(result)",
    '    result["pid"] = os.getpid()\n'
    '    result["stats"] = {"transcription_count": transcription_count}\n'
    '    if mode == "first_false" and request["request_id"] == 1:\n'
    '        result.update(success=False, error="backend refused request", detail={"code": 37})\n'
    "    emit(result)",
)


def setUpModule():
    support.setUpModule()


class SessionTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.script.write_text(SESSION_BACKEND, encoding="utf-8")
        self.audio = self.directory / "synthetic 音频.wav"
        self.audio.write_bytes(b"synthetic fixture; backend never decodes audio")
        self.launches, self.clients = [], []

    def launch_fake(self, mode="normal"):
        original = support.cli.EngineClient

        def launch(command, timeout, env=None):
            self.launches.append((list(command), timeout, dict(env or {})))
            client = original(
                [support.BACKEND_PYTHON, str(self.script)], timeout=timeout,
                env={**(env or {}), "CLI_TEST_MODE": mode,
                     "CLI_TEST_RECORD": str(self.record)},
            )
            self.clients.append(client)
            return client

        return mock.patch.object(support.cli, "EngineClient", side_effect=launch)

    def invoke(self, records, arguments=()):
        source = records if isinstance(records, str) else "".join(
            (record if isinstance(record, str) else json.dumps(record, ensure_ascii=True)) + "\n"
            for record in records
        )
        output, errors = io.StringIO(), io.StringIO()
        runtime = [] if any(flag in arguments for flag in ("--python", "--engine-exe")) else [
            "--python", support.BACKEND_PYTHON,
        ]
        with mock.patch.object(sys, "stdin", io.StringIO(source)), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = support.cli.main([
                *runtime, "--timeout", ".35", *arguments, "session",
            ])
        self.assertIs(type(code), int)
        responses = []
        for line in output.getvalue().splitlines():
            try:
                response = json.loads(line)
            except ValueError as error:
                self.fail(f"Non-JSON session stdout: {line!r}: {error}")
            self.assertIsInstance(response, dict)
            self.assertIs(type(response.get("success")), bool)
            responses.append(response)
        return code, responses, errors.getvalue()

    def requests(self):
        return [event["request"] for event in self.recorded() if event["event"] == "request"]

    def assert_closed(self):
        self.assertEqual(len(self.clients), 1)
        client = self.clients[0]
        self.assert_reaped(client)
        self.assertTrue(client.child.stdin.closed)
        self.assertTrue(client.child.stdout.closed)
        for worker in (client.reader, client.sender):
            if worker is not None:
                self.assertFalse(worker.is_alive())

    def test_serial_commands_keep_one_engine_and_independent_request_ids(self):
        identities = [-7, 1, -7, 2 ** 80]
        with self.launch_fake():
            code, replies, _ = self.invoke([
                {"request_id": identity, "action": action}
                for identity, action in zip(identities, ("status", "stats", "status", "stats"))
            ])
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], identities)
        self.assertTrue(all(reply["success"] for reply in replies))
        self.assertEqual(len(self.launches), 1)
        self.assertEqual(len({reply["pid"] for reply in replies}), 1)
        self.assertEqual([reply["request"]["request_id"] for reply in replies], [1, 2, 3, 4])
        self.assertEqual([event["event"] for event in self.recorded()].count("spawn"), 1)
        self.assertEqual([request["action"] for request in self.requests()],
                         ["status", "stats", "status", "stats", "exit"])
        self.assert_closed()

    def test_transcribe_then_stats_observes_same_engine_and_preserves_result_fields(self):
        with self.launch_fake():
            code, replies, _ = self.invoke([
                {"request_id": 61, "action": "transcribe", "audio": str(self.audio),
                 "language": "zh", "context": "测试 context", "hot_words": ["Light Whisper", "术语"]},
                {"request_id": 62, "action": "stats"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), 2)
        self.assertTrue(all(reply["success"] for reply in replies))
        self.assertEqual(replies[0]["text"], "真实 JSONL ✓")
        self.assertEqual(replies[1]["stats"]["transcription_count"], 1)
        self.assertEqual(replies[0]["pid"], replies[1]["pid"])
        self.assertEqual(replies[0]["request"], {
            "request_id": 1, "action": "transcribe", "audio_path": str(self.audio.resolve()),
            "options": {"language": "zh", "context": "测试 context"},
            "hot_words": ["Light Whisper", "术语"],
        })
        self.assert_closed()

    def test_blank_or_empty_stdin_returns_no_reply_and_never_constructs_engine(self):
        for source in ("", "\n \t\n\r\n"):
            with self.subTest(source=source), \
                 mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
                code, replies, _ = self.invoke(source)
            self.assertEqual(code, 0)
            self.assertEqual(replies, [])

    def test_invalid_json_objects_and_ids_reply_per_record_without_engine(self):
        records = ["{broken", "null", "[]", "1", '"text"',
                   {"action": "status"}, {"request_id": True, "action": "status"},
                   {"request_id": 1.0, "action": "status"},
                   {"request_id": "7", "action": "status"},
                   {"request_id": False, "action": "status"},
                   {"request_id": None, "action": "status"},
                   {"request_id": [], "action": "status"},
                   {"request_id": {}, "action": "status"}]
        with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
            code, replies, _ = self.invoke(records)
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), len(records))
        for reply in replies:
            self.assertFalse(reply["success"])
            self.assertIsNone(reply.get("request_id"))
            self.assertIn("request_id", reply)
            self.assertTrue(reply.get("error"))

    def test_unsupported_actions_and_unknown_fields_never_reach_backend(self):
        records = [
            {"request_id": 11, "action": "download"},
            {"request_id": 12, "action": "invoke", "command": "anything"},
            {"request_id": 13, "action": "cleanup"},
            {"request_id": 14, "action": "status", "audio": str(self.audio)},
            {"request_id": 15, "action": "stats", "extra": True},
            {"request_id": 16, "action": "exit", "force": True},
            {"request_id": 17, "action": None},
            {"request_id": 18},
            {"request_id": 19, "action": []},
            {"request_id": 20, "action": {}},
            {"request_id": 21, "action": True},
        ]
        with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
            code, replies, _ = self.invoke(records)
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], list(range(11, 22)))
        self.assertTrue(all(not reply["success"] and reply.get("error") for reply in replies))

    def test_transcribe_field_and_path_errors_are_recoverable_without_engine(self):
        base = {"action": "transcribe", "audio": str(self.audio)}
        fields = [
            {"audio": None}, {"audio": ""}, {"audio": []},
            {"audio": str(self.directory)}, {"audio": str(self.directory / "missing.wav")},
            {"language": None}, {"language": 1}, {"context": []}, {"context": False},
            {"hot_words": "term"}, {"hot_words": ["term", 1]}, {"hot_words": None},
            {"unknown": "field"},
        ]
        records = [{**base, **field, "request_id": index} for index, field in enumerate(fields)]
        records.append({"request_id": 99, "action": "transcribe"})
        with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
            code, replies, _ = self.invoke(records)
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies],
                         list(range(len(fields))) + [99])
        self.assertTrue(all(not reply["success"] and reply.get("error") for reply in replies))

    def test_input_errors_between_valid_commands_do_not_consume_engine_ids(self):
        with self.launch_fake():
            code, replies, _ = self.invoke([
                {"request_id": 31, "action": "status"},
                "{broken", {"request_id": 32, "action": "transcribe", "audio": str(self.directory)},
                {"request_id": 33, "action": "stats"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], [31, None, 32, 33])
        self.assertEqual([reply["success"] for reply in replies], [True, False, False, True])
        self.assertEqual([(request["action"], request["request_id"]) for request in self.requests()],
                         [("status", 1), ("stats", 2), ("exit", 3)])
        self.assert_closed()

    def test_minimal_transcribe_and_explicit_empty_hot_words_are_valid(self):
        with self.launch_fake():
            code, replies, _ = self.invoke([
                {"request_id": 1, "action": "transcribe", "audio": str(self.audio)},
                {"request_id": 2, "action": "transcribe", "audio": str(self.audio),
                 "language": "", "context": "", "hot_words": []},
            ])
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), 2)
        self.assertTrue(all(reply["success"] for reply in replies))
        first, second = (reply["request"] for reply in replies)
        self.assertEqual(first["options"], {})
        self.assertNotIn("hot_words", first)
        self.assertEqual(second["options"], {"language": "", "context": ""})
        self.assertEqual(second["hot_words"], [])
        self.assert_closed()

    def test_backend_failure_reply_is_preserved_and_next_command_runs(self):
        with self.launch_fake("first_false"):
            code, replies, _ = self.invoke([
                {"request_id": 71, "action": "status"},
                {"request_id": 72, "action": "stats"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), 2)
        self.assertEqual([reply.get("request_id") for reply in replies], [71, 72])
        self.assertFalse(replies[0]["success"])
        self.assertEqual(replies[0]["detail"], {"code": 37})
        self.assertTrue(replies[0].get("error"))
        self.assertTrue(replies[1]["success"])
        self.assert_closed()

    def test_stale_future_and_noisy_engine_messages_are_not_session_replies(self):
        with self.launch_fake("noise"):
            code, replies, _ = self.invoke([
                {"request_id": 1001, "action": "status"},
                {"request_id": 1002, "action": "stats"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], [1001, 1002])
        self.assertTrue(all(reply["success"] and "wrong" not in reply for reply in replies))
        self.assert_closed()

    def test_exit_before_first_operation_acknowledges_once_without_engine(self):
        with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
            code, replies, _ = self.invoke([
                {"request_id": 81, "action": "exit"},
                {"request_id": 82, "action": "status"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual(replies, [{"success": True, "request_id": 81}])

    def test_exit_after_operation_reaps_engine_and_discards_queued_commands(self):
        with self.launch_fake():
            code, replies, _ = self.invoke([
                {"request_id": 91, "action": "status"},
                {"request_id": 92, "action": "exit"},
                {"request_id": 93, "action": "stats"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], [91, 92])
        self.assertEqual(replies[1], {"success": True, "request_id": 92})
        self.assertEqual([request["action"] for request in self.requests()], ["status", "exit"])
        self.assert_closed()

    def test_request_timeout_or_eof_is_terminal_and_never_dispatches_next_line(self):
        for mode in ("request_timeout", "request_eof", "request_closed_stdout"):
            with self.subTest(mode=mode), self.launch_fake(mode):
                code, replies, _ = self.invoke([
                    {"request_id": 101, "action": "transcribe", "audio": str(self.audio)},
                    {"request_id": 102, "action": "stats"},
                ])
            self.assertEqual(code, 1)
            self.assertEqual(len(replies), 1)
            self.assertEqual(replies[0].get("request_id"), 101)
            self.assertFalse(replies[0]["success"])
            self.assertTrue(replies[0].get("error"))
            self.assertEqual([request["action"] for request in self.requests()
                              if request["action"] != "exit"], ["transcribe"])
            self.assert_closed()
            self.record.unlink()
            self.launches.clear()
            self.clients.clear()

    def test_startup_failure_is_correlated_to_first_command_and_terminal(self):
        for mode in ("startup_false", "startup_eof", "startup_timeout"):
            with self.subTest(mode=mode), self.launch_fake(mode):
                code, replies, _ = self.invoke([
                    {"request_id": 111, "action": "stats"},
                    {"request_id": 112, "action": "status"},
                ])
            self.assertEqual(code, 1)
            self.assertEqual(len(replies), 1)
            self.assertEqual(replies[0].get("request_id"), 111)
            self.assertFalse(replies[0]["success"])
            self.assertTrue(replies[0].get("error"))
            self.assertFalse(any(request["action"] != "exit" for request in self.requests()))
            self.assert_closed()
            self.record.unlink()
            self.launches.clear()
            self.clients.clear()

    def test_child_environment_is_isolated_offline_and_user_config_is_unchanged(self):
        config_dir = Path(os.environ["LIGHT_WHISPER_DATA_DIR"])
        config_dir.mkdir()
        config = config_dir / "engine.json"
        original = b'{"engine":"confucius4-r2t2","test_only":"keep"}'
        config.write_bytes(original)
        before = set(self.directory.rglob("*"))
        with self.launch_fake():
            code, replies, _ = self.invoke([{"request_id": 121, "action": "status"}])
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), 1)
        self.assertTrue(replies[0]["success"])
        command, _, environment = self.launches[0]
        self.assertEqual(command[-3:], ["serve", "--engine", "confucius4-r2t2"])
        self.assertEqual(environment["HF_HUB_OFFLINE"], "1")
        self.assertEqual(environment["TRANSFORMERS_OFFLINE"], "1")
        self.assertNotEqual(Path(environment["LIGHT_WHISPER_DATA_DIR"]), config_dir)
        self.assertFalse(Path(environment["LIGHT_WHISPER_DATA_DIR"]).exists())
        self.assertEqual(config.read_bytes(), original)
        self.assertEqual(set(config_dir.iterdir()), {config})
        added = set(self.directory.rglob("*")) - before
        self.assertTrue(all(path == self.record or path.name.endswith(".stderr.log") for path in added))
        self.assert_closed()

    def test_explicit_python_and_engine_exe_keep_the_owned_engine_path(self):
        for flag in ("--python", "--engine-exe"):
            with self.subTest(flag=flag), self.launch_fake():
                code, replies, _ = self.invoke(
                    [{"request_id": 125, "action": "status"}],
                    [flag, support.BACKEND_PYTHON],
                )
            self.assertEqual(code, 0)
            self.assertEqual(len(replies), 1)
            self.assertTrue(replies[0]["success"])
            command = self.launches[0][0]
            self.assertEqual(command[0], str(Path(support.BACKEND_PYTHON).resolve()))
            self.assertEqual(str(support.cli.ENGINE_SCRIPT) in command, flag == "--python")
            self.assert_closed()
            self.record.unlink()
            self.launches.clear()
            self.clients.clear()

    def test_script_entrypoint_uses_real_stdin_jsonl_under_cp1252_stdout(self):
        fixture_root = self.directory / "isolated script root"
        script = fixture_root / "scripts" / "light_whisper_cli.py"
        engine = fixture_root / "src-tauri" / "resources" / "engine.py"
        script.parent.mkdir(parents=True)
        engine.parent.mkdir(parents=True)
        shutil.copyfile(support.ROOT / "scripts" / "light_whisper_cli.py", script)
        engine.write_text(SESSION_BACKEND, encoding="utf-8")
        self.assertEqual(script.read_bytes(), (support.ROOT / "scripts" / "light_whisper_cli.py").read_bytes())
        records = (
            {"request_id": 131, "action": "transcribe", "audio": str(self.audio)},
            {"request_id": 132, "action": "stats"},
            {"request_id": 133, "action": "exit"},
            {"request_id": 134, "action": "status"},
        )
        child = subprocess.Popen(
            [support.BACKEND_PYTHON, str(script), "--python", support.BACKEND_PYTHON,
             "--timeout", "1", "session"], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=fixture_root,
            env={**os.environ, "CLI_TEST_MODE": "normal", "CLI_TEST_RECORD": str(self.record),
                 "PYTHONIOENCODING": "cp1252"},
        )
        lines = queue.Queue()

        def read_lines():
            for line in child.stdout:
                lines.put(line)

        reader = threading.Thread(target=read_lines, daemon=True)
        reader.start()
        try:
            child.stdin.write((json.dumps(records[0], ensure_ascii=True) + "\n").encode("ascii"))
            child.stdin.flush()
            try:
                first = json.loads(lines.get(timeout=3))
            except queue.Empty:
                self.fail("Session did not flush its first reply before stdin EOF")
            self.assertEqual(first.get("request_id"), 131)
            self.assertTrue(first["success"])
            tail = "".join(json.dumps(record, ensure_ascii=True) + "\n" for record in records[1:])
            child.stdin.write(tail.encode("ascii"))
            child.stdin.flush()
            child.stdin.close()
            self.assertEqual(child.wait(timeout=5), 0)
            reader.join(timeout=2)
            self.assertFalse(reader.is_alive())
            replies = [first]
            while not lines.empty():
                replies.append(json.loads(lines.get_nowait()))
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=5)
            reader.join(timeout=2)
        self.assertEqual([reply.get("request_id") for reply in replies], [131, 132, 133])
        self.assertEqual(replies[0]["text"], "真实 JSONL ✓")
        self.assertEqual(replies[1]["stats"]["transcription_count"], 1)
        self.assertEqual(replies[0]["pid"], replies[1]["pid"])
        self.assertEqual([request["action"] for request in self.requests()], ["transcribe", "stats", "exit"])

    def controlled_client(self, request_failure=None, cleanup_failure=None):
        client = mock.MagicMock()
        client.__enter__.return_value = client
        client.request.return_value = {"success": True, "request_id": 1, "text": "synthetic transport"}
        client.request.side_effect = request_failure
        cleanup_observations = []

        def close(*exception):
            cleanup_observations.append([
                json.loads(line) for line in sys.stdout.getvalue().splitlines()
            ])
            if cleanup_failure is not None:
                raise cleanup_failure
            return False

        client.__exit__.side_effect = close
        return client, cleanup_observations

    def invoke_input(self, source):
        output, errors = io.StringIO(), io.StringIO()
        with mock.patch.object(sys, "stdin", source), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = support.cli.main(["--python", support.BACKEND_PYTHON, "session"])
        replies = [json.loads(line) for line in output.getvalue().splitlines()]
        for reply in replies:
            self.assertIsInstance(reply, dict)
            self.assertIs(type(reply.get("success")), bool)
        return code, replies, errors.getvalue()

    def test_eof_cleanup_failure_keeps_completed_and_invalid_replies_without_extra_json(self):
        fault = "synthetic_eof_cleanup_fault"
        client, cleanup = self.controlled_client(cleanup_failure=RuntimeError(fault))
        with mock.patch.object(support.cli, "EngineClient", return_value=client):
            code, replies, errors = self.invoke([
                {"request_id": 141, "action": "status"},
                {"request_id": 142, "action": "stats", "unsupported": True},
            ])
        self.assertEqual(code, 1)
        self.assertEqual([reply.get("request_id") for reply in replies], [141, 142])
        self.assertEqual([reply["success"] for reply in replies], [True, False])
        self.assertIn(fault, errors)
        self.assertEqual(cleanup, [replies])
        client.request.assert_called_once_with("status")

    def test_exit_cleanup_failure_replies_once_with_correlated_failure_and_never_acknowledges_early(self):
        fault = "synthetic_exit_cleanup_fault"
        client, cleanup = self.controlled_client(cleanup_failure=RuntimeError(fault))
        with mock.patch.object(support.cli, "EngineClient", return_value=client):
            code, replies, _ = self.invoke([
                {"request_id": 151, "action": "status"},
                {"request_id": 152, "action": "exit"},
                {"request_id": 153, "action": "stats"},
            ])
        self.assertEqual(code, 1)
        self.assertEqual([reply.get("request_id") for reply in replies], [151, 152])
        self.assertEqual([reply["success"] for reply in replies], [True, False])
        self.assertIn(fault, replies[1].get("error", ""))
        self.assertEqual(cleanup, [[replies[0]]])
        client.request.assert_called_once_with("status")

    def test_exit_success_is_published_after_cleanup_completes(self):
        client, cleanup = self.controlled_client()
        with mock.patch.object(support.cli, "EngineClient", return_value=client):
            code, replies, _ = self.invoke([
                {"request_id": 161, "action": "status"},
                {"request_id": 162, "action": "exit"},
            ])
        self.assertEqual(code, 0)
        self.assertEqual([reply.get("request_id") for reply in replies], [161, 162])
        self.assertEqual(replies[1], {"success": True, "request_id": 162})
        self.assertEqual(cleanup, [[replies[0]]])

    def test_keyboard_interrupt_before_any_record_has_no_json_or_engine_and_returns_130(self):
        def interrupted():
            raise KeyboardInterrupt
            yield "unreachable"

        with mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")):
            code, replies, errors = self.invoke_input(interrupted())
        self.assertEqual(code, 130)
        self.assertEqual(replies, [])
        self.assertTrue(errors.strip())

    def test_keyboard_interrupt_reading_next_record_reaps_owner_without_duplicate_reply(self):
        def interrupted():
            yield json.dumps({"request_id": 171, "action": "status"}) + "\n"
            raise KeyboardInterrupt

        client, cleanup = self.controlled_client()
        with mock.patch.object(support.cli, "EngineClient", return_value=client):
            code, replies, errors = self.invoke_input(interrupted())
        self.assertEqual(code, 130)
        self.assertEqual([reply.get("request_id") for reply in replies], [171])
        self.assertTrue(replies[0]["success"])
        self.assertTrue(errors.strip())
        self.assertEqual(cleanup, [replies])
        client.request.assert_called_once_with("status")

    def test_keyboard_interrupt_awaiting_response_reaps_owner_and_fails_pending_id_once(self):
        client, cleanup = self.controlled_client(request_failure=KeyboardInterrupt())
        with mock.patch.object(support.cli, "EngineClient", return_value=client):
            code, replies, _ = self.invoke([
                {"request_id": 181, "action": "status"},
                {"request_id": 182, "action": "stats"},
            ])
        self.assertEqual(code, 130)
        self.assertEqual([reply.get("request_id") for reply in replies], [181])
        self.assertFalse(replies[0]["success"])
        self.assertTrue(replies[0].get("error"))
        self.assertEqual(cleanup, [[]])
        client.request.assert_called_once_with("status")

    def test_script_entrypoint_preserves_raw_utf8_stdin_fields(self):
        fixture_root = self.directory / "UTF8 script root"
        script = fixture_root / "scripts" / "light_whisper_cli.py"
        engine = fixture_root / "src-tauri" / "resources" / "engine.py"
        script.parent.mkdir(parents=True)
        engine.parent.mkdir(parents=True)
        shutil.copyfile(support.ROOT / "scripts" / "light_whisper_cli.py", script)
        engine.write_text(SESSION_BACKEND, encoding="utf-8")
        context, words = "中文 context ✓", ["术语", "Light Whisper"]
        records = [
            {"request_id": 191, "action": "transcribe", "audio": str(self.audio),
             "language": "zh", "context": context, "hot_words": words},
            {"request_id": 192, "action": "stats"},
            {"request_id": 193, "action": "exit"},
        ]
        source = "".join(json.dumps(record, ensure_ascii=False) + "\n" for record in records)
        self.assertIn(context.encode("utf-8"), source.encode("utf-8"))
        completed = subprocess.run(
            [support.BACKEND_PYTHON, str(script), "--python", support.BACKEND_PYTHON,
             "--timeout", "1", "session"], input=source.encode("utf-8"),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=fixture_root,
            env={**os.environ, "CLI_TEST_MODE": "normal", "CLI_TEST_RECORD": str(self.record),
                 "PYTHONIOENCODING": "utf-8"}, timeout=5,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode("utf-8", errors="replace"))
        replies = [json.loads(line) for line in completed.stdout.splitlines()]
        self.assertEqual([reply.get("request_id") for reply in replies], [191, 192, 193])
        self.assertTrue(all(reply["success"] for reply in replies))
        self.assertEqual(replies[0]["request"]["options"], {"language": "zh", "context": context})
        self.assertEqual(replies[0]["request"]["hot_words"], words)
        self.assertEqual(replies[1]["stats"]["transcription_count"], 1)


if __name__ == "__main__":
    unittest.main()
