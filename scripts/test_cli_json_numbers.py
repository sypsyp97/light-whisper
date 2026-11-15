"""Backend numeric validity through the source CLI and its real process owner."""

import contextlib
import io
import json
import math
import os
import shutil
import subprocess
import sys
import unittest
from unittest import mock

from scripts import test_light_whisper_cli as support


BACKEND = r'''
import json, os, sys

mode = os.environ["CLI_NUMBER_MODE"]
token = os.environ["CLI_NUMBER_TOKEN"]
record = os.environ["CLI_TEST_RECORD"]
def note(value):
    with open(record, "a", encoding="utf-8") as stream:
        stream.write(json.dumps(value) + "\n")
def emit(value):
    print(json.dumps(value), flush=True)
def bad(request_id=None):
    value = {"success": mode != "action_false", "details": {"samples": [0, {"metric": "NUMBER_TOKEN"}]}}
    if request_id is not None:
        value["request_id"] = request_id
    print(json.dumps(value).replace('"NUMBER_TOKEN"', token), flush=True)
def noise(request_id=None):
    print("native diagnostic: " + token, flush=True)
    print('{"event":"log","samples":[' + token + ']}', flush=True)
    print('{"success":1,"request_id":1,"samples":[' + token + ']}', flush=True)
    if request_id is not None:
        for foreign in (request_id + 100, True, float(request_id)):
            value = {"success": True, "request_id": foreign, "sample": "NUMBER_TOKEN"}
            print(json.dumps(value).replace('"NUMBER_TOKEN"', token), flush=True)
note({"event": "spawn", "pid": os.getpid()})
try:
    if mode == "noise":
        noise()
    if mode == "startup":
        bad()
    emit({"success": True})
    for line in sys.stdin:
        request = json.loads(line)
        note({"event": "request", "request": request})
        if request["action"] == "exit":
            break
        if mode == "noise":
            noise(request["request_id"])
        if mode in ("action", "action_false"):
            bad(request["request_id"])
        # The immediately queued valid reply catches a strict decoder that
        # silently skips a bad matching frame instead of failing the operation.
        emit({"success": True, "request_id": request["request_id"],
              "details": {"samples": [0, {"metric": 1.25}, -2.5, 1e308, 2 ** 2048,
                                      True, None, "NaN Infinity 1e999"]}})
finally:
    note({"event": "exit"})
'''


def setUpModule():
    support.setUpModule()


def strict_response(line):
    def reject(value):
        raise ValueError("nonfinite JSON constant: " + value)

    value = json.loads(line, parse_constant=reject)

    def finite(item):
        if isinstance(item, dict):
            return all(finite(child) for child in item.values())
        if isinstance(item, list):
            return all(finite(child) for child in item)
        return not isinstance(item, float) or math.isfinite(item)

    if not finite(value):
        raise ValueError("nonfinite decoded number")
    return value


class JsonNumberTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.script.write_text(BACKEND, encoding="utf-8")

    def public_cli(self, mode, token, surface):
        root = self.directory / ("source-cli-" + str(len(self.owned_children)))
        script = root / "scripts/light_whisper_cli.py"
        engine = root / "src-tauri/resources/engine.py"
        script.parent.mkdir(parents=True)
        engine.parent.mkdir(parents=True)
        shutil.copyfile(support.ROOT / "scripts/light_whisper_cli.py", script)
        self.assertEqual(script.read_bytes(), (support.ROOT / "scripts/light_whisper_cli.py").read_bytes())
        engine.write_text(BACKEND, encoding="utf-8")
        record = root / "requests.jsonl"
        caller_ids = (-(2 ** 2048 + 7), 86, 87)
        payload = "".join(json.dumps(value) + "\n" for value in (
            {"request_id": caller_ids[0], "action": "status"},
            {"request_id": caller_ids[1], "action": "stats"},
            {"request_id": caller_ids[2], "action": "exit"},
        )) if surface == "session" else ""
        completed = subprocess.run(
            [support.BACKEND_PYTHON, str(script), "--python", support.BACKEND_PYTHON,
             "--engine", "qwen3-asr-0.6b", "--threads", "1", "--timeout", "1", surface],
            input=payload, text=True, encoding="utf-8", stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, cwd=root, timeout=5,
            env={**os.environ, "APPDATA": str(root / "roaming"),
                 "LIGHT_WHISPER_DATA_DIR": str(root / "config"), "HF_HOME": str(root / "models"),
                 "CLI_NUMBER_MODE": mode, "CLI_NUMBER_TOKEN": token, "CLI_TEST_RECORD": str(record)},
        )
        events = [json.loads(line) for line in record.read_text(encoding="utf-8").splitlines()]
        self.assertEqual(events[-1]["event"], "exit", "owned fake backend did not finish cleanup")
        self.assertEqual(sum(event["event"] == "spawn" for event in events), 1, "operation was retried")
        replies = []
        for line in completed.stdout.splitlines():
            try:
                replies.append(strict_response(line))
            except ValueError as error:
                self.fail(f"CLI stdout is not finite JSON: {error}: {line[:180]}")
        return completed, replies, [event["request"] for event in events if event["event"] == "request"], caller_ids

    def test_source_cli_matching_nonfinite_startup_is_a_runtime_failure(self):
        for token in ("NaN", "1e999"):
            for surface in ("status", "session"):
                with self.subTest(token=token, surface=surface):
                    completed, replies, requests, caller_ids = self.public_cli("startup", token, surface)
                    self.assertEqual(completed.returncode, 1, "matching nonfinite startup was accepted")
                    self.assertEqual(len(replies), 1)
                    self.assertIs(replies[0]["success"], False)
                    self.assertTrue(replies[0].get("error"), "runtime failure must be visible")
                    if surface == "session":
                        self.assertEqual(replies[0]["request_id"], caller_ids[0])
                    self.assertEqual([request["action"] for request in requests], ["exit"])

    def test_source_cli_matching_nonfinite_action_is_terminal_and_never_retried(self):
        for token in ("NaN", "1e999"):
            for surface in ("status", "session"):
                with self.subTest(token=token, surface=surface):
                    completed, replies, requests, caller_ids = self.public_cli("action", token, surface)
                    self.assertEqual(completed.returncode, 1)
                    self.assertEqual(len(replies), 1)
                    self.assertIs(replies[0]["success"], False)
                    self.assertTrue(replies[0].get("error"), "runtime failure must be visible")
                    if surface == "session":
                        self.assertEqual(replies[0]["request_id"], caller_ids[0])
                    self.assertEqual([request["action"] for request in requests], ["status", "exit"])

    def test_source_cli_finite_nested_numbers_and_opaque_integer_ids_are_preserved(self):
        for surface in ("status", "session"):
            with self.subTest(surface=surface):
                completed, replies, requests, caller_ids = self.public_cli("finite", "NaN", surface)
                self.assertEqual(completed.returncode, 0, completed.stderr)
                self.assertTrue(all(reply["success"] for reply in replies))
                expected = [0, {"metric": 1.25}, -2.5, 1e308, 2 ** 2048, True, None, "NaN Infinity 1e999"]
                self.assertEqual(replies[0]["details"]["samples"], expected)
                if surface == "session":
                    self.assertEqual([reply["request_id"] for reply in replies], list(caller_ids))
                    self.assertEqual(replies[1]["details"]["samples"], expected)
                self.assertEqual([request["request_id"] for request in requests],
                                 [1, 2, 3] if surface == "session" else [1, 2])

    def test_nonfinite_backend_false_is_terminal_instead_of_recoverable(self):
        completed, replies, requests, caller_ids = self.public_cli("action_false", "NaN", "session")
        self.assertEqual(completed.returncode, 1)
        self.assertEqual(len(replies), 1)
        self.assertIs(replies[0]["success"], False)
        self.assertEqual(replies[0]["request_id"], caller_ids[0])
        self.assertTrue(replies[0].get("error"))
        self.assertEqual([request["action"] for request in requests], ["status", "exit"])

    def test_source_cli_ignores_nonfinite_foreign_and_unstructured_log_frames(self):
        for token in ("NaN", "1e999"):
            for surface in ("status", "session"):
                with self.subTest(token=token, surface=surface):
                    completed, replies, requests, caller_ids = self.public_cli("noise", token, surface)
                    self.assertEqual(completed.returncode, 0, completed.stderr)
                    self.assertTrue(all(reply["success"] for reply in replies))
                    self.assertEqual(replies[0]["details"]["samples"][1], {"metric": 1.25})
                    self.assertEqual([request["action"] for request in requests],
                                     ["status", "stats", "exit"] if surface == "session" else ["status", "exit"])
                    if surface == "session":
                        self.assertEqual([reply["request_id"] for reply in replies], list(caller_ids))

    def test_matching_bad_frame_reaps_before_failure_output_and_does_not_read_next_record(self):
        for mode in ("startup", "action"):
            for surface in ("status", "session"):
                with self.subTest(mode=mode, surface=surface):
                    clients, consumed, publications = [], [], []
                    self.record.unlink(missing_ok=True)
                    original = support.cli.EngineClient

                    def launch(command, timeout, env=None):
                        client = original([support.BACKEND_PYTHON, str(self.script)], timeout, env={
                            **(env or {}), "CLI_NUMBER_MODE": mode, "CLI_NUMBER_TOKEN": "NaN",
                            "CLI_TEST_RECORD": str(self.record),
                        })
                        clients.append(client)
                        return client

                    class Input:
                        def __iter__(inner):
                            for index, action in enumerate(("status", "stats", "exit")):
                                consumed.append(index)
                                yield json.dumps({"request_id": -7, "action": action}) + "\n"

                    class Output(io.StringIO):
                        def write(inner, text):
                            if text.strip():
                                # Observe without raising inside production error
                                # handling: an observer must not manufacture the
                                # runtime failure that this test is demanding.
                                publications.append((len(clients), clients[0].child.poll(),
                                    all(worker is None or not worker.is_alive()
                                        for worker in (clients[0].reader, clients[0].sender))))
                            return super().write(text)

                    output = Output()
                    with mock.patch.object(support.cli, "EngineClient", side_effect=launch), \
                         mock.patch.object(sys, "stdin", Input()), contextlib.redirect_stdout(output):
                        code = support.cli.main(["--python", support.BACKEND_PYTHON,
                                                 "--timeout", "1", surface])
                    self.assertEqual(code, 1)
                    self.assertEqual(len(clients), 1)
                    self.assert_reaped(clients[0])
                    self.assertEqual(consumed, [0] if surface == "session" else [])
                    self.assertTrue(publications)
                    for count, returncode, workers_done in publications:
                        self.assertEqual(count, 1)
                        self.assertIsNotNone(returncode, "failure output preceded child cleanup")
                        self.assertTrue(workers_done, "failure output preceded worker cleanup")
                    replies = [strict_response(line) for line in output.getvalue().splitlines()]
                    self.assertEqual(len(replies), 1)
                    self.assertIs(replies[0]["success"], False)
                    self.record.unlink()

    def test_ignored_nonfinite_noise_keeps_the_original_absolute_deadline(self):
        client = support.cli.EngineClient([], timeout=1)
        frames = [
            '{"success":true,"request_id":99,"nested":[NaN]}',
            '{"event":"log","nested":[1e999]}',
            '{"success":true,"request_id":1,"value":1}',
        ]
        with mock.patch.object(client.messages, "get", side_effect=frames) as receive, \
             mock.patch.object(support.cli.time, "monotonic", side_effect=[100, 100.4, 100.4, 100.8, 100.8, 101.2]):
            with self.assertRaises(TimeoutError):
                client._receive(1, 101)
        self.assertEqual(receive.call_count, 3)
        for actual, expected in zip(receive.call_args_list, (1, .6, .2)):
            self.assertAlmostEqual(actual.kwargs["timeout"], expected)


if __name__ == "__main__":
    unittest.main()
