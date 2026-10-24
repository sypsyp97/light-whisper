"""Source correspondence only; actual queue decisions have Rust replay tests."""

import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def body(path: str, name: str) -> str:
    source = (ROOT / path).read_text(encoding="utf-8")
    start = re.search(rf"(?m)^(?:pub(?:\([^)]*\))? )?(?:async )?fn {name}\(", source)
    if not start:
        raise AssertionError(f"missing production function {name}")
    remainder = source[start.end():]
    end = re.search(r"(?m)^(?:pub(?:\([^)]*\))? )?(?:async )?fn \w+\(", remainder)
    return remainder[:end.start()] if end else remainder


class OutputDrainWiring(unittest.TestCase):
    def test_start_error_schedules_the_shared_drain(self):
        self.assertRegex(body("src-tauri/src/commands/audio.rs", "emit_start_error"),
                         r"audio_service::flush_pending_paste\(app_handle\)")

    def test_start_cancel_schedules_before_the_terminal_branch(self):
        source = body("src-tauri/src/commands/audio.rs", "stop_recording_inner")
        branch = source.index("let session = match recording {")
        self.assertIn("audio_service::flush_pending_paste(&app_handle)", source[:branch])

    def test_success_and_error_terminal_helpers_schedule_the_shared_drain(self):
        for name in ["emit_done_with_timing", "emit_terminal_outcome"]:
            self.assertIn("flush_pending_paste(app)",
                          body("src-tauri/src/services/audio_service/finalize.rs", name))

    def test_history_archive_encoding_does_not_run_inline_during_finalization(self):
        source = body("src-tauri/src/services/audio_service/finalize.rs", "finalize_recording")
        self.assertNotIn("encode_wav(", source)

    def test_opted_in_raw_audio_is_snapshotted_before_final_asr_can_drain_it(self):
        source = body("src-tauri/src/services/audio_service/finalize.rs", "finalize_recording")
        self.assertIn("history_audio_snapshot(", source)
        self.assertLess(source.index("history_audio_snapshot("), source.index("do_final_asr("))

    def test_history_persist_encodes_audio_in_a_blocking_task(self):
        source = (ROOT / "src-tauri/src/services/audio_service/finalize.rs").read_text(encoding="utf-8")
        persist = source.split("impl HistorySessionContext {", 1)[1].split("\n}\n", 1)[0]
        self.assertRegex(persist, r"tokio::task::spawn_blocking\(\s*move\s*\|\|\s*(?:\{\s*)?"
                                 r"encode_wav\(&samples,\s*sample_rate\)")

    def test_asr_failure_publishes_before_awaiting_history(self):
        source = body("src-tauri/src/services/audio_service/finalize.rs", "finalize_recording")
        failure = source.split("let text = match asr_text {", 1)[1].split("let lang_ref", 1)[0]
        self.assertLess(failure.index("emit_error("), failure.index(".persist("))

    def test_final_dictation_is_published_and_queued_before_awaiting_history(self):
        source = body("src-tauri/src/services/audio_service/finalize.rs", "finalize_recording")
        persist = source.rindex(".persist(")
        self.assertLess(source.rindex("emit_done_with_timing("), persist)
        self.assertLess(source.rindex("do_paste("), persist)

    def test_no_speech_edit_and_assistant_branches_publish_before_history(self):
        source = body("src-tauri/src/services/audio_service/finalize.rs", "finalize_recording")
        edit_start = "if mode == RecordingMode::Dictation && edit_context.is_some() {"
        no_speech = source.split("if text.is_empty() {", 1)[1].split(edit_start, 1)[0]
        edit = source.split(edit_start, 1)[1].split("} else if mode == RecordingMode::Assistant {", 1)[0]
        assistant = source.split("match assistant_result {", 1)[1].split("    } else {", 1)[0]
        cases = [
            ("no_speech", no_speech, r"emit_terminal_outcome\(", False),
            ("edit_success", edit.split("Ok(outcome) => {", 1)[1].split("Err(e) => {", 1)[0],
             r"emit_done(?:_with_timing)?\(", True),
            ("edit_failure", edit.split("Err(e) => {", 1)[1], r"emit_error\(", False),
            ("assistant_success", assistant.split("Ok(outcome) => {", 1)[1].split("Err(err) => {", 1)[0],
             r"emit_done(?:_with_timing)?\(", False),
            ("assistant_failure", assistant.split("Err(err) => {", 1)[1], r"emit_error\(", False),
        ]
        for name, branch, publication, automatic_paste in cases:
            with self.subTest(branch=name):
                event = re.search(publication, branch)
                self.assertIsNotNone(event)
                persist = branch.index(".persist(")
                self.assertLess(event.start(), persist)
                if name.endswith("_success"):
                    self.assertIn("emit_done_with_timing(", branch)
                if automatic_paste:
                    self.assertLess(branch.index("do_paste("), persist)


if __name__ == "__main__":
    unittest.main()
