"""Additional public CLI gaps from the frozen 903-result partial snapshot."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import queue
import runpy
import shutil
import subprocess
import sys
import threading
import unittest
from unittest import mock

from scripts import test_cli_measurement_gaps as accepted
from scripts import test_cli_session as sessions
from scripts import test_cli_target as targets
from scripts import test_light_whisper_cli as support


def setUpModule():
    support.setUpModule()


def reader_probe_source():
    # Reuse the accepted finite scheduler/controller. Change only the production
    # preemption point: _read_stdout has published EOF outside all queue locks.
    sender_boundary = ('(event == "return" and frame.f_code.co_name == "write"\n'
                       '                and frame.f_locals["message"]["action"] == "status")')
    if accepted.SENDER_EXIT_PROBE.count(sender_boundary) != 1:
        raise RuntimeError("accepted scheduler fixture changed")
    return accepted.SENDER_EXIT_PROBE.replace(
        sender_boundary, '(event == "return" and frame.f_code.co_name == "_read_stdout")',
    ).replace("sender_post_publication_held", "reader_after_eof_held")


class FinalSurvivorGapTests(support.BackendCase):
    invoke = targets.TargetTests.invoke
    single = targets.TargetTests.single
    requests = targets.TargetTests.requests
    launch_fake = targets.TargetTests.launch_fake
    assert_closed = targets.TargetTests.assert_closed

    def setUp(self):
        super().setUp()
        self.app = self.directory / "synthetic app.exe"
        self.app.write_bytes(b"test-only LIGHT_WHISPER_AGENT_PROTOCOL_1")
        repository = self.directory / "repository"
        repository.mkdir()
        (repository / "package.json").write_bytes((support.ROOT / "package.json").read_bytes())
        for patch in (
            mock.patch.object(support.cli, "ROOT", repository),
            mock.patch.dict(os.environ, {"LOCALAPPDATA": str(self.directory / "local-appdata")}),
        ):
            patch.start()
            self.addCleanup(patch.stop)

    def test_dynamic_engine_target_rejects_app_executable_before_child(self):
        with mock.patch.object(support.cli, "EngineClient") as child:
            code, reply = self.single([
                "--target=engine", "--app-exe", str(self.app), "doctor",
            ])
        self.assertEqual(code, 2, reply)
        self.assertIs(reply["success"], False)
        self.assertTrue(reply.get("error"))
        child.assert_not_called()

    def test_existing_default_python_with_missing_script_rejects_before_child(self):
        missing = self.directory / "missing-engine.py"
        with mock.patch.object(support.cli, "DEFAULT_PYTHON", Path(support.BACKEND_PYTHON)), \
             mock.patch.object(support.cli, "ENGINE_SCRIPT", missing), \
             mock.patch.object(support.cli, "EngineClient",
                               side_effect=AssertionError("child acquired before script validation")) as child:
            code, reply = self.single(["--target=engine", "status"])
        self.assertEqual(code, 2, reply)
        self.assertIs(reply["success"], False)
        self.assertTrue(reply.get("error"))
        child.assert_not_called()

    def copied_cli(self, name):
        root = self.directory / name
        script = root / "scripts/light_whisper_cli.py"
        backend = root / "src-tauri/resources/engine.py"
        script.parent.mkdir(parents=True)
        backend.parent.mkdir(parents=True)
        shutil.copyfile(support.ROOT / "scripts/light_whisper_cli.py", script)
        backend.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        (root / "package.json").write_bytes((support.ROOT / "package.json").read_bytes())
        return script

    def test_runpy_non_main_names_do_not_run_cli_or_acquire_child(self):
        script = self.copied_cli("import-only-cli")
        for run_name in (None, "__cli_debug__"):
            output, errors = io.StringIO(), io.StringIO()
            failure = None
            namespace = None
            options = {} if run_name is None else {"run_name": run_name}
            with self.subTest(run_name=run_name), \
                 mock.patch.object(sys, "argv", [str(script), "--app-exe", str(self.app), "doctor"]), \
                 mock.patch.object(subprocess, "Popen") as child, \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                try:
                    namespace = runpy.run_path(str(script), **options)
                except SystemExit as error:
                    failure = error
                self.assertIsNone(failure, "non-main runpy load executed the CLI entrypoint")
                self.assertEqual(output.getvalue(), "", "non-main runpy load emitted CLI output")
                self.assertEqual(errors.getvalue(), "")
                self.assertTrue(callable(namespace.get("main")))
                self.assertEqual(namespace["__name__"], "<run_path>" if run_name is None else run_name)
                child.assert_not_called()

    def test_script_exits_after_cleanup_with_reader_held_after_eof_publication(self):
        script = self.copied_cli("reader-exit-cli")
        probe = self.directory / "reader-exit-probe.py"
        probe.write_text(reader_probe_source(), encoding="utf-8")
        proof_path = self.directory / "reader-exit-proof.json"
        job = None
        if os.name == "nt":
            spec = importlib.util.spec_from_file_location(
                "cli_final_gap_owned_host", support.ROOT / "formal/agent-api-host/run.py",
            )
            host = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(host)
            job = host.Job()
        helper = subprocess.Popen(
            [support.BACKEND_PYTHON, str(probe), str(script), str(proof_path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=self.environment("request_eof"),
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            start_new_session=os.name != "nt",
        )
        messages = queue.Queue()

        def read_proof():
            for line in helper.stderr:
                try:
                    value = json.loads(line)
                except ValueError:
                    continue
                if isinstance(value, dict) and value.get("event") == "main_returned":
                    messages.put(value)
                    return
            messages.put(None)

        reader = threading.Thread(target=read_proof, daemon=True)
        reader.start()
        terminated_by_controller = False
        released_by_controller = False
        try:
            if job is not None:
                job.add(helper)
            helper.stdin.write(b"S")
            helper.stdin.flush()
            proof = messages.get(timeout=8)
            if proof is None:
                raise RuntimeError("CLI probe exited before reaching the reader cleanup observation")
            reader.join(timeout=1)
            self.assertFalse(reader.is_alive())
            self.assertEqual(proof["return_code"], 1, proof)
            self.assertTrue(proof["reader_after_eof_held"], proof)
            self.assertTrue(proof["backend_reaped"], proof)
            self.assertTrue(proof["pipes_closed"], proof)
            self.assertFalse(proof["sender_alive"], proof)
            self.assertTrue(proof["reader_alive"], proof)
            try:
                helper.wait(timeout=2)
                exited_before_release = True
            except subprocess.TimeoutExpired:
                exited_before_release = False
                helper.stdin.write(b"R")
                helper.stdin.flush()
                released_by_controller = True
            output, _ = helper.communicate(timeout=5)
            replies = [json.loads(line) for line in output.splitlines()]
            self.assertEqual(helper.returncode, 1)
            self.assertEqual(len(replies), 1)
            self.assertIs(replies[0]["success"], False)
            self.assertTrue(replies[0].get("error"))
            self.assertEqual([request["action"] for request in self.requests()], ["status"])
            self.reader_exit_observation = {
                **proof, "helper_exit_before_reader_release": exited_before_release,
                "reader_released_by_controller": released_by_controller,
                "helper_exit_code": helper.returncode, "controller_terminated": False,
            }
            self.assertTrue(exited_before_release,
                            "CLI process waited for the gated reader after bounded owner cleanup")
        finally:
            if helper.poll() is None:
                try:
                    helper.stdin.write(b"R")
                    helper.stdin.flush()
                    helper.wait(timeout=5)
                except (OSError, subprocess.TimeoutExpired):
                    terminated_by_controller = True
                    if job is not None:
                        job.close()
                    elif os.name != "nt":
                        import signal
                        os.killpg(helper.pid, signal.SIGKILL)
                    else:
                        helper.kill()
                    helper.wait(timeout=2)
            reader.join(timeout=2)
            if job is not None:
                job.close()
            if terminated_by_controller:
                raise RuntimeError("reader CLI probe infrastructure required controller termination")

    def test_scaled_partial_raw_writes_preserve_complete_jsonl_requests(self):
        # Explicit fault model: scale CPython's documented per-write cap to
        # eight bytes. These are real OS writes returning real short counts;
        # this does not claim small blocking Windows pipe writes do so unaided.
        cap = 8
        original_popen = subprocess.Popen
        writes, stacks, hooked = [], [], []

        def launch(*args, **kwargs):
            child = original_popen(*args, **kwargs)
            self.assertIs(type(child.stdin), io.TextIOWrapper)
            buffer = child.stdin.buffer
            raw = getattr(buffer, "raw", buffer)
            self.assertIs(type(raw), io.FileIO)
            original_write = raw.write
            stacks.append(type(buffer).__name__)

            def short_write(data):
                count = original_write(data[:cap])
                writes.append({"requested": len(data), "written": count})
                return count

            raw.write = short_write
            hooked.append(raw)
            return child

        client = self.client(timeout=1)
        response = failure = None
        marker = "synthetic complete JSONL request"
        try:
            with mock.patch.object(subprocess, "Popen", side_effect=launch):
                try:
                    with client:
                        response = client.request("status", marker=marker)
                except Exception as error:
                    failure = error
        finally:
            for raw in hooked:
                del raw.write
        self.assert_reaped(client)
        self.assertTrue(client.child.stdin.closed and client.child.stdout.closed)
        self.assertTrue(all(worker is None or not worker.is_alive()
                            for worker in (client.sender, client.reader)))
        self.partial_write_observation = {
            "kind": "SCALED_INJECTED_PARTIAL_WRITE_FAULT_MODEL", "cap_bytes": cap,
            "actual_stdin_buffer_types": stacks, "real_raw_write_returns": writes,
            "public_failure_type": type(failure).__name__ if failure is not None else None,
        }
        self.assertTrue(any(item["requested"] > cap for item in writes),
                        "partial-write fault schedule never intercepted a larger write")
        self.assertIsNone(failure, "injected short raw writes truncated the public JSONL request")
        self.assertIs(response["success"], True)
        self.assertEqual(response["request"], {"action": "status", "request_id": 1, "marker": marker})
        self.assertEqual([request["action"] for request in self.requests()], ["status", "exit"])

    def test_noninterned_engine_command_dispatches_get_and_set_wire_actions(self):
        self.script.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        cases = ((["get"], "engine_get", {}),
                 (["set", "qwen3-asr-0.6b"], "engine_set", {"engine": "qwen3-asr-0.6b"}))
        for command, action, fields in cases:
            self.record.unlink(missing_ok=True)
            engine_token = "".join(("en", "gine"))
            with self.subTest(command=command), self.launch_fake():
                code, reply = self.single([
                    "--target=app", "--app-exe", str(self.app), "--timeout", "1",
                    engine_token, *command,
                ])
                self.assertEqual(code, 0, reply)
                self.assertIs(reply["success"], True)
                self.assertEqual(reply["request"], {"action": action, "request_id": 1, **fields})
                self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
                self.assertEqual([request["action"] for request in self.requests()], [action, "exit"])
                self.assert_closed()

    def test_dynamic_app_session_accepts_desktop_records_and_keeps_one_owner(self):
        self.script.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        records = [
            {"request_id": -7, "action": "models"},
            {"request_id": 2 ** 80, "action": "engine_get"},
            {"request_id": 41, "action": "engine_set", "engine": "qwen3-asr-0.6b"},
            {"request_id": 42, "action": "exit"},
        ]
        with self.launch_fake():
            code, replies, _ = self.invoke([
                "--target=app", "--app-exe", str(self.app), "--timeout", "1", "session",
            ], records)
        self.assertEqual(code, 0, replies)
        self.assertEqual([reply.get("request_id") for reply in replies],
                         [record["request_id"] for record in records])
        self.assertTrue(all(reply["success"] for reply in replies), replies)
        self.assertEqual(len({reply["pid"] for reply in replies[:-1]}), 1)
        expected = [{**record, "request_id": index + 1} for index, record in enumerate(records)]
        self.assertEqual(self.requests(), expected)
        self.assertEqual(self.launches[0][0], [str(self.app.resolve()), "--agent-client"])
        self.assert_closed()
        with mock.patch.object(support.cli, "EngineClient") as child:
            code, rejected, _ = self.invoke(["--target=engine", "session"], records)
        self.assertEqual(code, 0, rejected)
        self.assertEqual([reply.get("request_id") for reply in rejected],
                         [record["request_id"] for record in records])
        self.assertTrue(all(reply["success"] is False and reply.get("error") for reply in rejected[:-1]))
        self.assertEqual(rejected[-1], {"success": True, "request_id": 42})
        child.assert_not_called()


if __name__ == "__main__":
    unittest.main()
