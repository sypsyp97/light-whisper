import io
import json
import logging
import os
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

from server_common import BaseASRServer


class _TestServerBase(BaseASRServer):
    def __init__(self):
        self.initialize_calls = 0
        self.transcribe_calls = []
        self.status_calls = 0
        logger = logging.getLogger(f"test_server_stream_dispatch.{id(self)}")
        logger.disabled = True
        logger.propagate = False
        with mock.patch("server_common.signal.signal"):
            super().__init__(engine="test", logger=logger)

    def _setup_runtime_environment(self):
        pass  # Dispatch tests must not alter the runner's process environment.

    def _detect_device(self):
        return "cpu"

    def _get_model_repos(self):
        return []

    def initialize(self):
        self.initialize_calls += 1
        return {"success": True, "engine": self.engine}

    def check_status(self):
        self.status_calls += 1
        return {"success": True, "status": "ready"}

    def get_performance_stats(self):
        return {"success": True}

    def transcribe_audio(self, *args, **kwargs):
        self.transcribe_calls.append((args, kwargs))
        return {"success": True, "text": "ordinary"}


class RecordingStreamServer(_TestServerBase):
    def __init__(self, fail_request_id=None):
        self.stream_commands = []
        self.fail_request_id = fail_request_id
        super().__init__()

    def handle_stream_command(self, command):
        self.stream_commands.append(command)
        if command.get("request_id") == self.fail_request_id:
            raise RuntimeError("stream hook failed")
        return {"success": True, "action": command["action"]}


class ServerStreamDispatchTests(unittest.TestCase):
    @staticmethod
    def _run_commands(server, commands):
        stdin = io.StringIO(
            "\n".join(json.dumps(command, ensure_ascii=False) for command in commands)
            + "\n"
        )
        stdout = io.StringIO()
        fake_hf_cache = types.SimpleNamespace(
            get_hf_cache_root=lambda: "unused",
            is_hf_repo_ready=lambda _repo: True,
        )
        with (
            mock.patch.object(sys, "stdin", stdin),
            mock.patch.object(sys, "stdout", stdout),
            mock.patch.dict(sys.modules, {"hf_cache_utils": fake_hf_cache}),
        ):
            server.run()
        return [json.loads(line) for line in stdout.getvalue().splitlines()]

    def test_stream_actions_route_with_full_payload_and_request_ids(self):
        server = RecordingStreamServer()
        commands = [
            {
                "action": "stream_start",
                "request_id": 101,
                "session_id": 7,
                "context": "你好",
                "language": "zh",
            },
            {
                "action": "stream_feed",
                "request_id": 102,
                "session_id": 7,
                "offset": 160,
                "audio": [0.1, 0.2],
                "options": {"source": "test"},
            },
            {
                "action": "stream_finish",
                "request_id": 103,
                "session_id": 7,
                "options": {"flush": True},
            },
            {
                "action": "stream_cancel",
                "request_id": 104,
                "session_id": 7,
                "reason": "test",
            },
            {"action": "exit", "request_id": 105},
        ]

        responses = self._run_commands(server, commands)

        self.assertTrue(responses[0]["success"])
        self.assertEqual(server.stream_commands, commands[:4])
        self.assertEqual(server.transcribe_calls, [])
        for command, response in zip(commands[:4], responses[1:5]):
            self.assertTrue(response["success"])
            self.assertEqual(response["action"], command["action"])
            self.assertEqual(response["request_id"], command["request_id"])
        self.assertTrue(responses[5]["success"])
        self.assertEqual(responses[5]["request_id"], 105)

    def test_base_default_stream_hook_is_unsupported_without_new_work(self):
        server = _TestServerBase()

        responses = self._run_commands(
            server,
            [
                {"action": "stream_start", "request_id": 201},
                {"action": "exit", "request_id": 202},
            ],
        )

        unsupported = responses[1]
        self.assertFalse(unsupported["success"])
        self.assertIsInstance(unsupported.get("error"), str)
        self.assertTrue(unsupported["error"].strip())
        self.assertEqual(unsupported["request_id"], 201)
        self.assertEqual(server.initialize_calls, 1)
        self.assertEqual(server.transcribe_calls, [])
        self.assertTrue(responses[2]["success"])

    def test_stream_hook_error_keeps_request_id_and_loop_recovers(self):
        server = RecordingStreamServer(fail_request_id=302)
        commands = [
            {"action": "stream_feed", "request_id": 301, "session_id": 1},
            {"action": "stream_feed", "request_id": 302, "session_id": 1},
            {"action": "stream_finish", "request_id": 303, "session_id": 1},
            {"action": "exit", "request_id": 304},
        ]

        responses = self._run_commands(server, commands)

        self.assertTrue(responses[1]["success"])
        self.assertEqual(responses[1]["request_id"], 301)
        self.assertFalse(responses[2]["success"])
        self.assertEqual(responses[2]["request_id"], 302)
        self.assertIsInstance(responses[2].get("error"), str)
        self.assertTrue(responses[3]["success"])
        self.assertEqual(responses[3]["request_id"], 303)
        self.assertTrue(responses[4]["success"])
        self.assertEqual(responses[4]["request_id"], 304)
        self.assertEqual(server.stream_commands, commands[:3])

    def test_stream_typo_stays_unknown_and_does_not_reach_stream_hook(self):
        server = RecordingStreamServer()

        responses = self._run_commands(
            server,
            [
                {"action": "stream_typo", "request_id": 401},
                {"action": "exit", "request_id": 402},
            ],
        )

        self.assertFalse(responses[1]["success"])
        self.assertEqual(responses[1]["request_id"], 401)
        self.assertIsInstance(responses[1].get("error"), str)
        self.assertEqual(server.stream_commands, [])
        self.assertTrue(responses[2]["success"])
        self.assertEqual(responses[2]["request_id"], 402)

    def test_status_transcribe_and_exit_dispatch_remain_available(self):
        server = RecordingStreamServer()

        responses = self._run_commands(
            server,
            [
                {"action": "status", "request_id": 501},
                {
                    "action": "transcribe",
                    "request_id": 502,
                    "audio_path": None,
                    "options": {"source": "test"},
                },
                {"action": "exit", "request_id": 503},
            ],
        )

        self.assertTrue(responses[1]["success"])
        self.assertEqual(responses[1]["request_id"], 501)
        self.assertTrue(responses[2]["success"])
        self.assertEqual(responses[2]["request_id"], 502)
        self.assertTrue(responses[3]["success"])
        self.assertEqual(responses[3]["request_id"], 503)
        self.assertEqual(server.status_calls, 1)
        self.assertEqual(len(server.transcribe_calls), 1)
        self.assertEqual(server.stream_commands, [])


if __name__ == "__main__":
    unittest.main()
