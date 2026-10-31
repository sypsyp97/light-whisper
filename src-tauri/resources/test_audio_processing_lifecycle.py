"""Exercise enhancement ownership in both actual ASR server lifecycles."""

from contextlib import ExitStack, contextmanager
import logging
import os
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server
import r2t2_asr_server
from r2t2_stream import R2T2StreamSession
from test_qwen3_asr_server import FakeVad
from test_r2t2_stream import RecordingRuntime


ENGINES = (
    (qwen3_asr_server, qwen3_asr_server.Qwen3ASRServer),
    (r2t2_asr_server, r2t2_asr_server.R2T2ASRServer),
)


class AudioProcessingLifecycleTests(unittest.TestCase):
    @contextmanager
    def server_case(self, module, server_class, *, enabled):
        """Keep construction/initialization real; replace all model/device work."""
        enhancers, natives = [], []

        def new_enhancer(*_args, **_kwargs):
            enhancer = mock.Mock(spec=["enhance", "reset", "feed", "flush", "close"])
            enhancers.append(enhancer)
            return enhancer

        factory = mock.Mock(name="RealtimeReuseEnhancer", side_effect=new_enhancer)
        enhancement_module = types.ModuleType("reuse_enhancement")
        enhancement_module.RealtimeReuseEnhancer = factory
        enhancement_module.ReuseEnhancer = mock.Mock(
            name="ReuseEnhancer",
            side_effect=AssertionError("server default must select RealtimeReuseEnhancer"),
        )
        logger = logging.Logger(f"test_audio_processing_lifecycle.{server_class.__name__}")
        logger.disabled = True
        with ExitStack() as stack:
            stack.enter_context(mock.patch.dict(os.environ, {"LIGHT_WHISPER_AUDIO_PROCESSING": "1" if enabled else "0"}))
            stack.enter_context(mock.patch.dict(sys.modules, {"reuse_enhancement": enhancement_module}))
            stack.enter_context(mock.patch("server_common.signal.signal"))
            stack.enter_context(mock.patch.object(module, "logger", logger))
            stack.enter_context(mock.patch.object(server_class, "_detect_device", return_value="cpu"))
            stack.enter_context(mock.patch.object(server_class, "_setup_runtime_environment"))
            stack.enter_context(mock.patch.object(server_class, "_resolve_model_path", return_value="test-model.gguf"))
            stack.enter_context(mock.patch.object(server_class, "_get_gpu_device_info", return_value={"device": "cpu"}))
            stack.enter_context(mock.patch.object(module, "FireRedVad", return_value=FakeVad([])))
            if module is qwen3_asr_server:
                stack.enter_context(mock.patch.object(server_class, "_warmup_inference"))
            else:
                stack.enter_context(mock.patch.object(module, "SegmentedR2T2", side_effect=lambda *args, **kwargs: RecordingRuntime()))
            server = server_class()

            def load_runtime(_model_path):
                native = types.SimpleNamespace(close=mock.Mock(), chunk_samples=4)
                natives.append(native)
                if module is qwen3_asr_server:
                    server.model = server.session = native
                else:
                    server.native = native
                server.backend = server.device = "cpu"

            stack.enter_context(mock.patch.object(server, "_load_runtime", side_effect=load_runtime))
            try:
                yield types.SimpleNamespace(server=server, factory=factory, enhancers=enhancers, natives=natives)
            finally:
                server._close_runtime()

    @staticmethod
    def set_enabled(server, enabled):
        return server._execute_action({"enabled": enabled}, "set_audio_processing")

    def test_enabled_initialize_attaches_one_enhancer_and_is_idempotent(self):
        for module, server_class in ENGINES:
            with self.subTest(engine=server_class.__name__), self.server_case(module, server_class, enabled=True) as case:
                result = case.server.initialize()
                self.assertTrue(result["success"], result)
                self.assertTrue(case.server.initialized)
                case.factory.assert_called_once()
                self.assertIsInstance(case.server.stream, R2T2StreamSession)
                self.assertIs(case.server.stream.audio_processor, case.enhancers[0])
                stream = case.server.stream
                self.assertTrue(case.server.initialize()["success"])
                case.factory.assert_called_once()
                self.assertEqual(len(case.natives), 1)
                self.assertIs(case.server.stream, stream)
                case.enhancers[0].close.assert_not_called()

    def test_off_initialize_and_idle_toggle_attach_detach_without_reloading_asr(self):
        for module, server_class in ENGINES:
            with self.subTest(engine=server_class.__name__), self.server_case(module, server_class, enabled=False) as case:
                self.assertTrue(case.server.initialize()["success"])
                case.factory.assert_not_called()
                stream = case.server.stream
                self.assertIsNone(stream.audio_processor)
                for _ in range(2):
                    response = self.set_enabled(case.server, True)
                    self.assertTrue(response["success"], response)
                    self.assertTrue(response["audio_processing_enabled"])
                case.factory.assert_called_once()
                self.assertIs(case.server.stream, stream)
                self.assertIs(stream.audio_processor, case.enhancers[0])
                for _ in range(2):
                    response = self.set_enabled(case.server, False)
                    self.assertTrue(response["success"], response)
                    self.assertFalse(response["audio_processing_enabled"])
                self.assertIsNone(stream.audio_processor)
                case.enhancers[0].close.assert_called_once_with()
                self.assertTrue(self.set_enabled(case.server, True)["success"])
                self.assertEqual(case.factory.call_count, 2)
                self.assertIs(stream.audio_processor, case.enhancers[1])
                self.assertEqual(len(case.natives), 1)
                self.assertTrue(case.server.initialized)

    def test_idle_suspend_releases_enhancer_and_asr_then_resume_recreates_both(self):
        for module, server_class in ENGINES:
            with self.subTest(engine=server_class.__name__), self.server_case(module, server_class, enabled=False) as case:
                self.assertTrue(case.server.initialize()["success"])
                # Existing Base toggle provides an enhancer before the unload,
                # isolating close/resume behavior from enabled initialization.
                self.assertTrue(self.set_enabled(case.server, True)["success"])
                original_processor, original_native = case.enhancers[0], case.natives[0]
                original_stream, original_vad = case.server.stream, case.server.vad_model

                self.assertTrue(case.server._suspend_gpu_runtime())
                original_processor.close.assert_called_once_with()
                original_native.close.assert_called_once_with()
                self.assertFalse(case.server.initialized)
                self.assertIsNone(case.server.stream)
                self.assertIs(case.server.vad_model, original_vad)
                self.assertTrue(case.server.initialize()["success"])
                self.assertEqual(case.factory.call_count, 2, "resume must retain the enabled policy")
                self.assertEqual(len(case.natives), 2)
                self.assertIsNot(case.server.stream, original_stream)
                self.assertIs(case.server.stream.audio_processor, case.enhancers[1])
                self.assertIsNot(case.enhancers[1], original_processor)
                self.assertIs(case.server.vad_model, original_vad)

    def test_enhancer_load_failure_cannot_report_ready_and_can_retry_off(self):
        for module, server_class in ENGINES:
            with self.subTest(engine=server_class.__name__), self.server_case(module, server_class, enabled=True) as case:
                case.factory.side_effect = RuntimeError("injected enhancement load failure")
                result = case.server.initialize()
                self.assertFalse(result["success"], result)
                case.factory.assert_called_once()
                self.assertFalse(case.server.initialized)
                self.assertFalse(case.server.check_status()["model_loaded"])
                self.assertIsNone(case.server.stream)
                for native in case.natives:
                    native.close.assert_called_once_with()

                response = self.set_enabled(case.server, False)
                self.assertTrue(response["success"], response)
                self.assertFalse(response["audio_processing_enabled"])
                self.assertTrue(case.server.initialize()["success"])
                self.assertTrue(case.server.initialized)
                self.assertIsNone(case.server.stream.audio_processor)
                case.factory.assert_called_once()


if __name__ == "__main__":
    unittest.main()
