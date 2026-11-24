import base64
import json
import os
import subprocess
import sys
import tempfile
import unittest
import wave
from pathlib import Path


class RealStopTailTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("LIGHT_WHISPER_R2T2_TEST_AUDIO"), "requires the restaurant WAV and CUDA runtime")
    def test_rolling_stop_retains_last_spoken_word(self):
        """Use the public restaurant WAV documented in docs/r2t2-native.md."""
        with wave.open(os.environ["LIGHT_WHISPER_R2T2_TEST_AUDIO"], "rb") as source:
            self.assertEqual((source.getframerate(), source.getnchannels(), source.getsampwidth()), (16000, 1, 2))
            self.assertEqual(source.getnframes(), 107840)
            clip = source.readframes(source.getnframes())

        speech = clip * 3 + clip[:38400 * 2]
        pause = bytes(5120 * 2)  # 320 ms of captured silence
        cases = (
            ("delayed", speech + pause, "自己带"),
            ("silence", pause, ""),
            ("immediate", speech, "自己带"),
        )
        commands = []
        finish_ids = []
        for session, (_, pcm, _) in enumerate(cases, 1):
            commands.append(dict(action="stream_start", session_id=session))
            for offset in range(0, len(pcm), 2560 * 2):
                commands.append(dict(
                    action="stream_feed", session_id=session, offset=offset // 2,
                    sample_rate=16000, audio_format="pcm_s16le",
                    audio_base64=base64.b64encode(pcm[offset:offset + 2560 * 2]).decode("ascii"),
                ))
            commands.append(dict(action="stream_finish", session_id=session))
            finish_ids.append(len(commands))

        with tempfile.TemporaryDirectory() as data:
            child = subprocess.run(
                [sys.executable, "-X", "utf8", Path(__file__).with_name("engine.py"), "serve", "--engine", "confucius4-r2t2"],
                input="".join(json.dumps(dict(command, request_id=i)) + "\n" for i, command in enumerate(commands, 1)),
                env=dict(os.environ, HF_HUB_OFFLINE="1", LIGHT_WHISPER_AUDIO_PROCESSING="0", LIGHT_WHISPER_DATA_DIR=data),
                capture_output=True, encoding="utf-8", timeout=180,
            )
        self.assertEqual(child.returncode, 0, child.stderr)
        ready, *rows = map(json.loads, child.stdout.splitlines())
        self.assertTrue(ready["success"], ready)
        self.assertEqual(ready["backend"], "cuda")
        self.assertEqual(len(rows), len(commands))
        for request_id, row in enumerate(rows, 1):
            self.assertTrue(row["success"], row)
            self.assertEqual(row["request_id"], request_id)

        for (name, pcm, suffix), request_id in zip(cases, finish_ids):
            with self.subTest(stop=name):
                final = rows[request_id - 1]
                self.assertTrue(final["final"])
                self.assertEqual(final["sample_count"], len(pcm) // 2)
                if suffix:
                    words = "".join(c for c in final["text"] if c.isalnum())
                    self.assertTrue(words.endswith(suffix), final["text"])
                    # The final repetition has no audio after the cut word.
                    self.assertEqual(words.rsplit("顾客", 1)[-1], suffix)
                else:
                    self.assertEqual(final["text"], "")


if __name__ == "__main__":
    unittest.main()
