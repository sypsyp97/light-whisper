"""Challenge GREEN session closure tests with bounded deliberately wrong source.

These six targeted fault probes are not an exhaustive mutation score. Production
files stay read-only: variants execute in a temporary module, or in the existing
test's disposable script fixture for the UTF-8 subprocess case.
"""

import argparse
import hashlib
import importlib
import io
import json
from pathlib import Path
import shutil
import sys
import types
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
TEST_NAMES = [
    "test_eof_cleanup_failure_keeps_completed_and_invalid_replies_without_extra_json",
    "test_exit_cleanup_failure_replies_once_with_correlated_failure_and_never_acknowledges_early",
    "test_exit_success_is_published_after_cleanup_completes",
    "test_keyboard_interrupt_before_any_record_has_no_json_or_engine_and_returns_130",
    "test_keyboard_interrupt_reading_next_record_reaps_owner_without_duplicate_reply",
    "test_keyboard_interrupt_awaiting_response_reaps_owner_and_fails_pending_id_once",
    "test_script_entrypoint_preserves_raw_utf8_stdin_fields",
]
PROBES = [
    ("early_exit_ack",
     '                    resources.close()\n                    session_reply({"success": True, "request_id": pending_id})',
     '                    session_reply({"success": True, "request_id": pending_id})\n                    resources.close()',
     TEST_NAMES[1:3]),
    ("extra_unowned_failure_json",
     '        print(error, file=sys.stderr)\n    return code',
     '        session_reply({"success": False, "request_id": pending_id, "error": error})\n        print(error, file=sys.stderr)\n    return code',
     [TEST_NAMES[0], TEST_NAMES[3], TEST_NAMES[4]]),
    ("swallow_exit_cleanup_failure",
     '                    resources.close()\n                    session_reply(',
     '                    try:\n                        resources.close()\n                    except Exception:\n                        pass\n                    session_reply(',
     [TEST_NAMES[1]]),
    ("wrong_interrupt_exit_code",
     '        code, error = 130, "Interrupted"',
     '        code, error = 1, "Interrupted"',
     TEST_NAMES[3:6]),
    ("retain_completed_pending_id",
     '                session_reply({**response, "request_id": pending_id})\n                pending_id = None',
     '                session_reply({**response, "request_id": pending_id})',
     [TEST_NAMES[4]]),
    ("discard_non_ascii_input",
     '                    message = json.loads(line)',
     '                    message = json.loads(line.encode("ascii", errors="ignore").decode("ascii"))',
     [TEST_NAMES[6]]),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source_path = ROOT / "scripts/light_whisper_cli.py"
    source = source_path.read_text(encoding="utf-8")
    support = importlib.import_module("scripts.test_light_whisper_cli")
    original_copy = shutil.copyfile
    sha = lambda data: hashlib.sha256(data).hexdigest()
    receipt = {"source_sha256": sha(source_path.read_bytes()),
               "test_sha256": sha((ROOT / "scripts/test_cli_session.py").read_bytes()),
               "runner_sha256": sha(Path(__file__).read_bytes()), "runs": []}

    def execute(name, names, variant=None):
        log = io.StringIO()
        suite = unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromName(
            "scripts.test_cli_session.SessionTests." + method) for method in names)
        with mock.patch.dict(sys.modules, {} if variant is None else {support.MODULE: variant}):
            result = unittest.TextTestRunner(stream=log, verbosity=2).run(suite)
        (output / (name + ".log")).write_text(log.getvalue(), encoding="utf-8")
        item = {"case": name, "tests": result.testsRun, "failures": len(result.failures),
                "errors": len(result.errors), "passed": result.wasSuccessful() if variant is None
                else bool(result.failures) and not result.errors}
        receipt["runs"].append(item)
        print(json.dumps(item))

    execute("baseline", TEST_NAMES)
    for name, before, after, names in PROBES:
        if source.count(before) != 1:
            raise RuntimeError(f"Source anchor drift for {name}: {source.count(before)} matches")
        changed = source.replace(before, after, 1)
        variant = types.ModuleType(support.MODULE)
        variant.__file__ = str(source_path)
        exec(compile(changed, str(source_path), "exec"), variant.__dict__)

        def copy_variant(src, dst, *positional, **keywords):
            if Path(src).resolve() == source_path.resolve():
                Path(dst).write_text(changed, encoding="utf-8")
                return str(dst)
            return original_copy(src, dst, *positional, **keywords)

        with mock.patch.object(shutil, "copyfile", side_effect=copy_variant):
            execute(name, names, variant)
        receipt["runs"][-1]["variant_sha256"] = sha(changed.encode("utf-8"))
        (output / (name + ".replacement.json")).write_text(
            json.dumps({"before": before, "after": after}, indent=2), encoding="utf-8")
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return 0 if all(item["passed"] for item in receipt["runs"]) else 1


if __name__ == "__main__":
    sys.exit(main())
