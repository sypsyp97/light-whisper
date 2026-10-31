import builtins
import importlib
import importlib.util
import logging
import os
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

from server_common import BaseASRServer


class _AudioProcessingServer(BaseASRServer):
    def __init__(self, *, initialized=True):
        self.stream_active = False
        logger = logging.getLogger(f"test_audio_processing.{id(self)}")
        logger.disabled = True
        logger.propagate = False
        with mock.patch("server_common.signal.signal"):
            super().__init__(engine="test", logger=logger)
        self.initialized = initialized

    def _detect_device(self):
        return "cpu"

    def _setup_runtime_environment(self):
        pass

    def initialize(self):
        self.initialized = True
        return {"success": True}

    def check_status(self):
        return {"success": True, "initialized": self.initialized}

    def _gpu_idle_stream_active(self):
        return self.stream_active


class AudioProcessingConfigurationTests(unittest.TestCase):
    def setUp(self):
        self.enhancer = mock.Mock(spec=["close"])
        self.factory = mock.Mock(name="RealtimeReuseEnhancer", return_value=self.enhancer)
        module = types.ModuleType("reuse_enhancement")
        module.RealtimeReuseEnhancer = self.factory
        module.ReuseEnhancer = mock.Mock(
            name="ReuseEnhancer",
            side_effect=AssertionError("server default must select RealtimeReuseEnhancer"),
        )
        patcher = mock.patch.dict(sys.modules, {"reuse_enhancement": module})
        patcher.start()
        self.addCleanup(patcher.stop)

    @staticmethod
    def _set_enabled(server, enabled):
        return server._execute_action({"enabled": enabled}, "set_audio_processing")

    def _assert_enabled(self, response, enabled):
        self.assertIs(response.get("success"), True, response)
        self.assertIs(response.get("audio_processing_enabled"), enabled, response)

    def test_default_off_can_be_saved_during_stream_without_import_or_load(self):
        enhancement_imports = []
        original_import = builtins.__import__
        original_import_module = importlib.import_module

        def tracked_import(importer):
            def run(name, *args, **kwargs):
                if name == "reuse_enhancement" or name.startswith("reuse_enhancement."):
                    enhancement_imports.append(name)
                return importer(name, *args, **kwargs)
            return run

        with (
            mock.patch("builtins.__import__", side_effect=tracked_import(original_import)),
            mock.patch("importlib.import_module", side_effect=tracked_import(original_import_module)),
        ):
            spec = importlib.util.spec_from_file_location(
                "_audio_processing_fresh_server_common",
                os.path.join(os.path.dirname(__file__), "server_common.py"),
            )
            fresh_module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(fresh_module)
            server = _AudioProcessingServer(initialized=False)
            server.stream_active = True
            self._assert_enabled(self._set_enabled(server, False), False)
            server.stream_active = False
            server.initialize()
            server.stream_active = True
            self._assert_enabled(self._set_enabled(server, False), False)

        self.assertEqual(enhancement_imports, [])
        self.factory.assert_not_called()
        self.enhancer.close.assert_not_called()

    def test_enable_loads_once_when_asr_is_initialized(self):
        server = _AudioProcessingServer()

        self._assert_enabled(self._set_enabled(server, True), True)
        self._assert_enabled(self._set_enabled(server, True), True)

        self.factory.assert_called_once()
        self.enhancer.close.assert_not_called()

    def test_uninitialized_server_saves_setting_without_loading(self):
        server = _AudioProcessingServer(initialized=False)

        self._assert_enabled(self._set_enabled(server, True), True)
        self._assert_enabled(self._set_enabled(server, False), False)

        self.factory.assert_not_called()
        self.enhancer.close.assert_not_called()

    def test_disable_closes_once_and_reenable_creates_a_new_enhancer(self):
        server = _AudioProcessingServer()
        replacement = mock.Mock(spec=["close"])
        self.factory.side_effect = [self.enhancer, replacement]

        self._assert_enabled(self._set_enabled(server, True), True)
        self._assert_enabled(self._set_enabled(server, False), False)
        self._assert_enabled(self._set_enabled(server, False), False)
        self.enhancer.close.assert_called_once_with()

        self._assert_enabled(self._set_enabled(server, True), True)
        self.assertEqual(self.factory.call_count, 2)
        self.enhancer.close.assert_called_once_with()
        replacement.close.assert_not_called()

    def test_missing_or_non_boolean_enabled_preserves_saved_setting(self):
        invalid_commands = [{}] + [
            {"enabled": value}
            for value in (None, 0, 1, 1.0, "true", "false", [], {})
        ]
        for enabled in (False, True):
            with self.subTest(enabled=enabled):
                server = _AudioProcessingServer()
                self._assert_enabled(self._set_enabled(server, enabled), enabled)
                for command in invalid_commands:
                    with self.subTest(command=command):
                        response = server._execute_action(command, "set_audio_processing")
                        self.assertIs(response.get("success"), False, response)
                        self.assertTrue(response.get("error"), response)
                        server.stream_active = True
                        self._assert_enabled(self._set_enabled(server, enabled), enabled)
                        server.stream_active = False

        self.factory.assert_called_once()
        self.enhancer.close.assert_not_called()

    def test_active_stream_blocks_changes_but_allows_same_value(self):
        for enabled in (False, True):
            with self.subTest(enabled=enabled):
                server = _AudioProcessingServer()
                self._assert_enabled(self._set_enabled(server, enabled), enabled)
                server.stream_active = True

                response = self._set_enabled(server, not enabled)
                self.assertIs(response.get("success"), False, response)
                self.assertTrue(response.get("error"), response)
                self._assert_enabled(self._set_enabled(server, enabled), enabled)

        self.factory.assert_called_once()
        self.enhancer.close.assert_not_called()

    def test_load_failure_keeps_disabled_and_next_enable_can_retry(self):
        server = _AudioProcessingServer()
        self.factory.side_effect = [RuntimeError("model load failed"), self.enhancer]

        response = self._set_enabled(server, True)
        self.assertIs(response.get("success"), False, response)
        self.assertTrue(response.get("error"), response)
        self.factory.assert_called_once()
        self.enhancer.close.assert_not_called()

        server.stream_active = True
        self._assert_enabled(self._set_enabled(server, False), False)
        server.stream_active = False
        self._assert_enabled(self._set_enabled(server, True), True)
        self._assert_enabled(self._set_enabled(server, True), True)
        self.assertEqual(self.factory.call_count, 2)
        self.enhancer.close.assert_not_called()


if __name__ == "__main__":
    unittest.main()
