"""Preserve decoded JSON strings across real Python CLI/backend pipes.

The maintained synthetic backend ASCII-escapes its replies and request log.
Session inputs also use ASCII JSON escapes, never invalid raw UTF-8. These
tests exercise the production CLI and EngineClient, without native ASR or GUI.
"""

import json
import os
import shutil
import subprocess
import unittest
import wave

from scripts import test_light_whisper_cli as support


SCALAR = '中文 / \U0001f642 / e\u0301 / "quoted" \\ slash\nline'
UNPAIRED = ("\ud800", "\udfff", "left\ud800/中\udfff/right\U0001f642")
OPAQUE = {"token": "opaque-\ud800-\udfff", "key\ud800": [SCALAR, "\udfff", ""]}
BACKEND = r'''
import json, os, sys

sys.stdin.reconfigure(encoding="utf-8")
sys.stdout.reconfigure(encoding="utf-8")
record = os.environ["CLI_TEST_RECORD"]
def emit(value):
    print(json.dumps(value, ensure_ascii=True), flush=True)
def note(value):
    with open(record, "a", encoding="ascii") as stream:
        stream.write(json.dumps(value, ensure_ascii=True) + "\n")
note({"event": "spawn", "pid": os.getpid()})
try:
    emit({"success": True})
    for line in sys.stdin:
        request = json.loads(line)
        note({"event": "request", "request": request})
        if request["action"] == "exit":
            break
        emit({"success": True, "request_id": request["request_id"], "request": request,
              "opaque": {"token": "opaque-\ud800-\udfff",
                         "key\ud800": ['中文 / \U0001f642 / e\u0301 / "quoted" \\ slash\nline',
                                       "\udfff", ""]}})
finally:
    note({"event": "exit"})
'''


def setUpModule():
    support.setUpModule()


class UnicodeSerializationTests(support.BackendCase):
    def setUp(self):
        super().setUp()
        self.script.write_text(BACKEND, encoding="utf-8")
        self.audio = self.directory / "synthetic 音频.wav"
        with wave.open(str(self.audio), "wb") as audio:
            audio.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
            audio.writeframes(b"\0\0" * 8)

    def assert_closed(self, client):
        self.assert_reaped(client)
        self.assertTrue(client.child.stdin.closed)
        self.assertTrue(client.child.stdout.closed)
        for worker in (client.reader, client.sender):
            if worker is not None:
                self.assertFalse(worker.is_alive())

    def requests(self, record):
        events = [json.loads(line) for line in record.read_text(encoding="ascii").splitlines()]
        self.assertEqual(events[-1]["event"], "exit", "synthetic backend did not finish cleanup")
        self.assertEqual(sum(event["event"] == "spawn" for event in events), 1)
        return [event["request"] for event in events if event["event"] == "request"]

    def public_cli(self, command, records=()):
        root = self.directory / ("source-cli-" + str(len(self.owned_children)))
        script = root / "scripts/light_whisper_cli.py"
        engine = root / "src-tauri/resources/engine.py"
        script.parent.mkdir(parents=True)
        engine.parent.mkdir(parents=True)
        source = support.ROOT / "scripts/light_whisper_cli.py"
        shutil.copyfile(source, script)
        self.assertEqual(script.read_bytes(), source.read_bytes())
        engine.write_text(BACKEND, encoding="utf-8")
        record = root / "requests.jsonl"
        # Encode only ASCII bytes: the input is valid UTF-8 JSON containing
        # escaped code units, not an invalid UTF-8 sequence or surrogate argv.
        payload = "".join(json.dumps(value, ensure_ascii=True) + "\n" for value in records).encode("ascii")
        completed = subprocess.run(
            [support.BACKEND_PYTHON, str(script), "--target", "engine",
             "--python", support.BACKEND_PYTHON, "--engine", "qwen3-asr-0.6b",
             "--threads", "1", "--timeout", "2", *command],
            input=payload, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd=root, timeout=8,
            env={**os.environ, "APPDATA": str(root / "roaming"),
                 "LIGHT_WHISPER_DATA_DIR": str(root / "config"),
                 "HF_HOME": str(root / "models"), "CLI_TEST_RECORD": str(record)},
        )
        self.assertEqual(completed.returncode, 0, completed.stderr.decode("utf-8", errors="replace"))
        replies = [json.loads(line) for line in completed.stdout.decode("utf-8").splitlines()]
        self.assertTrue(replies)
        self.assertTrue(all(reply["success"] is True for reply in replies))
        self.assertTrue(all(child.poll() is not None for child in self.owned_children))
        return replies, self.requests(record)

    def test_engine_client_preserves_scalar_unicode_values_and_keys(self):
        fields = {"payload": {SCALAR: [SCALAR, "", "literal \\ud800", "?"]}}
        client = self.client(timeout=2)
        with client:
            reply = client.request("echo", **fields)
        self.assert_closed(client)
        expected = {**fields, "action": "echo", "request_id": 1}
        self.assertEqual((reply["request"], self.requests(self.record)[0]), (expected, expected))

    def test_engine_client_preserves_unpaired_and_mixed_json_values_and_keys(self):
        client = self.client(timeout=2)
        with client:
            replies = [client.request("echo", payload={value: [value, SCALAR, "?"]})
                       for value in UNPAIRED]
        self.assert_closed(client)
        requests = self.requests(self.record)
        for index, value in enumerate(UNPAIRED):
            with self.subTest(value=ascii(value)):
                expected = {"payload": {value: [value, SCALAR, "?"]},
                            "action": "echo", "request_id": index + 1}
                self.assertEqual((replies[index]["request"], requests[index]), (expected, expected))

    def test_source_cli_transcribe_preserves_scalar_context_and_hot_words(self):
        words = [SCALAR, "", SCALAR]
        replies, requests = self.public_cli([
            "transcribe", str(self.audio), "--language", SCALAR, "--context", SCALAR,
            *[part for word in words for part in ("--hot-word", word)],
        ])
        expected = {"action": "transcribe", "request_id": 1,
                    "audio_path": str(self.audio.resolve()),
                    "options": {"language": SCALAR, "context": SCALAR}, "hot_words": words}
        self.assertEqual((replies[0]["request"], requests[0]), (expected, expected))
        self.assertEqual([request["action"] for request in requests], ["transcribe", "exit"])

    def test_source_cli_session_preserves_escaped_strings_and_external_ids(self):
        large_id = -(2 ** 2048 + 7)
        identities = [large_id, 17, large_id, 88]
        records = [
            {"request_id": identities[index], "action": "transcribe", "audio": str(self.audio),
             "language": value, "context": value, "hot_words": [value, SCALAR, "", value]}
            for index, value in enumerate(UNPAIRED)
        ] + [{"request_id": identities[-1], "action": "exit"}]
        replies, requests = self.public_cli(["session"], records)
        self.assertEqual([reply["request_id"] for reply in replies], identities)
        self.assertEqual([request["request_id"] for request in requests], [1, 2, 3, 4])
        self.assertEqual([request["action"] for request in requests], ["transcribe"] * 3 + ["exit"])
        for index, value in enumerate(UNPAIRED):
            with self.subTest(value=ascii(value)):
                expected = {"action": "transcribe", "request_id": index + 1,
                            "audio_path": str(self.audio.resolve()),
                            "options": {"language": value, "context": value},
                            "hot_words": [value, SCALAR, "", value]}
                self.assertEqual((replies[index]["request"], requests[index]), (expected, expected))

    def test_source_cli_preserves_escaped_backend_response_fields(self):
        for surface in ("status", "session"):
            with self.subTest(surface=surface):
                records = ({"request_id": -7, "action": "status"},
                           {"request_id": 86, "action": "exit"}) if surface == "session" else ()
                replies, _ = self.public_cli([surface], records)
                self.assertEqual(replies[0]["opaque"], OPAQUE)
                if surface == "session":
                    self.assertEqual([reply["request_id"] for reply in replies], [-7, 86])


if __name__ == "__main__":
    unittest.main()
