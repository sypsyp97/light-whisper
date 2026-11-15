

import os
import re
import shutil
import subprocess
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CANDIDATE = "a" * 40


class ReleaseCiTests(unittest.TestCase):
    def run_publication(self, fetch_change="none") -> subprocess.CompletedProcess:
        source = (ROOT / "scripts/release.sh").read_text(encoding="utf-8")
        function = re.search(r"^publish_release\(\) \{\n.*?^\}", source, re.M | re.S)
        self.assertIsNotNone(function)
        bash = (Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
                if os.name == "nt" else shutil.which("bash"))
        self.assertTrue(bash and Path(bash).is_file(), "Git Bash (Windows) or Bash is required")
        # Execute the actual function, but replace every Git/GitHub boundary.
        # fetch mutates this harness's state between the function's real observations.
        harness = r'''
set -euo pipefail
RELEASE_BRANCH=main
TAG=v1.8.0
RELEASE_NOTES=--generate-notes
INSTALLER=synthetic-installer.exe
CURRENT_HEAD="$1"
REMOTE_HEAD="$1"
CURRENT_DIRTY=''
fail() { printf '%s\n' "$*" >&2; exit 1; }
git() {
    case "$*" in
        'rev-parse HEAD') printf '%s\n' "$CURRENT_HEAD" ;;
        'status --porcelain') printf '%s' "$CURRENT_DIRTY" ;;
        'fetch --quiet origin main')
            case "$TEST_FETCH_CHANGE" in
                none) ;;
                head) CURRENT_HEAD="$TEST_OTHER_SHA" ;;
                dirty) CURRENT_DIRTY=' M synthetic-file' ;;
                remote) REMOTE_HEAD="$TEST_OTHER_SHA" ;;
                failure) return 17 ;;
                *) return 99 ;;
            esac ;;
        'rev-parse origin/main') printf '%s\n' "$REMOTE_HEAD" ;;
        'tag -a '*) printf 'EFFECT_TAG:%s\n' "$*" ;;
        'push origin refs/tags/v1.8.0') printf 'EFFECT_PUSH:%s\n' "$*" ;;
        *) printf 'Unexpected git invocation: %s\n' "$*" >&2; return 99 ;;
    esac
}
gh() {
    case "$*" in
        'release create '*) printf 'EFFECT_RELEASE:%s\n' "$*" ;;
        *) printf 'Unexpected gh invocation: %s\n' "$*" >&2; return 99 ;;
    esac
}
'''
        environment = dict(os.environ, TEST_FETCH_CHANGE=fetch_change, TEST_OTHER_SHA="b" * 40)
        return subprocess.run([str(bash), "-c", harness + function[0] + '\npublish_release "$1"',
                               "publication-test", CANDIDATE], env=environment, text=True,
                              encoding="utf-8", capture_output=True, timeout=15)

    def assert_no_publication_effects(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        for marker in ("EFFECT_TAG:", "EFFECT_PUSH:", "EFFECT_RELEASE:"):
            self.assertNotIn(marker, result.stdout)

    def test_stable_publication_tags_the_exact_candidate_before_release(self):
        result = self.run_publication()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"EFFECT_TAG:tag -a v1.8.0 {CANDIDATE} -m Release v1.8.0", result.stdout)
        self.assertLess(result.stdout.index("EFFECT_TAG:"), result.stdout.index("EFFECT_PUSH:"))
        self.assertLess(result.stdout.index("EFFECT_PUSH:"), result.stdout.index("EFFECT_RELEASE:"))

    def test_head_changed_during_fetch_blocks_tag_and_release(self):
        self.assert_no_publication_effects(self.run_publication("head"))

    def test_worktree_dirtied_during_fetch_blocks_tag_and_release(self):
        self.assert_no_publication_effects(self.run_publication("dirty"))

    def test_remote_changed_during_fetch_blocks_tag_and_release(self):
        self.assert_no_publication_effects(self.run_publication("remote"))

    def test_failed_fetch_blocks_tag_and_release(self):
        self.assert_no_publication_effects(self.run_publication("failure"))

    def run_gate(self, formal: str, head: str = CANDIDATE,
                 *, verus: str = "success", cli: str = "success",
                 frontend: str = "success", python: str = "success",
                 rust: str = "success") -> subprocess.CompletedProcess:
        source = (ROOT / "scripts/release.sh").read_text(encoding="utf-8")
        function = re.search(r"^wait_for_ci\(\) \{\n.*?^\}", source, re.M | re.S)
        self.assertIsNotNone(function)
        bash = (Path(os.environ.get("ProgramFiles", "C:/Program Files")) / "Git/bin/bash.exe"
                if os.name == "nt" else shutil.which("bash"))
        self.assertTrue(bash and Path(bash).is_file(), "Git Bash (Windows) or Bash is required")


        harness = r'''
set -euo pipefail
CI_WORKFLOW=ci.yml
CI_DISCOVERY_TIMEOUT_SECONDS=5
fail() { printf '%s\n' "$*" >&2; exit 1; }
gh() {
    case "$*" in
        'run list '*) printf '123\n' ;;
        'run watch '*) return 0 ;;
        *'--json headSha '*) printf '%s\n' "$TEST_RUN_HEAD" ;;
        *'--json conclusion '*) printf 'success\n' ;;
        *'--json jobs '*'Application formal contracts'*)
            if [[ "$TEST_FORMAL_RESULT" != missing ]]; then printf '%s\n' "$TEST_FORMAL_RESULT"; fi ;;
        *'--json jobs '*'Verus implementation proofs'*)
            if [[ "$TEST_VERUS_RESULT" != missing ]]; then printf '%s\n' "$TEST_VERUS_RESULT"; fi ;;
        *'--json jobs '*'CLI contracts'*)
            if [[ "$TEST_CLI_RESULT" != missing ]]; then printf '%s\n' "$TEST_CLI_RESULT"; fi ;;
        *'--json jobs '*'Frontend'*)
            if [[ "$TEST_FRONTEND_RESULT" != missing ]]; then printf '%s\n' "$TEST_FRONTEND_RESULT"; fi ;;
        *'--json jobs '*'Python'*)
            if [[ "$TEST_PYTHON_RESULT" != missing ]]; then printf '%s\n' "$TEST_PYTHON_RESULT"; fi ;;
        *'--json jobs '*'Rust'*)
            if [[ "$TEST_RUST_RESULT" != missing ]]; then printf '%s\n' "$TEST_RUST_RESULT"; fi ;;
        *) printf 'Unexpected gh invocation: %s\n' "$*" >&2; return 99 ;;
    esac
}
'''
        environment = dict(os.environ, TEST_FORMAL_RESULT=formal, TEST_RUN_HEAD=head,
                           TEST_VERUS_RESULT=verus, TEST_CLI_RESULT=cli,
                           TEST_FRONTEND_RESULT=frontend, TEST_PYTHON_RESULT=python,
                           TEST_RUST_RESULT=rust)
        return subprocess.run([str(bash), "-c", harness + function[0] + '\nwait_for_ci "$1"',
                               "release-ci-test", CANDIDATE], env=environment,
                              text=True, encoding="utf-8", capture_output=True, timeout=15)

    def test_all_six_required_jobs_pass_for_candidate(self):
        result = self.run_gate("success")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_formal_job_blocks_release(self):
        result = self.run_gate("missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_skipped_formal_job_blocks_release(self):
        result = self.run_gate("skipped")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_failed_formal_job_blocks_release(self):
        result = self.run_gate("failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_cancelled_formal_job_blocks_release(self):
        result = self.run_gate("cancelled")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Application formal contracts", result.stderr)

    def test_different_commit_blocks_release(self):
        result = self.run_gate("success", "b" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("headSha", result.stderr)

    def test_missing_verus_job_blocks_release(self):
        result = self.run_gate("success", verus="missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Verus implementation proofs", result.stderr)

    def test_skipped_verus_job_blocks_release(self):
        result = self.run_gate("success", verus="skipped")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Verus implementation proofs", result.stderr)

    def test_failed_verus_job_blocks_release(self):
        result = self.run_gate("success", verus="failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Verus implementation proofs", result.stderr)

    def test_cancelled_verus_job_blocks_release(self):
        result = self.run_gate("success", verus="cancelled")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Verus implementation proofs", result.stderr)

    def test_each_existing_job_still_blocks_release(self):
        for argument, name in (("frontend", "Frontend"), ("python", "Python"), ("rust", "Rust")):
            for status in ("missing", "skipped", "failure", "cancelled"):
                with self.subTest(job=name, status=status):
                    result = self.run_gate("success", **{argument: status})
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn(name, result.stderr)

    def test_missing_cli_job_blocks_release_despite_successful_aggregate(self):
        result = self.run_gate("success", cli="missing")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CLI contracts", result.stderr)

    def test_skipped_cli_job_blocks_release_despite_successful_aggregate(self):
        result = self.run_gate("success", cli="skipped")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CLI contracts", result.stderr)

    def test_failed_cli_job_blocks_release_despite_successful_aggregate(self):
        result = self.run_gate("success", cli="failure")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CLI contracts", result.stderr)

    def test_cancelled_cli_job_blocks_release_despite_successful_aggregate(self):
        result = self.run_gate("success", cli="cancelled")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("CLI contracts", result.stderr)

    def test_all_six_successes_on_another_sha_still_block_release(self):
        result = self.run_gate("success", "b" * 40, cli="success")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("headSha", result.stderr)


if __name__ == "__main__":
    unittest.main()
