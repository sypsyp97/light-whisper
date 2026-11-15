"""Public command dispatch and broker-fatal handling through real JSONL children.

Only the process launch boundary substitutes a synthetic broker. Production main,
argument parsing, record validation, EngineClient transport and cleanup all run.
The synthetic child never connects to a desktop, loads a model or decodes audio.
"""

import contextlib
import io
import json
import sys
import unittest
from unittest import mock

from scripts import test_cli_session as sessions
from scripts import test_cli_target as targets
from scripts import test_light_whisper_cli as support


ENGINES = ("qwen3-asr-0.6b", "confucius4-r2t2")
COMMANDS = (
    (("models",), "models", {}),
    (("devices",), "devices", {}),
    (("engine", "get"), "engine_get", {}),
    *(((("engine", "set", engine), "engine_set", {"engine": engine}))
      for engine in ENGINES),
    (("engine", "start"), "engine_start", {}),
    (("engine", "restart"), "engine_restart", {}),
)
BROKER_BACKEND = sessions.SESSION_BACKEND.replace(
    '    data_dir = os.environ.get("LIGHT_WHISPER_DATA_DIR", "")',
    '    if mode == "fatal_noise":\n'
    '        emit({"success": False, "request_id": request["request_id"] + 1,\n'
    '              "fatal": True, "error": "unmatched fatal response"})\n'
    '        emit({"success": "false", "request_id": request["request_id"],\n'
    '              "fatal": True, "error": "invalid fatal response"})\n'
    '        emit({"request_id": request["request_id"], "fatal": True})\n'
    '    data_dir = os.environ.get("LIGHT_WHISPER_DATA_DIR", "")',
).replace(
    "    emit(result)",
    '    if request["request_id"] == 1:\n'
    '        if mode in ("fatal_false", "fatal_true"):\n'
    '            result.update(success=mode == "fatal_true", fatal=True,\n'
    '                          error="synthetic native connection lost")\n'
    '        elif mode in ("recoverable_false", "recoverable_missing"):\n'
    '            result.update(success=False, error="synthetic command refused")\n'
    '            if mode == "recoverable_false":\n'
    '                result["fatal"] = False\n'
    "    emit(result)",
)


def setUpModule():
    support.setUpModule()


class CountedInput(io.StringIO):
    def __init__(self, records):
        super().__init__("".join(json.dumps(record) + "\n" for record in records))
        self.consumed = 0

    def __next__(self):
        line = super().__next__()
        self.consumed += 1
        return line


class CommandSurfaceTests(support.BackendCase):
    invoke = targets.TargetTests.invoke
    single = targets.TargetTests.single
    launch_fake = targets.TargetTests.launch_fake
    assert_closed = targets.TargetTests.assert_closed
    requests = targets.TargetTests.requests

    def setUp(self):
        super().setUp()
        self.script.write_text(BROKER_BACKEND, encoding="utf-8")
        self.app = self.directory / "synthetic app.exe"
        self.app.write_bytes(b"test-only LIGHT_WHISPER_AGENT_PROTOCOL_1")
        self.audio = self.directory / "synthetic 音频.wav"
        self.audio.write_bytes(b"test fixture; synthetic broker never decodes")
        self.launches, self.clients = [], []

    def app_arguments(self, *command):
        return ["--app-exe", str(self.app), "--timeout", ".5", *command]

    def clear_transport(self):
        self.record.unlink(missing_ok=True)
        self.launches, self.clients = [], []

    def test_one_shot_desktop_commands_dispatch_the_exact_wire_action(self):
        for command, action, fields in COMMANDS:
            self.clear_transport()
            with self.subTest(command=command), self.launch_fake():
                code, result = self.single(self.app_arguments(*command))
            self.assertEqual(code, 0, result)
            self.assertTrue(result["success"])
            self.assertEqual(result["request"], {
                "action": action, "request_id": 1, **fields,
            })
            self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
            self.assertEqual([request["action"] for request in self.requests()], [action, "exit"])
            self.assert_closed()

    def test_desktop_commands_reject_explicit_or_inferred_engine_target_before_spawn(self):
        selections = (["--target", "engine"], ["--engine", ENGINES[0]],
                      ["--python", support.BACKEND_PYTHON])
        for command, _, _ in COMMANDS:
            for selection in selections:
                with self.subTest(command=command, selection=selection), self.launch_fake():
                    code, result = self.single([*selection, *command])
                self.assertEqual(code, 2)
                self.assertFalse(result["success"])
                self.assertTrue(result.get("error"))
                self.assertEqual(self.launches, [])
                self.assertFalse(self.record.exists())

    def test_one_shot_engine_set_accepts_only_the_two_local_engine_names(self):
        for engine in ("glm-asr", "alibaba-asr", "qwen3-asr-1.7b", "unknown", ""):
            with self.subTest(engine=engine), self.launch_fake():
                code, result = self.single(self.app_arguments("engine", "set", engine))
            self.assertEqual(code, 2)
            self.assertFalse(result["success"])
            self.assertTrue(result.get("error"))
            self.assertEqual(self.launches, [])
        for command in (("engine",), ("engine", "set"), ("engine", "stop")):
            with self.subTest(command=command), self.launch_fake():
                code, result = self.single(self.app_arguments(*command))
            self.assertEqual(code, 2)
            self.assertFalse(result["success"])
            self.assertEqual(self.launches, [])

    def test_transcribe_hot_words_are_repeatable_top_level_strings_and_omitted_when_absent(self):
        selections = (["--app-exe", str(self.app)], ["--python", support.BACKEND_PYTHON])
        for selection in selections:
            for words in (None, ["Light Whisper", "术语 ✓", "", "术语 ✓"]):
                self.clear_transport()
                flags = [] if words is None else [part for word in words for part in ("--hot-word", word)]
                with self.subTest(selection=selection, words=words), self.launch_fake():
                    code, result = self.single([
                        *selection, "--timeout", ".5", "transcribe", str(self.audio),
                        "--language", "zh", "--context", "测试 context", *flags,
                    ])
                self.assertEqual(code, 0, result)
                expected = {"request_id": 1, "action": "transcribe",
                            "audio_path": str(self.audio.resolve()),
                            "options": {"language": "zh", "context": "测试 context"}}
                if words is not None:
                    expected["hot_words"] = words
                self.assertEqual(result["request"], expected)
                self.assert_closed()

    def test_desktop_session_accepts_all_commands_and_preserves_opaque_external_ids(self):
        identities = [-7, 2 ** 80, -7, 0, -2 ** 80, 1, 1]
        records = [{"request_id": identity, "action": action, **fields}
                   for identity, (_, action, fields) in zip(identities, COMMANDS)]
        with self.launch_fake():
            code, replies, _ = self.invoke(self.app_arguments("session"), records)
        self.assertEqual(code, 0)
        self.assertEqual([reply["request_id"] for reply in replies], identities)
        self.assertTrue(all(reply["success"] for reply in replies), replies)
        self.assertEqual([reply["request"] for reply in replies], [
            {"request_id": number, "action": action, **fields}
            for number, (_, action, fields) in enumerate(COMMANDS, 1)
        ])
        self.assertEqual(len(self.launches), 1)
        self.assertEqual([request["action"] for request in self.requests()],
                         [action for _, action, _ in COMMANDS] + ["exit"])
        self.assert_closed()

    def test_session_engine_set_and_command_field_validation_is_recoverable_before_spawn(self):
        invalid = [{"action": "engine_set"}]
        invalid += [{"action": "engine_set", "engine": engine} for engine in (
            "glm-asr", "alibaba-asr", "qwen3-asr-1.7b", "", None, True, 1, [], {},
        )]
        invalid += [{"action": "engine_set", "engine": ENGINES[0], "extra": True}]
        invalid += [{"action": action, "engine": ENGINES[0]}
                    for action in ("models", "devices", "engine_get", "engine_start", "engine_restart")]
        records = [{"request_id": number, **record} for number, record in enumerate(invalid, 1)]
        records.append({"request_id": -1, "action": "exit"})
        with self.launch_fake():
            code, replies, _ = self.invoke(self.app_arguments("session"), records)
        self.assertEqual(code, 0)
        self.assertEqual([reply["request_id"] for reply in replies], list(range(1, len(invalid) + 1)) + [-1])
        self.assertTrue(all(not reply["success"] and reply.get("error") for reply in replies[:-1]))
        self.assertEqual(replies[-1], {"request_id": -1, "success": True})
        self.assertEqual(self.launches, [])

    def test_desktop_only_session_records_reject_engine_target_without_starting_child(self):
        records = [{"request_id": number, "action": action, **fields}
                   for number, (_, action, fields) in enumerate(COMMANDS, 1)]
        records.append({"request_id": 90, "action": "exit"})
        with self.launch_fake():
            code, replies, _ = self.invoke(["--target", "engine", "session"], records)
        self.assertEqual(code, 0)
        self.assertTrue(all(not reply["success"] and reply.get("error") for reply in replies[:-1]))
        self.assertEqual([reply["request_id"] for reply in replies], list(range(1, len(COMMANDS) + 1)) + [90])
        self.assertEqual(replies[-1], {"request_id": 90, "success": True})
        self.assertEqual(self.launches, [])

    def test_rejected_desktop_records_preserve_an_existing_independent_session(self):
        records = [{"request_id": 1, "action": "status"}]
        records += [{"request_id": number, "action": action, **fields}
                    for number, (_, action, fields) in enumerate(COMMANDS, 2)]
        records.append({"request_id": 99, "action": "stats"})
        with self.launch_fake():
            code, replies, _ = self.invoke([
                "--python", support.BACKEND_PYTHON, "--timeout", ".5", "session",
            ], records)
        self.assertEqual(code, 0)
        self.assertTrue(replies[0]["success"])
        self.assertTrue(all(not reply["success"] for reply in replies[1:-1]))
        self.assertTrue(replies[-1]["success"])
        self.assertEqual([request["action"] for request in self.requests()], ["status", "stats", "exit"])
        self.assert_closed()

    def test_matching_valid_fatal_response_raises_from_engine_client_for_either_success_value(self):
        for mode in ("fatal_false", "fatal_true"):
            self.clear_transport()
            client = self.client(mode=mode, timeout=.5)
            with self.subTest(mode=mode):
                with self.assertRaises(Exception) as caught:
                    with client:
                        client.request("status")
                self.assertIn("synthetic native connection lost", str(caught.exception))
                self.assertEqual([request["action"] for request in self.requests()], ["status", "exit"])
                self.assert_reaped(client)

    def test_unmatched_or_invalid_fatal_response_is_ignored_until_valid_matching_reply(self):
        client = self.client(mode="fatal_noise", timeout=.5)
        with client:
            reply = client.request("status")
        self.assertTrue(reply["success"])
        self.assertEqual(reply["request_id"], 1)
        self.assertEqual(reply["request"]["action"], "status")
        self.assert_reaped(client)

    def test_fatal_session_response_is_correlated_terminal_without_reading_or_retrying_next_record(self):
        for mode in ("fatal_false", "fatal_true"):
            self.clear_transport()
            source = CountedInput([
                {"request_id": -2 ** 80, "action": "status"},
                {"request_id": 22, "action": "stats"},
            ])
            output = io.StringIO()
            with self.subTest(mode=mode), self.launch_fake(mode), \
                 mock.patch.object(sys, "stdin", source), contextlib.redirect_stdout(output):
                code = support.cli.main(self.app_arguments("session"))
            replies = [json.loads(line) for line in output.getvalue().splitlines()]
            self.assertEqual(code, 1)
            self.assertEqual(len(replies), 1)
            self.assertEqual(replies[0]["request_id"], -2 ** 80)
            self.assertFalse(replies[0]["success"])
            self.assertIn("synthetic native connection lost", replies[0].get("error", ""))
            self.assertEqual(source.consumed, 1, "fatal transport consumed a queued session record")
            self.assertEqual([request["action"] for request in self.requests()], ["status", "exit"])
            self.assertEqual(len(self.launches), 1, "fatal transport was automatically retried")
            self.assert_closed()

    def test_explicit_backend_failure_without_fatal_true_remains_recoverable(self):
        for mode in ("recoverable_false", "recoverable_missing"):
            self.clear_transport()
            with self.subTest(mode=mode), self.launch_fake(mode):
                code, replies, _ = self.invoke(self.app_arguments("session"), [
                    {"request_id": 17, "action": "status"},
                    {"request_id": 18, "action": "stats"},
                ])
            self.assertEqual(code, 0)
            self.assertEqual([reply["request_id"] for reply in replies], [17, 18])
            self.assertFalse(replies[0]["success"])
            self.assertIn("synthetic command refused", replies[0].get("error", ""))
            self.assertTrue(replies[1]["success"])
            self.assertEqual([request["action"] for request in self.requests()], ["status", "stats", "exit"])
            self.assert_closed()


if __name__ == "__main__":
    unittest.main()
