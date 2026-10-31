import json
import os
import queue
import sys
import threading
import types
import unittest
from unittest import mock

from test_server_stream_dispatch import RecordingStreamServer


_NATIVE_NOISE = "native-worker-only-noise"


class _SuppressingServer(RecordingStreamServer):
    def __init__(self):
        super().__init__(fail_request_id=12)
        self.suppression_entered = threading.Event()
        self.release_worker = threading.Event()
        self.worker_finished = threading.Event()
        self.worker = None

    def initialize(self):
        self.worker = threading.Thread(target=self._native_worker, daemon=True)
        self.worker.start()
        if not self.suppression_entered.wait(timeout=2):
            raise TimeoutError("native worker did not enter stdout suppression")
        return super().initialize()

    def _native_worker(self):
        try:
            with self.stdout_suppressor.suppress():
                print(_NATIVE_NOISE, flush=True)
                self.suppression_entered.set()
                self.release_worker.wait(timeout=10)
        finally:
            self.worker_finished.set()


class ServerProtocolOutputTests(unittest.TestCase):
    def test_all_responses_flush_before_suppressed_worker_is_released(self):
        server = _SuppressingServer()
        commands = [
            json.dumps({"action": "stream_feed", "request_id": 11}),
            json.dumps({"action": "stream_feed", "request_id": 12}),
            "{invalid-json",
            json.dumps({"action": "stream_finish", "request_id": 13}),
            json.dumps({"action": "exit", "request_id": 14}),
        ]
        fake_hf_cache = types.SimpleNamespace(
            get_hf_cache_root=lambda: "unused",
            is_hf_repo_ready=lambda _repo: True,
        )
        lines = queue.Queue()
        run_errors = []

        def run_server():
            try:
                server.run()
            except BaseException as exc:
                run_errors.append(exc)

        read_fd, write_fd = os.pipe()
        command_read_fd, command_write_fd = os.pipe()
        with (
            os.fdopen(read_fd, "r", encoding="utf-8") as protocol_reader,
            os.fdopen(command_read_fd, "r", encoding="utf-8") as command_reader,
            os.fdopen(command_write_fd, "w", encoding="utf-8") as command_writer,
        ):
            def read_responses():
                for line in protocol_reader:
                    lines.put(line)

            reader = threading.Thread(target=read_responses, daemon=True)
            runner = threading.Thread(target=run_server, daemon=True)
            reader.start()
            try:
                # A real buffered pipe makes each response's flush observable.
                with (
                    os.fdopen(write_fd, "w", encoding="utf-8") as protocol_output,
                    mock.patch.object(sys, "stdin", command_reader),
                    mock.patch.object(sys, "stdout", protocol_output),
                    mock.patch.dict(sys.modules, {"hf_cache_utils": fake_hf_cache}),
                    mock.patch.dict(os.environ, {"LIGHT_WHISPER_GPU_IDLE_SECONDS": "0"}),
                ):
                    runner.start()
                    try:
                        self.assertTrue(server.suppression_entered.wait(timeout=2))
                        responses = []
                        for command, response_kind in zip(
                            [None] + commands,
                            ("init", "action", "error", "invalidJSON", "recovery", "exit"),
                        ):
                            # Send the next command only after receiving the prior reply.
                            if command is not None:
                                command_writer.write(command + "\n")
                                command_writer.flush()
                            try:
                                line = lines.get(timeout=2)
                            except queue.Empty:
                                self.fail(
                                    f"{response_kind} response was not flushed while the native "
                                    f"worker held stdout suppression; run errors: {run_errors!r}"
                                )
                            self.assertNotIn(_NATIVE_NOISE, line)
                            responses.append(json.loads(line))
                            self.assertFalse(server.worker_finished.is_set())

                        self.assertEqual(
                            [(response["success"], response.get("request_id")) for response in responses],
                            [(True, None), (True, 11), (False, 12), (False, None), (True, 13), (True, 14)],
                        )
                        self.assertEqual(responses[1]["action"], "stream_feed")
                        self.assertEqual(responses[4]["action"], "stream_finish")
                        for response in responses[2:4]:
                            self.assertIsInstance(response.get("error"), str)
                            self.assertTrue(response["error"].strip())
                        self.assertIsInstance(responses[2].get("traceback"), str)
                        self.assertNotIn("request_id", responses[0])
                        self.assertNotIn("request_id", responses[3])
                    finally:
                        server.release_worker.set()
                        command_writer.close()
                        if server.worker is not None:
                            server.worker.join(timeout=2)
                        runner.join(timeout=2)
            finally:
                reader.join(timeout=2)

        self.assertFalse(server.worker.is_alive())
        self.assertFalse(runner.is_alive())
        self.assertFalse(reader.is_alive())
        self.assertEqual(run_errors, [])


if __name__ == "__main__":
    unittest.main()
