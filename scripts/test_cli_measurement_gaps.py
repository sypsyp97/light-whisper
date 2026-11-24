"""Artifact verification boundaries and CLI regressions."""

import struct
import unittest

from scripts import test_build_cli as packaging

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import sys
import threading
import tracemalloc
from unittest import mock

from scripts import test_cli_commands as commands
from scripts import test_cli_session as sessions
from scripts import test_cli_target as targets
from scripts import test_light_whisper_cli as support


def setUpModule():
    support.setUpModule()


SENDER_EXIT_PROBE = r'''
import hashlib, json, os
from pathlib import Path
import runpy, sys, threading

script, report = map(Path, sys.argv[1:])
# Do not acquire a backend until the parent has assigned this process to its job.
if os.read(sys.stdin.fileno(), 1) != b"S":
    raise RuntimeError("missing owned-process start gate")
released = threading.Event()
held = threading.Event()
owners = []

def control():
    os.read(sys.stdin.fileno(), 1)
    released.set()

threading.Thread(target=control, daemon=True).start()

def schedule(frame, event, value):
    if frame.f_code.co_filename == str(script):
        if (event == "return" and frame.f_code.co_name == "write"
                and frame.f_locals["message"]["action"] == "status"):
            owners.append(frame.f_locals["self"])
            held.set()
            # Finite scheduler delay only. The parent normally releases this
            # gate sooner; the watchdog prevents an orphan if the parent fails.
            released.wait(timeout=12)
        elif event == "return" and frame.f_code.co_name == "main":
            sys.settrace(None)
            client = owners[0] if owners else None
            proof = {
                "event": "main_returned", "return_code": value,
                "sender_post_publication_held": held.is_set() and not released.is_set(),
                "backend_reaped": client is not None and client.child.poll() is not None,
                "pipes_closed": client is not None and client.child.stdin.closed and client.child.stdout.closed,
                "sender_alive": client is not None and client.sender.is_alive(),
                "reader_alive": client is not None and client.reader.is_alive(),
                "source_sha256": hashlib.sha256(script.read_bytes()).hexdigest(),
            }
            report.write_text(json.dumps(proof), encoding="utf-8")
            print(json.dumps(proof), file=sys.stderr, flush=True)
    return schedule

threading.settrace(schedule)
sys.settrace(schedule)
sys.argv = [str(script), "--python", sys.executable, "--timeout", "1", "status"]
runpy.run_path(str(script), run_name="__main__")
'''


class ArtifactSignatureTests(unittest.TestCase):
    def setUp(self):
        self.fixture=packaging.CliArtifactTests("runTest")
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()

    def verify(self,payload):
        self.fixture.exe.write_bytes(packaging.executable("cli",packaging.FIRST,payload))
        return self.fixture.builder.verify_cli(self.fixture.exe)

    def test_stamped_image_with_invalid_pe_signature_is_rejected(self):
        payload=bytearray(packaging.pe_image())
        offset=struct.unpack_from("<I",payload,0x3c)[0]
        for signature in (b"PX\0\0",b"NE\0\0",b"PE\1\0",b"\0"*4):
            with self.subTest(signature=signature),self.assertRaises(ValueError):
                payload[offset:offset+4]=signature
                self.verify(payload)

    def test_stamped_image_with_low_pe_offset_is_rejected(self):
        for offset in (0,1,63):
            with self.subTest(offset=offset),self.assertRaises(ValueError):
                payload=bytearray(packaging.pe_image())
                struct.pack_into("<I",payload,0x3c,offset)
                self.verify(payload)

    def test_stamped_image_with_boundary_pe_offset_is_accepted(self):
        original=packaging.pe_image()
        payload=bytearray(original[:64]+original[128:])
        struct.pack_into("<I",payload,0x3c,64)
        report=self.verify(payload)
        self.assertEqual(report["size"],self.fixture.exe.stat().st_size)


class PublicCliGapTests(support.BackendCase):
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

    def test_equals_form_app_target_rejects_independent_options_before_child(self):
        selectors = (
            ("--engine", "qwen3-asr-0.6b"),
            ("--python", support.BACKEND_PYTHON),
            ("--engine-exe", support.BACKEND_PYTHON),
            ("--data-dir", str(self.directory / "runtime")),
        )
        for selector in selectors:
            with self.subTest(selector=selector), \
                 mock.patch.object(support.cli, "EngineClient") as child:
                code, reply = self.single(["--target=app", *selector, "doctor"])
            self.assertEqual(code, 2, reply)
            self.assertIs(reply["success"], False)
            self.assertTrue(reply.get("error"))
            child.assert_not_called()
        completed = subprocess.run(
            [support.BACKEND_PYTHON, str(support.ROOT / "scripts/light_whisper_cli.py"),
             "--target=app", "--engine", "qwen3-asr-0.6b", "doctor"],
            capture_output=True, timeout=5, env=self.environment(),
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        self.assertEqual(completed.returncode, 2, completed.stdout)
        replies = [json.loads(line) for line in completed.stdout.splitlines()]
        self.assertEqual(len(replies), 1)
        self.assertIs(replies[0]["success"], False)
        self.assertTrue(replies[0].get("error"))
        self.assertFalse(self.record.exists())

    def test_truthy_nonstring_audio_recovers_without_acquiring_child(self):
        for audio in (True, 1, 1.0, [1], {"x": 1}):
            with self.subTest(audio=audio), \
                 mock.patch.object(support.cli, "EngineClient") as child:
                code, replies, _ = self.invoke(
                    ["--python", support.BACKEND_PYTHON, "session"],
                    [{"request_id": 10, "action": "transcribe", "audio": audio},
                     {"request_id": 11, "action": "exit"}],
                )
            self.assertEqual(code, 0, replies)
            self.assertEqual([reply.get("request_id") for reply in replies], [10, 11])
            self.assertIs(replies[0]["success"], False)
            self.assertTrue(replies[0].get("error"))
            self.assertEqual(replies[1], {"success": True, "request_id": 11})
            child.assert_not_called()

    def test_truthy_nonstring_audio_preserves_owner_and_next_backend_identity(self):
        invalid = [True, 1, 1.0, [1], {"x": 1}]
        records = [{"request_id": 20, "action": "status"}]
        records += [{"request_id": 21 + index, "action": "transcribe", "audio": audio}
                    for index, audio in enumerate(invalid)]
        records += [{"request_id": 30, "action": "stats"},
                    {"request_id": 31, "action": "exit"}]
        with self.launch_fake():
            code, replies, _ = self.invoke(
                ["--python", support.BACKEND_PYTHON, "--timeout", "1", "session"], records,
            )
        self.assertEqual(code, 0, replies)
        self.assertEqual([reply.get("request_id") for reply in replies],
                         [record["request_id"] for record in records])
        self.assertTrue(replies[0]["success"])
        self.assertTrue(all(reply["success"] is False and reply.get("error")
                            for reply in replies[1:-2]))
        self.assertTrue(replies[-2]["success"])
        self.assertEqual(replies[0]["pid"], replies[-2]["pid"])
        self.assertEqual(replies[-1], {"success": True, "request_id": 31})
        self.assertEqual([(request["action"], request["request_id"])
                          for request in self.requests()],
                         [("status", 1), ("stats", 2), ("exit", 3)])
        self.assert_closed()

    def test_only_boolean_true_fatal_terminates_owned_session(self):
        # Rust emits a boolean fatal flag. Numeric/string values exercise the
        # Python client's defensive parser, not a conforming Rust error emitter.
        self.script.write_text(commands.BROKER_BACKEND.replace(
            'result["fatal"] = False',
            'result["fatal"] = json.loads(os.environ["CLI_GAP_FATAL"])',
        ), encoding="utf-8")
        for present, fatal in ((False, None), (True, False), (True, 0),
                               (True, 1), (True, 1.0), (True, "true"), (True, True)):
            self.record.unlink(missing_ok=True)
            source = commands.CountedInput([
                {"request_id": 17, "action": "status"},
                {"request_id": 18, "action": "stats"},
            ])
            output = io.StringIO()
            mode = "recoverable_false" if present else "recoverable_missing"
            with self.subTest(present=present, fatal=fatal), \
                 mock.patch.dict(os.environ, {"CLI_GAP_FATAL": json.dumps(fatal)}), \
                 self.launch_fake(mode), mock.patch.object(sys, "stdin", source), \
                 contextlib.redirect_stdout(output):
                code = support.cli.main([
                    "--app-exe", str(self.app), "--timeout", "1", "session",
                ])
            replies = [json.loads(line) for line in output.getvalue().splitlines()]
            if fatal is True:
                self.assertEqual(code, 1)
                self.assertEqual([reply.get("request_id") for reply in replies], [17])
                self.assertEqual(source.consumed, 1)
                expected = [("status", 1), ("exit", 2)]
            else:
                self.assertEqual(code, 0, replies)
                self.assertEqual([reply.get("request_id") for reply in replies], [17, 18])
                self.assertEqual(source.consumed, 2)
                self.assertTrue(replies[1]["success"])
                self.assertEqual("fatal" in replies[0], present)
                if present:
                    self.assertIs(type(replies[0]["fatal"]), type(fatal))
                    self.assertEqual(replies[0]["fatal"], fatal)
                expected = [("status", 1), ("stats", 2), ("exit", 3)]
            self.assertIs(replies[0]["success"], False)
            self.assertTrue(replies[0].get("error"))
            self.assertEqual([(request["action"], request["request_id"])
                              for request in self.requests()], expected)
            self.assertEqual(len(self.launches), 1)
            self.assert_closed()

    def test_doctor_scan_allocation_does_not_grow_with_unmarked_file_size(self):
        paths = [self.directory / f"unmarked-{size}-MiB.exe" for size in (1, 8)]
        block = b"\0" * (64 * 1024)
        for path, size in zip(paths, (1, 8)):
            with path.open("wb") as stream:
                for _ in range(size * 16):
                    stream.write(block)
        # Warm argparse/path caches before tracking; fixture bytes stay outside
        # tracking. Doctor must report unsupported files without launching them.
        with mock.patch.object(support.cli, "EngineClient") as child:
            self.single(["--app-exe", str(paths[0]), "doctor"])
            self.scan_peaks = []
            for path in paths:
                tracemalloc.start()
                try:
                    code, reply = self.single(["--app-exe", str(path), "doctor"])
                    self.scan_peaks.append(tracemalloc.get_traced_memory()[1])
                finally:
                    tracemalloc.stop()
                self.assertEqual(code, 0, reply)
                self.assertIs(reply["app_protocol_supported"], False)
                self.assertIs(reply["model_load_checked"], False)
            child.assert_not_called()
        # 512 KiB allows eight current 64 KiB read buffers plus parser/allocator
        # variation, while exposing retention of the additional 7 MiB of input.
        tolerance = 512 * 1024
        self.assertLessEqual(self.scan_peaks[1], self.scan_peaks[0] + tolerance,
                             f"1 MiB/8 MiB doctor allocation peaks: {self.scan_peaks}")

    def test_startup_error_preserves_visible_unicode_in_decoded_outer_json(self):
        message = "启动失败：测试引擎"
        self.script.write_text(sessions.SESSION_BACKEND.replace(
            '"phase": "startup"', '"phase": "startup", "error": ' + json.dumps(message),
        ), encoding="utf-8")
        with self.launch_fake("startup_false"):
            code, reply = self.single([
                "--python", support.BACKEND_PYTHON, "--timeout", "1", "status",
            ])
        self.assertEqual(code, 1)
        self.assertIs(reply["success"], False)
        self.assertIn(message, reply.get("error", ""))
        self.assertEqual([request["action"] for request in self.requests()], ["exit"])
        self.assert_closed()

    def test_script_exits_after_bounded_cleanup_with_sender_at_publication_barrier(self):
        # Execute the actual CLI __main__, with a real EngineClient and a
        # maintained synthetic backend. Only scheduling at the write target's
        # return event changes; write/flush and queue publication already ended.
        fixture = self.directory / "sender-exit-cli"
        script = fixture / "scripts/light_whisper_cli.py"
        backend = fixture / "src-tauri/resources/engine.py"
        script.parent.mkdir(parents=True)
        backend.parent.mkdir(parents=True)
        shutil.copyfile(support.ROOT / "scripts/light_whisper_cli.py", script)
        backend.write_text(sessions.SESSION_BACKEND, encoding="utf-8")
        probe = self.directory / "sender-exit-probe.py"
        probe.write_text(SENDER_EXIT_PROBE, encoding="utf-8")
        proof_path = self.directory / "sender-exit-proof.json"
        job = None
        if os.name == "nt":
            spec = importlib.util.spec_from_file_location(
                "cli_gap_owned_host", support.ROOT / "formal/agent-api-host/run.py",
            )
            host = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(host)
            job = host.Job()
        helper = subprocess.Popen(
            [support.BACKEND_PYTHON, str(probe), str(script), str(proof_path)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=self.environment(), creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
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
                raise RuntimeError("CLI probe exited before reaching the cleanup observation")
            reader.join(timeout=1)
            self.assertFalse(reader.is_alive())
            self.assertEqual(proof["return_code"], 1, proof)
            self.assertTrue(proof["sender_post_publication_held"], proof)
            self.assertTrue(proof["backend_reaped"], proof)
            self.assertTrue(proof["pipes_closed"], proof)
            self.assertTrue(proof["sender_alive"], proof)
            self.assertFalse(proof["reader_alive"], proof)
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
            self.sender_exit_observation = {
                **proof, "helper_exit_before_sender_release": exited_before_release,
                "sender_released_by_controller": released_by_controller,
                "helper_exit_code": helper.returncode, "controller_terminated": False,
            }
            self.assertTrue(exited_before_release,
                            "CLI process waited for the gated sender after bounded owner cleanup")
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
                raise RuntimeError("CLI probe infrastructure required controller termination")


if __name__ == "__main__":
    unittest.main()
