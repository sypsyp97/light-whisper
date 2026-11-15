"""Public CLI regression gaps from the frozen 714-result mutation snapshot.

Only the maintained launch fixture substitutes a synthetic JSONL backend.
No desktop executable, model, native library or network service is executed.
"""

import json
import math
import os
import unittest
from unittest import mock

from scripts import test_cli_session as sessions
from scripts import test_cli_target as targets
from scripts import test_light_whisper_cli as support


def setUpModule():
    support.setUpModule()


class SurvivorFollowupTests(support.BackendCase):
    invoke = targets.TargetTests.invoke
    single = targets.TargetTests.single
    launch_fake = targets.TargetTests.launch_fake
    requests = targets.TargetTests.requests
    assert_closed = targets.TargetTests.assert_closed

    def setUp(self):
        super().setUp()
        self.script.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        self.app = self.directory / "synthetic app.exe"
        self.app.write_bytes(b"test-only LIGHT_WHISPER_AGENT_PROTOCOL_1")
        self.launches, self.clients = [], []
        repository = self.directory / "repository"
        repository.mkdir()
        (repository / "package.json").write_bytes((support.ROOT / "package.json").read_bytes())
        for patch in (
            mock.patch.object(support.cli, "ROOT", repository),
            mock.patch.dict(os.environ, {"LOCALAPPDATA": str(self.directory / "local-appdata")}),
        ):
            patch.start()
            self.addCleanup(patch.stop)

    def session_arguments(self):
        return ["--python", support.BACKEND_PYTHON, "--timeout", "1", "session"]

    def test_dynamic_explicit_app_target_dispatches_desktop_commands(self):
        for command, action in ((["models"], "models"), (["engine", "get"], "engine_get")):
            self.record.unlink(missing_ok=True)
            with self.subTest(command=command), self.launch_fake():
                code, reply = self.single([
                    "--target=app", "--app-exe", str(self.app), "--timeout", "1", *command,
                ])
                self.assertEqual(code, 0, reply)
                self.assertIs(reply["success"], True)
                self.assertEqual(reply["request"], {"action": action, "request_id": 1})
                self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
                self.assertEqual([(request["action"], request["request_id"])
                                  for request in self.requests()], [(action, 1), ("exit", 2)])
                self.assert_closed()

    def test_leading_whitespace_does_not_swallow_exit_or_acquire_backend(self):
        source = "\n \t\r\n" + json.dumps({"request_id": 10, "action": "exit"}) + "\n"
        invoke = sessions.SessionTests.invoke.__get__(self)
        with mock.patch.object(support.cli, "EngineClient") as child:
            code, replies, _ = invoke(source)
        self.assertEqual(code, 0, replies)
        self.assertEqual(replies, [{"success": True, "request_id": 10}])
        child.assert_not_called()

    def test_interspersed_whitespace_preserves_backend_and_following_records(self):
        records = [
            {"request_id": 20, "action": "status"}, "", " \t\r",
            {"request_id": 21, "action": "stats"}, " \t",
            {"request_id": 22, "action": "exit"},
            {"request_id": 23, "action": "stats"},
        ]
        # The maintained session input helper preserves raw blank/whitespace lines.
        invoke = sessions.SessionTests.invoke.__get__(self)
        with self.launch_fake():
            code, replies, _ = invoke(records, arguments=("--timeout", "1"))
        self.assertEqual(code, 0, replies)
        self.assertEqual([reply.get("request_id") for reply in replies], [20, 21, 22])
        self.assertTrue(all(reply["success"] for reply in replies))
        self.assertEqual(replies[0]["pid"], replies[1]["pid"])
        self.assertEqual(replies[2], {"success": True, "request_id": 22})
        self.assertEqual([(request["action"], request["request_id"])
                          for request in self.requests()], [("status", 1), ("stats", 2), ("exit", 3)])
        self.assert_closed()

    def test_dynamic_app_target_doctor_reports_the_app_runtime(self):
        with mock.patch.object(support.cli, "EngineClient") as child:
            code, reply = self.single([
                "--target=app", "--app-exe", str(self.app), "doctor",
            ])
        self.assertEqual(code, 0, reply)
        self.assertEqual(reply["target"], "app")
        self.assertEqual(reply["runtime"], str(self.app.resolve()))
        self.assertEqual(reply["surface"], "running desktop instance")
        self.assertIs(reply["app_protocol_supported"], True)
        self.assertIs(reply["model_load_checked"], False)
        child.assert_not_called()

    def test_doctor_finds_complete_marker_at_every_read_boundary_split(self):
        marker = b"LIGHT_WHISPER_AGENT_PROTOCOL_1"
        executable = self.directory / "boundary marker.exe"
        # 64 KiB is the current scanner read boundary, not an executable format.
        # Exercise every nonempty prefix/suffix split of the full protocol marker.
        for prefix_length in range(1, len(marker)):
            executable.write_bytes(b"\0" * (64 * 1024 - prefix_length) + marker + b"\0" * 7)
            with self.subTest(prefix_length=prefix_length), \
                 mock.patch.object(support.cli, "EngineClient") as child:
                code, reply = self.single([
                    "--target=app", "--app-exe", str(executable), "doctor",
                ])
                self.assertEqual(code, 0, reply)
                self.assertIs(reply["app_protocol_supported"], True)
                child.assert_not_called()

    def nonfinite_backend(self):
        self.script.write_text(sessions.SESSION_BACKEND.replace(
            "    emit(result)",
            '    if request["request_id"] == 1:\n'
            '        result["metric"] = float("nan")\n'
            "    emit(result)",
        ), encoding="utf-8")

    def test_matching_nonfinite_reply_preserves_numeric_cause_and_terminal_cleanup(self):
        self.nonfinite_backend()
        with self.launch_fake():
            code, replies, _ = self.invoke(self.session_arguments(), [
                {"request_id": 30, "action": "status"},
                {"request_id": 31, "action": "stats"},
                {"request_id": 32, "action": "exit"},
            ])
        self.assertEqual(code, 1)
        self.assertEqual([reply.get("request_id") for reply in replies], [30])
        self.assertIs(replies[0]["success"], False)
        self.assertEqual([request["action"] for request in self.requests()], ["status", "exit"])
        self.assert_closed()
        self.assertRegex(replies[0].get("error", "").lower(), r"non.?finite|nan|infinity")

    def test_matching_nonfinite_reply_raises_runtime_error_from_owned_client(self):
        self.nonfinite_backend()
        client = self.client(timeout=1)
        failure = None
        with client:
            try:
                client.request("status")
            except Exception as error:
                failure = error
        self.assert_reaped(client)
        self.assertTrue(client.child.stdin.closed and client.child.stdout.closed)
        self.assertTrue(all(worker is None or not worker.is_alive()
                            for worker in (client.sender, client.reader)))
        self.assertIsInstance(failure, RuntimeError)
        self.assertRegex(str(failure).lower(), r"non.?finite|nan|infinity")

    def test_queued_response_at_exact_deadline_is_accepted_and_strictly_later_rejected(self):
        # A real child produces the matching frame. The send wrapper places it
        # back in the real receive queue before advancing the controlled clock.
        # This partitions float equality; it makes no real-time scheduling claim.
        for late in (False, True):
            self.record.unlink(missing_ok=True)
            with self.subTest(late=late):
                client = self.client(timeout=1)
                now = [100.0]
                received = None
                failure = None
                queued = []
                with client:
                    original_send = client._send

                    def send_and_queue(message, deadline):
                        original_send(message, deadline)
                        frame = client.messages.get(timeout=1)
                        decoded = json.loads(frame)
                        self.assertEqual(decoded["request_id"], message["request_id"])
                        queued.append(decoded)
                        client.messages.put(frame)
                        now[0] = math.nextafter(deadline, math.inf) if late else deadline

                    with mock.patch.object(client, "_send", side_effect=send_and_queue), \
                         mock.patch.object(support.cli.time, "monotonic", side_effect=lambda: now[0]):
                        try:
                            received = client.request("status")
                        except Exception as error:
                            failure = error
                self.assert_reaped(client)
                self.assertTrue(client.child.stdin.closed and client.child.stdout.closed)
                self.assertEqual(len(queued), 1, "matching response was not queued by the schedule")
                if late:
                    self.assertIsInstance(failure, TimeoutError)
                    self.assertIsNone(received)
                else:
                    self.assertIsNone(failure, "queued response at exact deadline was rejected")
                    self.assertEqual(received, queued[0])
                self.assertTrue(all(worker is None or not worker.is_alive()
                                    for worker in (client.sender, client.reader)))


if __name__ == "__main__":
    unittest.main()
