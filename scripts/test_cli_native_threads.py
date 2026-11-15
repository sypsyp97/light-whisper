"""CLI main -> real child -> SDK constructors; DLL/model work remains synthetic."""

import contextlib
import io
import json
import os
from pathlib import Path
import sys
import unittest
from unittest import mock

from scripts import test_light_whisper_cli as support


THREAD_ENV = "LIGHT_WHISPER_NATIVE_THREADS"
MAX_THREADS = 2 ** 31 - 1
RESOURCES = Path(__file__).resolve().parents[1] / "src-tauri" / "resources"
CHILD = r'''
import json
import os
import sys
from unittest import mock

sys.path.insert(0, RESOURCE_PATH)
from qwen3_native import QwenNativeRuntime
from r2t2_native import NativeRuntime
from test_qwen3_native import FakeLibrary

def emit(value):
    print(json.dumps(value), flush=True)

emit({"success": True})
for line in sys.stdin:
    request = json.loads(line)
    if request["action"] == "exit":
        break
    sessions = []
    for runtime_type in (NativeRuntime, QwenNativeRuntime):
        for backend in ("cpu", "cuda"):
            library = FakeLibrary()
            with mock.patch("ctypes.CDLL", return_value=library), \
                 mock.patch("os.add_dll_directory", create=True):
                runtime = runtime_type("synthetic.gguf", "synthetic.dll", backend=backend)
                try:
                    sessions.extend([item[2].decode(), item[4]] for item in library.sessions)
                finally:
                    runtime.close()
            assert len(library.freed) == len(library.allocated)
    emit({"success": True, "request_id": request["request_id"],
          "environment": {key: os.environ.get(key) for key in (
              "LIGHT_WHISPER_NATIVE_THREADS", "OMP_NUM_THREADS", "MKL_NUM_THREADS",
              "OPENBLAS_NUM_THREADS", "LIGHT_WHISPER_DATA_DIR")},
          "sessions": sessions})
'''


def setUpModule():
    support.setUpModule()


class CLINativeThreadTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.script.write_text(CHILD.replace("RESOURCE_PATH", repr(str(RESOURCES))), encoding="utf-8")
        self.app = self.directory / "synthetic app.exe"
        self.app.write_bytes(b"synthetic; never executed; LIGHT_WHISPER_AGENT_PROTOCOL_1")
        self.enterContext(mock.patch.object(support.cli, "ENGINE_SCRIPT", self.script))
        self.clients = []

    def invoke(self, arguments):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), mock.patch.object(sys, "stdin", io.StringIO()):
            code = support.cli.main(arguments)
        return code, json.loads(output.getvalue()) if output.getvalue().strip() else None

    def assert_children_closed(self):
        self.assertTrue(self.owned_children)
        for child in self.owned_children:
            self.assertIsNotNone(child.poll())
            self.assertTrue(child.stdin.closed)
            self.assertTrue(child.stdout.closed)
        for client in self.clients:
            for worker in (client.reader, client.sender):
                if worker is not None:
                    self.assertFalse(worker.is_alive())

    def capture_client(self, *, broker=False):
        original = support.cli.EngineClient

        def create(command, timeout, env=None):
            if broker:
                self.assertEqual(command, [str(self.app.resolve()), "--agent-client"])
                command = [sys.executable, str(self.script)]
            client = original(command, timeout, env=env)
            self.clients.append(client)
            return client

        return mock.patch.object(support.cli, "EngineClient", side_effect=create)

    def test_independent_cli_budget_reaches_both_native_model_abis(self):
        inherited = {THREAD_ENV: "17", "OMP_NUM_THREADS": "19",
                     "MKL_NUM_THREADS": "23", "OPENBLAS_NUM_THREADS": "29"}
        for flags, expected in (([], 2), (["--threads", "1"], 1),
                                (["--threads", "2"], 2),
                                (["--threads", str(MAX_THREADS)], MAX_THREADS)):
            with self.subTest(flags=flags), mock.patch.dict(os.environ, inherited), self.capture_client():
                code, reply = self.invoke(["--target", "engine", "--python", sys.executable,
                                           "--timeout", "4", *flags, "status"])
                self.assertEqual(code, 0, reply)
                self.assertTrue(reply["success"])
                self.assertEqual(reply["environment"][THREAD_ENV], str(expected))
                self.assertEqual(reply["sessions"], [["cpu", expected], ["cuda", expected]] * 2)
                for key in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS"):
                    self.assertEqual(reply["environment"][key], str(expected))
                self.assertFalse(Path(reply["environment"]["LIGHT_WHISPER_DATA_DIR"]).exists())
                self.assert_children_closed()
                for key, value in inherited.items():
                    self.assertEqual(os.environ[key], value, "CLI modified its parent environment")

    def test_app_broker_inherits_thread_environment_without_cli_override(self):
        for inherited_native, expected in ((None, 4), ("9", 9)):
            environment = {key: value for key, value in os.environ.items() if key != THREAD_ENV}
            environment.update(OMP_NUM_THREADS="19", MKL_NUM_THREADS="23", OPENBLAS_NUM_THREADS="29")
            if inherited_native is not None:
                environment[THREAD_ENV] = inherited_native
            with self.subTest(inherited_native=inherited_native), \
                 mock.patch.dict(os.environ, environment, clear=True), self.capture_client(broker=True):
                code, reply = self.invoke(["--target", "app", "--app-exe", str(self.app),
                                           "--threads", "2", "--timeout", "4", "status"])
                self.assertEqual(code, 0, reply)
                self.assertTrue(reply["success"])
                self.assertEqual(reply["environment"][THREAD_ENV], inherited_native)
                self.assertEqual(reply["sessions"], [["cpu", expected], ["cuda", expected]] * 2)
                self.assertEqual(reply["environment"]["OMP_NUM_THREADS"], "19")
                self.assertEqual(reply["environment"]["MKL_NUM_THREADS"], "23")
                self.assertEqual(reply["environment"]["OPENBLAS_NUM_THREADS"], "29")
                self.assertEqual(reply["environment"]["LIGHT_WHISPER_DATA_DIR"],
                                 str(Path(environment["LIGHT_WHISPER_DATA_DIR"]).resolve()))
                self.assert_children_closed()

    def test_invalid_cli_budget_is_exit_two_before_any_child(self):
        targets = (["--target", "engine", "--python", sys.executable],
                   ["--target", "app", "--app-exe", str(self.app)])
        for value in ("0", "-1", "text", "1.5", str(MAX_THREADS + 1), str(2 ** 32), "9" * 100):
            for target in targets:
                for action in ("doctor", "status", "session"):
                    with self.subTest(value=value, target=target, action=action), \
                         mock.patch.object(support.cli, "EngineClient", side_effect=AssertionError("spawn")) as child:
                        code, reply = self.invoke([*target, "--threads", value, action])
                        self.assertEqual(code, 2, reply)
                        self.assertFalse(reply["success"])
                        self.assertTrue(reply.get("error"))
                        child.assert_not_called()


if __name__ == "__main__":
    unittest.main()
