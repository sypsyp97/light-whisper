import unittest
from unittest.mock import patch
import test_r2t2_asr_server as fixtures


class StreamStatsTests(unittest.TestCase):
    def test_only_completed_streams_contribute_audio_and_compute_time(self):
        server = fixtures.R2T2ASRServerAdapterTests._server_shell(
            fixtures.FakeStreamRuntime(feed_results=[("ok", "en")], finish_result=("ok", "en"))
        )
        with patch("r2t2_asr_server.time.perf_counter", side_effect=[1, 1.01, 2, 2.02, 3, 3.03, 4, 4.04, 5, 5.05]):
            self.assertTrue(server.handle_stream_command({"action": "stream_start", "session_id": 1})["success"])
            self.assertTrue(server.handle_stream_command({
                "action": "stream_feed", "session_id": 1, "offset": 0,
                "sample_rate": 16000, "audio_format": "pcm_s16le",
                "audio_base64": fixtures.R2T2ASRServerAdapterTests._pcm16([1] * 1600),
            })["success"])
            self.assertEqual(server.get_performance_stats()["transcription_count"], 0)
            self.assertTrue(server.handle_stream_command({"action": "stream_finish", "session_id": 1})["success"])
            self.assertTrue(server.handle_stream_command({"action": "stream_start", "session_id": 2})["success"])
            self.assertTrue(server.handle_stream_command({"action": "stream_cancel", "session_id": 2})["success"])
        stats = server.get_performance_stats()
        self.assertEqual(stats["transcription_count"], 1)
        self.assertEqual(stats["total_audio_duration"], 0.1)
        self.assertAlmostEqual(stats["average_inference_ms"], 60.0)
