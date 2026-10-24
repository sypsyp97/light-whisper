"""Source correspondence only; Rust/React tests exercise deadline and skip behavior."""

import re
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_request_snapshot

ROOT = Path(__file__).resolve().parents[1]
POLISH = "src-tauri/src/services/ai_polish_service.rs"
AUDIO = "src-tauri/src/commands/audio.rs"
FINALIZE = "src-tauri/src/services/audio_service/finalize.rs"


def read(path: str) -> str:
    return (ROOT / path).read_text(encoding="utf-8")


def body(path: str, name: str) -> str:
    source = read(path)
    start = source.index(f"fn {name}(")
    return source[start:source.index("\n}", start)]


class PolishOutputWiring(unittest.TestCase):
    def test_public_polish_and_edit_share_one_120_second_outer_budget(self):
        self.assertRegex(read(POLISH), r"const AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS: u64 = 120;")
        for public, inner in [("polish_text_with_overrides_detailed", "polish_text_inner"),
                              ("edit_text", "edit_text_inner")]:
            with self.subTest(entry=public):
                wrapper = body(POLISH, public)
                self.assertEqual(wrapper.count("run_polish_operation("), 1)
                self.assertRegex(wrapper, rf"run_polish_operation\(\s*state,\s*session_id,\s*"
                                 rf"std::time::Duration::from_secs\(AI_POLISH_STREAM_TOTAL_TIMEOUT_SECS\),\s*"
                                 rf"{inner}\(")
                self.assertNotIn("resolve_api_key_for_provider", wrapper)
                self.assertIn("resolve_api_key_for_provider", body(POLISH, inner))
        inner = body(POLISH, "polish_text_inner")
        for operation in ["evaluate_polish_gate(", "evaluate_polish_expansion(",
                          "build_polish_user_input(", "send_llm_request_with_fallback("]:
            self.assertIn(operation, inner)
        self.assertIn("send_llm_request_with_transport_fallback(",
                      body(POLISH, "send_llm_request_with_fallback"))

    def test_stop_registers_skip_before_processing_event_and_forwards_receiver(self):
        stop = body(AUDIO, "stop_recording_inner")
        registration = stop.index("state.recording.begin_dictation_polish(session_id)")
        processing = re.search(r"emit_recording_state\(&app_handle,\s*snapshot,\s*false,\s*false,\s*true,", stop)
        self.assertIsNotNone(processing)
        self.assertLess(registration, processing.start())
        self.assertIn("finalize_recording(app_handle, session, polish_cancel).await", stop)
        self.assertLess(stop.index("DictationPolishCleanup(&state.recording, session_id)"),
                        stop.index("finalize_recording("))
        self.assertIn("self.0.clear_dictation_polish(self.1)", read(AUDIO))

    def test_skip_races_only_edit_and_dictation_polish_after_final_asr(self):
        finalize = body(FINALIZE, "finalize_recording")
        self.assertEqual(finalize.count("run_cancellable_polish("), 2)
        self.assertLess(finalize.index("do_final_asr("), finalize.index("run_cancellable_polish("))
        edit = finalize.split("if mode == RecordingMode::Dictation && edit_context.is_some() {", 1)[1]
        edit = edit.split("} else if mode == RecordingMode::Assistant {", 1)[0]
        normal = finalize.split("let original = text.clone();", 1)[1]
        for branch, operation in [(edit, "edit_text"), (normal, "polish_text_with_overrides_detailed")]:
            self.assertRegex(branch, rf"run_cancellable_polish\(\s*ai_polish_service::{operation}\(")
            self.assertRegex(branch, r"\),\s*polish_cancel,\s*\)\s*\.await")

    def test_edit_skip_publishes_selected_original_without_paste_or_failure_history(self):
        edit = body(FINALIZE, "finalize_recording").split(
            "if mode == RecordingMode::Dictation && edit_context.is_some() {", 1)[1]
        edit = edit.split("} else if mode == RecordingMode::Assistant {", 1)[0]
        error = edit.split("Err(e) => {", 1)[1]
        self.assertIn("let cancelled = e == ai_polish_service::POLISH_CANCELLED_ERROR", error)
        self.assertNotIn("do_paste(", error)
        self.assertRegex(error, r"if cancelled \{\s*emit_done_with_timing\(\s*&app_handle,\s*"
                         r"session_id,\s*subtitle_show_gen,\s*mode,\s*&selected_text,\s*&selected_text,\s*"
                         r"duration_sec,\s*false,")
        self.assertRegex(error, r'if cancelled\s*\{\s*"success"\s*\}\s*else\s*\{\s*"processing_error"')
        self.assertIn('if cancelled { &selected_text } else { "" }', error)
        self.assertIn("(!cancelled).then_some(e.as_str())", error)

    def test_dictation_skip_and_timeout_keep_the_recognized_original(self):
        normal = body(FINALIZE, "finalize_recording").split("let original = text.clone();", 1)[1]
        cancellation = normal.split("Err(error) if error == ai_polish_service::POLISH_CANCELLED_ERROR => {", 1)[1]
        cancellation, failure = cancellation.split("Err(error) => {", 1)
        self.assertIn("(original.clone(), None, None, Some(elapsed_polish_ms))", cancellation)
        self.assertIn("(original.clone(), None, None, None)", failure.split("let polished =", 1)[0])

    def test_skip_ipc_and_main_button_are_wired_to_dictation_session(self):
        self.assertIn("commands::audio::cancel_dictation_polish,", read("src-tauri/src/lib.rs"))
        self.assertIn("state.recording.cancel_dictation_polish(session_id)", body(AUDIO, "cancel_dictation_polish"))
        self.assertRegex(read("src/api/tauri.ts"), r"export function cancelDictationPolish\(sessionId: number\)"
                         r': Promise<boolean> \{\s*return invokeCommand<boolean>\("cancel_dictation_polish", \{ sessionId \}\);')
        self.assertIn("await cancelDictationPolish(sessionId)", read("src/hooks/useRecording.ts"))
        self.assertRegex(read("src/pages/MainPage.tsx"), r'onCancelProcessing=\{\s*recordingMode\s*===\s*"dictation"\s*'
                         r"\?\s*\(\)\s*=>\s*\{\s*void cancelPolish\(\);\s*\}\s*:\s*undefined\s*\}")


class SnapshotGuardNegativeControls(unittest.TestCase):
    def check_mutation(self, source: str, message: str):
        with tempfile.TemporaryDirectory(prefix="light-whisper-polish-guard-") as directory:
            overlay = Path(directory)
            target = overlay / POLISH
            target.parent.mkdir(parents=True)
            target.write_text(source, encoding="utf-8")
            with patch.object(check_request_snapshot, "ROOT", overlay):
                with self.assertRaisesRegex(ValueError, message):
                    check_request_snapshot.check()

    def test_snapshot_guard_rejects_missing_outer_budget_delegation(self):
        source = read(POLISH)
        self.assertIn("run_polish_operation(", source)
        self.check_mutation(source.replace("run_polish_operation(", "unbounded_polish_operation(", 1),
                            "shared operation budget must wrap polish_text_inner")

    def test_snapshot_guard_still_rejects_live_configuration_after_auth(self):
        source = read(POLISH)
        original = "let changed = polished != text;"
        self.assertIn(original, source)
        self.check_mutation(source.replace(original, "let _live = state.llm_provider_config();\n    " + original, 1),
                            "live configuration or unscoped key cache read")


if __name__ == "__main__":
    unittest.main()
