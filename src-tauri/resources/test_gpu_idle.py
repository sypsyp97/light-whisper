import io
import json
import logging
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

import gpu_idle
from gpu_idle import (
    GPU_IDLE_ENV,
    gpu_idle_should_unload,
    parse_gpu_idle_seconds,
    status_keeps_process_ready,
)
from server_common import BaseASRServer


class IdlePolicyTests(unittest.TestCase):
    def test_zero_and_invalid_values_disable(self):
        for value in (None, 0, -1, False, True, "", "0", "nope", 1.5, "180.0"):
            self.assertEqual(parse_gpu_idle_seconds(value), 0, value)
        self.assertEqual(parse_gpu_idle_seconds(180), 180)
        self.assertEqual(parse_gpu_idle_seconds("180"), 180)
        self.assertEqual(parse_gpu_idle_seconds(gpu_idle.MAX_GPU_IDLE_SECONDS + 5), gpu_idle.MAX_GPU_IDLE_SECONDS)

    def test_timer_math_activity_reset_and_active_stream(self):
        self.assertFalse(
            gpu_idle_should_unload(
                now=10_000, last_activity=0, idle_seconds=0, stream_active=False
            )
        )
        self.assertFalse(
            gpu_idle_should_unload(
                now=179, last_activity=0, idle_seconds=180, stream_active=False
            )
        )
        self.assertTrue(
            gpu_idle_should_unload(
                now=180, last_activity=0, idle_seconds=180, stream_active=False
            )
        )
        # Activity moving to 100 restarts the window.
        self.assertFalse(
            gpu_idle_should_unload(
                now=279, last_activity=100, idle_seconds=180, stream_active=False
            )
        )
        self.assertTrue(
            gpu_idle_should_unload(
                now=280, last_activity=100, idle_seconds=180, stream_active=False
            )
        )
        self.assertFalse(
            gpu_idle_should_unload(
                now=999, last_activity=0, idle_seconds=1, stream_active=True
            )
        )
        self.assertFalse(
            gpu_idle_should_unload(
                now=999, last_activity=None, idle_seconds=1, stream_active=False
            )
        )

    def test_missing_env_stays_off(self):
        env = {}
        self.assertEqual(gpu_idle.gpu_idle_seconds_from_env(env), 0)
        env[GPU_IDLE_ENV] = "0"
        self.assertEqual(gpu_idle.gpu_idle_seconds_from_env(env), 0)
        env[GPU_IDLE_ENV] = "45"
        self.assertEqual(gpu_idle.gpu_idle_seconds_from_env(env), 45)


class _IdleServer(BaseASRServer):
    def __init__(self):
        self.suspend_calls = 0
        self.initialize_calls = 0
        self.stream_active = False
        self.loaded = True
        logger = logging.getLogger(f"test_gpu_idle.{id(self)}")
        logger.disabled = True
        logger.propagate = False
        with mock.patch("server_common.signal.signal"):
            super().__init__(engine="test", logger=logger)
        self.initialized = True

    def _setup_runtime_environment(self):
        pass

    def _detect_device(self):
        return "cpu"

    def initialize(self):
        self.initialize_calls += 1
        self.initialized = True
        self._gpu_suspended = False
        self.loaded = True
        return {"success": True, "engine": self.engine, "initialized": True}

    def check_status(self):
        return {
            "success": True,
            "initialized": self.initialized and self.loaded,
            "model_loaded": self.initialized and self.loaded,
            "engine": self.engine,
        }

    def get_performance_stats(self):
        return {"initialized": self.initialized}

    def transcribe_audio(self, *args, **kwargs):
        if not self.initialized:
            init_result = self.initialize()
            if not init_result.get("success"):
                return init_result
        return {"success": True, "text": "ok"}

    def _gpu_idle_stream_active(self):
        return self.stream_active

    def _suspend_gpu_runtime(self):
        if not self.initialized or self.stream_active:
            return False
        self.suspend_calls += 1
        self.initialized = False
        self.loaded = False
        self._gpu_suspended = True
        return True


class IdleServerTests(unittest.TestCase):
    def _run(self, server, commands, env=None):
        stdin = io.StringIO(
            "\n".join(json.dumps(command) for command in commands) + "\n"
        )
        stdout = io.StringIO()
        with (
            mock.patch.object(sys, "stdin", stdin),
            mock.patch.object(sys, "stdout", stdout),
            mock.patch("hf_cache_utils.get_hf_cache_root", return_value="unused"),
            mock.patch.dict(os.environ, env or {}, clear=False),
        ):
            if env is None:
                os.environ.pop(GPU_IDLE_ENV, None)
            server.run()
        server._stop_gpu_idle_thread()
        return [json.loads(line) for line in stdout.getvalue().splitlines() if line.strip()]

    def test_off_by_default_does_not_start_timer_or_change_status(self):
        server = _IdleServer()
        responses = self._run(
            server,
            [
                {"action": "status", "request_id": 1},
                {"action": "transcribe", "audio_path": "a.wav", "request_id": 2},
                {"action": "exit", "request_id": 3},
            ],
        )
        status = responses[1]
        self.assertEqual(server._gpu_idle_seconds, 0)
        self.assertIsNone(server._gpu_idle_thread)
        self.assertEqual(server.suspend_calls, 0)
        self.assertIsNone(server._last_activity_at)
        self.assertNotIn("gpu_suspended", status)
        self.assertTrue(status["initialized"])
        self.assertTrue(status["model_loaded"])
        self.assertEqual(status["request_id"], 1)

    def test_explicit_zero_env_stays_off(self):
        server = _IdleServer()
        self._run(server, [{"action": "exit", "request_id": 1}], env={GPU_IDLE_ENV: "0"})
        self.assertEqual(server._gpu_idle_seconds, 0)
        self.assertIsNone(server._gpu_idle_thread)
        self.assertEqual(server.suspend_calls, 0)

    def test_positive_env_starts_idle_window_after_startup(self):
        server = _IdleServer()
        server.saw_thread = False
        original = server._gpu_idle_loop

        def loop(stop):
            server.saw_thread = True
            return original(stop)

        server._gpu_idle_loop = loop
        with mock.patch("server_common.time.monotonic", return_value=100):
            self._run(server, [{"action": "exit", "request_id": 1}], env={GPU_IDLE_ENV: "15"})
        self.assertTrue(server.saw_thread)
        self.assertEqual(server._gpu_idle_seconds, 15)
        self.assertEqual(server.suspend_calls, 0)
        self.assertEqual(server._last_activity_at, 100)
        self.assertFalse(server._gpu_idle_tick(now=114))
        self.assertTrue(server._gpu_idle_tick(now=115))

    def test_enabling_after_use_starts_a_fresh_idle_window(self):
        server = _IdleServer()
        self._run(server, [{"action": "transcribe", "audio_path": "a.wav"}])
        with (
            mock.patch.object(server, "_ensure_gpu_idle_thread"),
            mock.patch("server_common.time.monotonic", return_value=100),
        ):
            server._handle_set_gpu_idle({"seconds": 15})
        self.assertFalse(server._gpu_idle_tick(now=114))
        self.assertTrue(server._gpu_idle_tick(now=115))

    def test_changing_positive_timeout_keeps_the_activity_window(self):
        server = _IdleServer()
        server._gpu_idle_seconds = 30
        server._last_activity_at = 100
        with mock.patch.object(server, "_ensure_gpu_idle_thread"):
            server._handle_set_gpu_idle({"seconds": 15})
        self.assertFalse(server._gpu_idle_tick(now=114))
        self.assertTrue(server._gpu_idle_tick(now=115))

    def test_activity_resets_and_status_does_not_count(self):
        server = _IdleServer()
        clock = {"now": 0.0}

        def monotonic():
            clock["now"] += 5
            return clock["now"]

        with (
            mock.patch.object(server, "_ensure_gpu_idle_thread"),
            mock.patch("server_common.time.monotonic", side_effect=monotonic),
        ):
            self._run(
                server,
                [
                    {"action": "set_gpu_idle", "seconds": 30, "request_id": 1},
                    {"action": "transcribe", "audio_path": "a.wav", "request_id": 2},
                    {"action": "status", "request_id": 3},
                    {"action": "stats", "request_id": 4},
                    {"action": "cleanup", "request_id": 5},
                    {"action": "stream_finish", "request_id": 6},
                    {"action": "exit", "request_id": 7},
                ],
            )
        # Enable, transcribe and stream_finish sample the clock; queries do not.
        self.assertEqual(server._last_activity_at, 15.0)
        self.assertEqual(server.suspend_calls, 0)

    def test_tick_unloads_only_when_due_and_not_busy_or_streaming(self):
        server = _IdleServer()
        server._gpu_idle_seconds = 180
        server._last_activity_at = 0
        self.assertFalse(server._gpu_idle_tick(now=179))
        self.assertEqual(server.suspend_calls, 0)

        server.stream_active = True
        self.assertFalse(server._gpu_idle_tick(now=500))
        self.assertEqual(server.suspend_calls, 0)
        server.stream_active = False

        self.assertFalse(server._gpu_idle_lock.acquire(blocking=False) is False)
        self.assertFalse(server._gpu_idle_tick(now=500))
        self.assertEqual(server.suspend_calls, 0)
        server._gpu_idle_lock.release()

        self.assertTrue(server._gpu_idle_tick(now=180))
        self.assertEqual(server.suspend_calls, 1)
        self.assertFalse(server.initialized)
        status = server._with_gpu_suspend_flag(server.check_status())
        self.assertTrue(status["success"])
        self.assertFalse(status["initialized"])
        self.assertFalse(status["model_loaded"])
        self.assertTrue(status["gpu_suspended"])
        self.assertTrue(status_keeps_process_ready(True, status))
        self.assertFalse(status_keeps_process_ready(False, {"success": True, "initialized": False, "model_loaded": False}))

        # Already unloaded: another tick does not reload or double-free.
        self.assertFalse(server._gpu_idle_tick(now=999))
        self.assertEqual(server.suspend_calls, 1)

        # Next transcribe uses the existing initialize() path.
        result = server.transcribe_audio("a.wav")
        self.assertEqual(result["text"], "ok")
        self.assertEqual(server.initialize_calls, 1)
        self.assertTrue(server.initialized)
        self.assertFalse(server._gpu_suspended)

    def test_setting_zero_disables_further_unloads(self):
        server = _IdleServer()
        server._handle_set_gpu_idle({"seconds": 10})
        self.assertTrue(server._gpu_idle_thread.is_alive())
        server._last_activity_at = 0
        turned_off = server._handle_set_gpu_idle({"seconds": 0})
        self.assertTrue(turned_off["success"])
        self.assertFalse(turned_off["enabled"])
        self.assertEqual(server._gpu_idle_seconds, 0)
        self.assertIsNone(server._last_activity_at)
        self.assertFalse(server._gpu_idle_tick(now=10_000))
        self.assertEqual(server.suspend_calls, 0)
        thread = server._gpu_idle_thread
        self.assertTrue(thread is None or not thread.is_alive())

    def test_missing_seconds_is_rejected(self):
        server = _IdleServer()
        result = server._handle_set_gpu_idle({})
        self.assertFalse(result["success"])
        self.assertEqual(server._gpu_idle_seconds, 0)


if __name__ == "__main__":
    unittest.main()
