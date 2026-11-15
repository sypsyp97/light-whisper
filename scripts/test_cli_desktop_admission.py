"""Desktop preflight through real CLI callers and EngineClient serialization.

The in-memory peer records writes and replies successfully to every record. It
does not emulate Rust validation: inadmissible input must never reach this peer.
No desktop, subprocess, audio decoder, model, or network is used.
"""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
from types import SimpleNamespace
import sys
import tempfile
import unittest
from unittest import mock


SOURCE_ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "desktop_admission_cli_under_test", SOURCE_ROOT / "scripts/light_whisper_cli.py")
cli = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cli)
REQUEST_MAX = 65536  # src-tauri/src/agent_api/transport.rs, includes the newline.
SCALAR = "中文😀 café"
CALLER_ID = -(2 ** 2048 + 7)


def wire_line(message):
    raw = io.BytesIO()
    with io.TextIOWrapper(raw, encoding="utf-8", errors="replace",
                          write_through=True, line_buffering=True) as text:
        text.write(json.dumps(message, ensure_ascii=True) + "\n")
        text.flush()
        return raw.getvalue()


class RecordingPipe(io.BytesIO):
    """Record physical pipe bytes after the production text wrapper translates."""

    def __init__(self, peer):
        super().__init__()
        self.peer = peer
        self.position = 0

    def flush(self):
        data = self.getvalue()
        while (end := data.find(b"\n", self.position)) >= 0:
            self.peer.record(data[self.position:end + 1])
            self.position = end + 1
        super().flush()


class RecordingPeer:
    """Existing request_client seam; retain production request/send/receive."""

    def __init__(self):
        self.opens = 0
        self.closes = 0
        self.lines = []
        self.client = None

    @contextlib.contextmanager
    def open(self, args):
        self.opens += 1
        client = cli.EngineClient(["test-only-memory-peer"], args.timeout)
        self.client = client
        pipe = io.TextIOWrapper(RecordingPipe(self), encoding="utf-8", errors="replace",
                                write_through=True, line_buffering=True)
        client.child = SimpleNamespace(stdin=pipe)
        client.ready = True
        try:
            yield client
        finally:
            client.ready = False
            pipe.close()
            self.closes += 1

    def record(self, line):
        self.lines.append(line)
        message = json.loads(line)
        self.client.messages.put(json.dumps({
            "success": True, "request_id": message["request_id"], "request": message,
        }, ensure_ascii=True) + "\n")

    def snapshot(self):
        return self.opens, self.closes, len(self.lines)


class RecordingInput:
    """Snapshot side effects before the next input is requested, without sleeps."""

    def __init__(self, records, peer):
        self.records = records
        self.peer = peer
        self.before = []

    def __iter__(self):
        for record in self.records:
            self.before.append(self.peer.snapshot())
            yield json.dumps(record, ensure_ascii=True) + "\n"


class DesktopAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cli-desktop-admission-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.audio = self.directory / "synthetic 中文.wav"
        self.audio.write_bytes(b"test-only audio; never decoded")
        patch = mock.patch.dict(os.environ, {
            "LIGHT_WHISPER_DATA_DIR": str(self.directory / "owned-config"),
            "HF_HOME": str(self.directory / "owned-models"),
        })
        patch.start()
        self.addCleanup(patch.stop)

    def transcribe(self, **options):
        return {"request_id": CALLER_ID, "action": "transcribe",
                "audio": str(self.audio), **options}

    def invoke(self, arguments, records=()):
        peer = RecordingPeer()
        source = RecordingInput(records, peer)
        output, errors = io.StringIO(), io.StringIO()
        with mock.patch.object(cli, "request_client", side_effect=peer.open), \
             mock.patch.object(sys, "stdin", source), \
             contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = cli.main(arguments)
        replies = [json.loads(line) for line in output.getvalue().splitlines()]
        self.assertTrue(all(type(reply.get("success")) is bool for reply in replies))
        self.assertEqual(errors.getvalue(), "")
        return code, replies, peer, source

    def oneshot_arguments(self, **fields):
        arguments = ["--target", "app", "transcribe", str(self.audio)]
        for key in ("language", "context"):
            if key in fields:
                arguments += ["--" + key, fields[key]]
        for word in fields.get("hot_words", []):
            arguments += ["--hot-word", word]
        return arguments

    def context_for_wire_size(self, size, internal_id):
        message = {"audio_path": str(self.audio.resolve()), "options": {"context": ""},
                   "action": "transcribe", "request_id": internal_id}
        context = "x" * (size - len(wire_line(message)))
        message["options"]["context"] = context
        self.assertEqual(len(wire_line(message)), size)
        return context

    def assert_input_error(self, reply, request_id=None):
        self.assertFalse(reply["success"])
        self.assertIsInstance(reply.get("error"), str)
        self.assertTrue(reply["error"].strip())
        if request_id is not None:
            self.assertIs(type(reply.get("request_id")), int)
            self.assertEqual(reply["request_id"], request_id)

    def invalid_fields(self):
        return [
            {"context": "中" * 11000},
            {"context": "😀" * 6000},
            {"context": '"\\' * 17000},
            {"hot_words": ["中" * 11000]},
            {"context": "\ud800"},
            {"context": "\udfff"},
            {"language": "prefix\ud800suffix"},
            {"hot_words": [SCALAR, "\udfff"]},
        ]

    def test_oneshot_invalid_desktop_input_is_json_exit_2_before_acquisition(self):
        for fields in self.invalid_fields():
            with self.subTest(fields=[(key, ascii(value)[:70]) for key, value in fields.items()]):
                code, replies, peer, _ = self.invoke(self.oneshot_arguments(**fields))
                self.assertEqual(code, 2)
                self.assertEqual(len(replies), 1)
                self.assert_input_error(replies[0])
                self.assertEqual(peer.snapshot(), (0, 0, 0))

    def test_first_invalid_session_record_is_recoverable_and_does_not_start_broker(self):
        for fields in self.invalid_fields():
            with self.subTest(fields=[(key, ascii(value)[:70]) for key, value in fields.items()]):
                records = [self.transcribe(**fields),
                           {"request_id": -19, "action": "status"},
                           {"request_id": 31, "action": "exit"}]
                code, replies, peer, source = self.invoke(["--target", "app", "session"], records)
                self.assertEqual(code, 0)
                self.assertEqual(len(replies), 3)
                self.assert_input_error(replies[0], CALLER_ID)
                self.assertEqual(source.before[1], (0, 0, 0))
                self.assertEqual([reply["request_id"] for reply in replies], [CALLER_ID, -19, 31])
                self.assertTrue(all(reply["success"] for reply in replies[1:]))
                self.assertEqual(peer.snapshot(), (1, 1, 1))
                self.assertEqual(json.loads(peer.lines[0])["request_id"], 1)

    def test_invalid_record_after_start_does_not_send_or_retire_live_client(self):
        for fields in self.invalid_fields():
            with self.subTest(fields=[(key, ascii(value)[:70]) for key, value in fields.items()]):
                records = [{"request_id": 18, "action": "status"},
                           self.transcribe(**fields),
                           {"request_id": 18, "action": "stats"},
                           {"request_id": -7, "action": "exit"}]
                code, replies, peer, source = self.invoke(["--target", "app", "session"], records)
                self.assertEqual(code, 0)
                self.assertEqual(len(replies), 4)
                self.assert_input_error(replies[1], CALLER_ID)
                self.assertEqual(source.before[2], (1, 0, 1))
                self.assertEqual([reply["request_id"] for reply in replies], [18, CALLER_ID, 18, -7])
                self.assertTrue(all(replies[index]["success"] for index in (0, 2, 3)))
                self.assertEqual(peer.snapshot(), (1, 1, 2))
                requests = [json.loads(line) for line in peer.lines]
                self.assertEqual([request["action"] for request in requests], ["status", "stats"])
                self.assertEqual([request["request_id"] for request in requests], [1, 2])

    def test_oneshot_exact_wire_boundary_includes_newline(self):
        for size, expected_code in ((REQUEST_MAX, 0), (REQUEST_MAX + 1, 2)):
            with self.subTest(size=size):
                context = self.context_for_wire_size(size, 1)
                code, replies, peer, _ = self.invoke(self.oneshot_arguments(context=context))
                self.assertEqual(code, expected_code)
                self.assertEqual(len(replies), 1)
                if expected_code:
                    self.assert_input_error(replies[0])
                    self.assertEqual(peer.snapshot(), (0, 0, 0))
                else:
                    self.assertTrue(replies[0]["success"])
                    self.assertEqual(len(peer.lines[0]), REQUEST_MAX)

    def test_session_exact_boundary_uses_internal_next_id_and_ignores_caller_width(self):
        for warmups in (0, 8, 9):
            for size in (REQUEST_MAX, REQUEST_MAX + 1):
                with self.subTest(warmups=warmups, size=size):
                    context = self.context_for_wire_size(size, warmups + 1)
                    records = [{"request_id": CALLER_ID, "action": "status"}] * warmups
                    records += [self.transcribe(context=context),
                                {"request_id": CALLER_ID, "action": "stats"},
                                {"request_id": 23, "action": "exit"}]
                    code, replies, peer, source = self.invoke(["--target", "app", "session"], records)
                    self.assertEqual(code, 0)
                    self.assertEqual(len(replies), warmups + 3)
                    self.assertEqual([reply["request_id"] for reply in replies],
                                     [CALLER_ID] * (warmups + 2) + [23])
                    accepted = size == REQUEST_MAX
                    self.assertEqual(replies[warmups]["success"], accepted)
                    if not accepted:
                        self.assert_input_error(replies[warmups], CALLER_ID)
                        self.assertEqual(source.before[warmups + 1],
                                         (int(warmups > 0), 0, warmups))
                    self.assertTrue(replies[warmups + 1]["success"])
                    requests = [json.loads(line) for line in peer.lines]
                    self.assertEqual([request["request_id"] for request in requests],
                                     list(range(1, warmups + int(accepted) + 2)))
                    self.assertEqual(peer.snapshot(), (1, 1, warmups + int(accepted) + 1))
                    if accepted:
                        self.assertEqual(len(peer.lines[warmups]), REQUEST_MAX)

    def test_same_payload_fits_internal_id_9_and_is_rejected_at_id_10(self):
        context = self.context_for_wire_size(REQUEST_MAX, 9)
        for warmups, accepted in ((8, True), (9, False)):
            with self.subTest(warmups=warmups):
                records = [{"request_id": CALLER_ID, "action": "status"}] * warmups
                records += [self.transcribe(context=context),
                            {"request_id": 4, "action": "status"},
                            {"request_id": 5, "action": "exit"}]
                code, replies, peer, _ = self.invoke(["--target", "app", "session"], records)
                self.assertEqual(code, 0)
                self.assertEqual(replies[warmups]["success"], accepted)
                self.assertTrue(replies[warmups + 1]["success"])
                self.assertEqual(len(peer.lines), warmups + int(accepted) + 1)

    def test_valid_scalar_unicode_keeps_values_on_both_desktop_callers(self):
        fields = {"language": SCALAR, "context": SCALAR,
                  "hot_words": [SCALAR, "", "literal \\ud800", SCALAR]}
        for surface in ("oneshot", "session"):
            with self.subTest(surface=surface):
                arguments = self.oneshot_arguments(**fields) if surface == "oneshot" else ["session"]
                records = () if surface == "oneshot" else [self.transcribe(**fields),
                                                           {"request_id": 7, "action": "exit"}]
                code, replies, peer, _ = self.invoke(arguments, records)
                self.assertEqual(code, 0)
                self.assertTrue(all(reply["success"] for reply in replies))
                request = json.loads(peer.lines[0])
                self.assertEqual(request["options"], {"language": SCALAR, "context": SCALAR})
                self.assertEqual(request["hot_words"], fields["hot_words"])
                self.assertEqual(request["request_id"], 1)
                self.assertLess(len(peer.lines[0]), REQUEST_MAX)

    def test_explicit_equals_target_app_oneshot_rejects_invalid_input_before_acquisition(self):
        for fields in self.invalid_fields():
            with self.subTest(fields=[(key, ascii(value)[:70]) for key, value in fields.items()]):
                arguments = ["--target=app", *self.oneshot_arguments(**fields)[2:]]
                code, replies, peer, _ = self.invoke(arguments)
                self.assertEqual(code, 2)
                self.assertEqual(len(replies), 1)
                self.assert_input_error(replies[0])
                self.assertEqual(peer.snapshot(), (0, 0, 0))

    def test_explicit_equals_target_app_session_rejects_invalid_input_and_recovers(self):
        for fields in self.invalid_fields():
            with self.subTest(fields=[(key, ascii(value)[:70]) for key, value in fields.items()]):
                records = [self.transcribe(**fields),
                           {"request_id": -19, "action": "status"},
                           {"request_id": 31, "action": "exit"}]
                code, replies, peer, source = self.invoke(["--target=app", "session"], records)
                self.assertEqual(code, 0)
                self.assertEqual(len(replies), 3)
                self.assert_input_error(replies[0], CALLER_ID)
                self.assertEqual(source.before[1], (0, 0, 0))
                self.assertEqual([reply["request_id"] for reply in replies], [CALLER_ID, -19, 31])
                self.assertTrue(all(reply["success"] for reply in replies[1:]))
                self.assertEqual(peer.snapshot(), (1, 1, 1))
                request = json.loads(peer.lines[0])
                self.assertEqual(request["action"], "status")
                self.assertEqual(request["request_id"], 1)

    def test_independent_engine_transcribe_preserves_scalar_context_beyond_desktop_limit(self):
        context = SCALAR + "x" * REQUEST_MAX
        arguments = ["--target=engine", *self.oneshot_arguments(context=context)[2:]]
        code, replies, peer, _ = self.invoke(arguments)
        self.assertEqual(code, 0)
        self.assertEqual(len(replies), 1)
        self.assertTrue(replies[0]["success"])
        self.assertEqual(peer.snapshot(), (1, 1, 1))
        self.assertGreater(len(peer.lines[0]), REQUEST_MAX)
        request = json.loads(peer.lines[0])
        self.assertEqual(request["action"], "transcribe")
        self.assertEqual(request["audio_path"], str(self.audio.resolve()))
        self.assertEqual(request["options"], {"context": context})
        self.assertEqual(request["request_id"], 1)


if __name__ == "__main__":
    unittest.main()
