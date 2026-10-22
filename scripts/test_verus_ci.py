"""Exercise the real Verus CI step without downloads or toolchain installation."""

import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from test_ci_fail_fast import step_script

ROOT = Path(__file__).resolve().parents[1]
STEP = "Verify production Rust contracts"
RELEASE = "0.2026.10.18.7d82994"
ARCHIVE_SHA256 = "82b9558a3c4fa77673700507c066c711535d130c2789dcfcec30878168db20ec"


class VerusIntegrationTests(unittest.TestCase):
    def test_required_job_and_local_entry(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        jobs = re.split(r"(?m)^  [A-Za-z0-9_-]+:\n", workflow)
        job = next((part for part in jobs
                    if "    name: Verus implementation proofs\n" in part), None)
        self.assertIsNotNone(job, "INTERFACE_PENDING: Verus CI job")
        self.assertIn("    runs-on: windows-latest\n", job)
        self.assertRegex(job, r"(?m)^    timeout-minutes: [1-9][0-9]*$")
        self.assertIn("permissions:\n  contents: read\n", workflow)
        self.assertNotRegex(job, r"(?m)^ +[a-z-]+: write$")
        for action in (
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1",
            "actions/setup-python@5fda3b95a4ea91299a34e894583c3862153e4b97",
        ):
            self.assertIn(action, job)
        self.assertIn("persist-credentials: false", job)
        self.assertIn("python-version-file: .python-version", job)
        uploads = [part for part in job.split("      - ")
                   if "uses: actions/upload-artifact@" in part]
        self.assertEqual(len(uploads), 1)
        self.assertIn("if: always()", uploads[0])
        self.assertIn("${{ runner.temp }}/verus-results", uploads[0])
        package = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))
        self.assertEqual(package["scripts"].get("check:verus"), "python scripts/check_verus.py")


@unittest.skipUnless(shutil.which("pwsh"), "requires PowerShell, as used by Windows CI")
class VerusStepTests(unittest.TestCase):
    def run_step(self, failure=""):
        try:
            script = step_script(STEP)
        except IndexError as error:
            raise RuntimeError(f"INTERFACE_PENDING: {STEP}") from error
        python = str(Path(sys.executable)).replace("'", "''")
        with tempfile.TemporaryDirectory(prefix="verus-ci-") as directory:
            root = Path(directory)
            calls_path = root / "calls.jsonl"
            environment = dict(os.environ, RUNNER_TEMP=str(root),
                               VERUS_TEST_CALLS=str(calls_path), VERUS_TEST_FAILURE=failure)
            # Native failures come from the current Python interpreter. Every
            # download, extraction, installation, and proof command is mocked.
            mock = f"""
function Write-TestCall($kind, $arguments) {{
  @{{ kind = $kind; arguments = @($arguments); rustup = $env:RUSTUP_HOME;
      cargo = $env:CARGO_HOME }} | ConvertTo-Json -Compress -Depth 5 |
    Add-Content -LiteralPath $env:VERUS_TEST_CALLS
}}
function Invoke-TestNative($kind, $arguments) {{
  Write-TestCall $kind $arguments
  $code = if ($env:VERUS_TEST_FAILURE -eq $kind) {{ 17 }} else {{ 0 }}
  & '{python}' -c "import sys; sys.exit($code)"
}}
function Invoke-WebRequest {{
  param($Uri, $OutFile)
  Write-TestCall 'download' @($Uri, $OutFile)
  [IO.File]::WriteAllText($OutFile, 'fake archive')
}}
function Get-FileHash {{
  param([Alias('LiteralPath')]$Path, $Algorithm)
  Write-TestCall 'hash' @($Path, $Algorithm)
  $hash = if ($env:VERUS_TEST_FAILURE -eq 'checksum') {{ '0' * 64 }} else {{ '{ARCHIVE_SHA256}' }}
  [pscustomobject]@{{ Hash = $hash }}
}}
function Expand-Archive {{
  param([Alias('LiteralPath')]$Path, $DestinationPath, [switch]$Force)
  Write-TestCall 'extract' @($Path, $DestinationPath)
  $directory = Join-Path $DestinationPath 'verus-x86-win'
  New-Item -ItemType Directory -Force -Path $directory | Out-Null
  [IO.File]::WriteAllText((Join-Path $directory 'verus.exe'), 'fake verifier')
}}
function rustup {{ Invoke-TestNative 'rustup' $args }}
function python {{
  $kind = if ($args -contains 'scripts/check_verus.py') {{ 'proof' }}
          elseif ($args -contains 'scripts/test_check_verus.py' -or
                  $args -contains 'test_check_verus.py') {{ 'runner-tests' }} else {{ 'python' }}
  Invoke-TestNative $kind $args
}}
{script}
Write-TestCall 'complete' @()
exit 0
"""
            result = subprocess.run(
                [shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-Command", mock],
                cwd=ROOT, env=environment, capture_output=True, text=True, timeout=30,
            )
            calls = ([json.loads(line) for line in calls_path.read_text(encoding="utf-8-sig").splitlines()]
                     if calls_path.exists() else [])
            return result, calls, root

    def test_success_uses_pinned_archive_isolated_toolchain_and_real_runner(self):
        result, calls, root = self.run_step()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        kinds = [call["kind"] for call in calls]
        for kind in ("download", "hash", "extract", "rustup", "runner-tests", "proof", "complete"):
            self.assertEqual(kinds.count(kind), 1, kinds)
        self.assertLess(kinds.index("hash"), kinds.index("extract"))
        self.assertLess(kinds.index("extract"), kinds.index("proof"))
        by_kind = {call["kind"]: call for call in calls}
        self.assertEqual(by_kind["download"]["arguments"][0],
                         f"https://github.com/verus-lang/verus/releases/download/release/{RELEASE}/"
                         f"verus-{RELEASE}-x86-win.zip")
        self.assertEqual(by_kind["hash"]["arguments"][0], by_kind["download"]["arguments"][1])
        self.assertEqual(by_kind["hash"]["arguments"][1].upper(), "SHA256")
        self.assertEqual(by_kind["extract"]["arguments"][0], by_kind["download"]["arguments"][1])
        install = by_kind["rustup"]["arguments"]
        self.assertEqual(install[:3], ["toolchain", "install", "1.99.1"])
        self.assertIn("--no-self-update", install)
        self.assertEqual(install[install.index("--profile") + 1], "minimal")
        for kind in ("rustup", "proof"):
            for variable in ("rustup", "cargo"):
                self.assertIsNotNone(by_kind[kind][variable])
                self.assertTrue(Path(by_kind[kind][variable]).is_relative_to(root))
            self.assertNotEqual(by_kind[kind]["rustup"], by_kind[kind]["cargo"])
        self.assertEqual(by_kind["rustup"]["rustup"], by_kind["proof"]["rustup"])
        self.assertEqual(by_kind["rustup"]["cargo"], by_kind["proof"]["cargo"])
        arguments = by_kind["proof"]["arguments"]
        self.assertEqual(arguments[0], "scripts/check_verus.py")
        self.assertEqual(Path(arguments[arguments.index("--verus") + 1]),
                         Path(by_kind["extract"]["arguments"][1]) / "verus-x86-win/verus.exe")
        self.assertEqual(Path(arguments[arguments.index("--logs") + 1]), root / "verus-results")

    def test_checksum_mismatch_stops_before_extraction(self):
        result, calls, _ = self.run_step("checksum")
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        kinds = [call["kind"] for call in calls]
        self.assertIn("hash", kinds)
        for forbidden in ("extract", "rustup", "proof", "complete"):
            self.assertNotIn(forbidden, kinds)

    def test_native_failures_stop_the_step(self):
        for failure in ("runner-tests", "rustup", "proof"):
            with self.subTest(failure=failure):
                result, calls, _ = self.run_step(failure)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertTrue(calls, result.stderr)
                self.assertEqual(calls[-1]["kind"], failure)


if __name__ == "__main__":
    unittest.main()
