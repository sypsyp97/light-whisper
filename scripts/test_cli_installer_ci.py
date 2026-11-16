import itertools
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github/workflows/installer-smoke.yml"
DRIVER = ROOT / "scripts/test_installer_command.ps1"


class InstallerWorkflowTests(unittest.TestCase):
    def test_uninstall_process_guard_precedes_command_cleanup(self):
        source = (ROOT / "src-tauri/nsis-hooks.nsh").read_text(encoding="utf-8")
        hook = re.search(r"!macro\s+NSIS_HOOK_PREUNINSTALL\b(.*?)!macroend", source, re.S)
        self.assertIsNotNone(hook, "The command uninstall hook is missing")
        cleanup = re.search(r'!insertmacro\s+LIGHT_WHISPER_COMMAND\s+"-Uninstall"', hook[1])
        self.assertIsNotNone(cleanup, "The command cleanup is missing")
        guard = re.search(r'!insertmacro\s+CheckIfAppIsRunning\s+"\$\{MAINBINARYNAME\}\.exe"'
                          r'\s+"\$\{PRODUCTNAME\}"', hook[1])
        self.assertIsNotNone(guard, "Uninstall cancellation must be decided before alias/PATH cleanup")
        self.assertLess(guard.start(), cleanup.start(), "Uninstall cancellation must preserve the installed command")

    def test_dispatch_binds_candidate_asset_and_hash_to_fresh_windows_smoke(self):
        self.assertTrue(WORKFLOW.is_file(), "INTERFACE_PENDING: installer smoke workflow is not declared")
        source = WORKFLOW.read_text(encoding="utf-8")
        self.assertRegex(source, r"(?m)^\s+workflow_dispatch:\s*$")
        for name in ("asset_id", "sha256", "candidate_sha", "expected_version"):
            self.assertRegex(source, rf"(?m)^\s+{name}:\s*$")
        self.assertRegex(source, r"(?m)^permissions:\s*\n\s+contents:\s*read\s*$")
        self.assertNotRegex(source, r"(?m)^\s*[\w-]+:\s*write\s*$")
        self.assertRegex(source, r"(?m)^\s+runs-on:\s*windows-latest\s*$")
        self.assertRegex(source, r"(?m)^\s+timeout-minutes:\s*10\s*$")
        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        pinned_checkout = re.search(r"actions/checkout@[0-9a-f]{40}", ci)[0]
        self.assertIn(pinned_checkout, source)
        self.assertRegex(source, r"ref:\s*\$\{\{\s*(?:github\.event\.)?inputs\.candidate_sha\s*\}\}")
        self.assertRegex(source, r"persist-credentials:\s*false")
        for variable, field in (("ASSET_ID", "asset_id"), ("EXPECTED_SHA256", "sha256"),
                                ("CANDIDATE_SHA", "candidate_sha"), ("EXPECTED_VERSION", "expected_version")):
            self.assertRegex(source, rf"{variable}:\s*\$\{{\{{\s*(?:github\.event\.)?inputs\.{field}\s*\}}\}}")
        self.assertRegex(source, r"GH_TOKEN:\s*\$\{\{\s*(?:github\.token|secrets\.GITHUB_TOKEN)\s*\}\}")
        run_blocks = re.findall(r"(?m)^([ ]+)run:\s*\|\s*\n((?:\1[ ]+[^\n]*\n|[ \t]*\n)*)", source)
        self.assertTrue(run_blocks, "Installer download and smoke invocation are missing")
        scripts = "\n".join(body for _, body in run_blocks)
        self.assertNotIn("${{", scripts, "Dispatch input must enter PowerShell through environment variables")
        self.assertIn("Invoke-WebRequest", scripts)
        self.assertIn("application/octet-stream", scripts)
        self.assertIn("releases/assets/", scripts)
        self.assertIn("$env:GITHUB_REPOSITORY", scripts)
        self.assertIn("$env:ASSET_ID", scripts)
        self.assertRegex(scripts, r"(?i)-OutFile\b")
        self.assertIn("scripts/test_installer_command.ps1", scripts.replace("\\", "/"))
        self.assertRegex(scripts, r"-InstallerPath\s+\$")
        self.assertRegex(scripts, r"-ExpectedVersion\s+\$env:EXPECTED_VERSION")
        self.assertRegex(scripts, r"-ExpectedSha256\s+\$env:EXPECTED_SHA256")
        for expression in (r"\^\[0-9\]\+\$", r"\^\[0-9a-fA-F\]\{64\}\$", r"\^\[0-9a-fA-F\]\{40\}\$"):
            self.assertRegex(scripts, expression)
        for action in re.findall(r"(?m)^\s+uses:\s*(\S+)", source):
            self.assertRegex(action, r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell parsers")
class InstallerDriverSafetyTests(unittest.TestCase):
    def test_both_powershell_versions_parse_and_all_nonfresh_states_refuse_before_effects(self):
        shells = (Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe",
                  Path(shutil.which("pwsh")))
        # Extract the actual pure guard from the AST; never invoke the installer driver or real registry writes.
        for shell in shells:
            with self.subTest(shell=shell.name):
                script = r'''
$ErrorActionPreference = 'Stop'
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($env:SMOKE_DRIVER, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw 'The native smoke driver does not parse.' }
$guard = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -eq 'Assert-FreshInstallerRunner' }, $true)
if ($null -eq $guard) { throw 'The fresh-runner guard is missing.' }
Invoke-Expression $guard.Extent.Text
$results = @()
foreach ($hosted in @($false, $true)) {
    foreach ($root in @($false, $true)) {
        foreach ($key in @($false, $true)) {
            foreach ($command in @($false, $true)) {
                $accepted = $true
                try { Assert-FreshInstallerRunner $hosted $root $key $command }
                catch { $accepted = $false }
                $results += $accepted
            }
        }
    }
}
$results | ConvertTo-Json -Compress
'''
                result = subprocess.run([str(shell), "-NoProfile", "-NonInteractive", "-Command", script],
                                        env=dict(os.environ, SMOKE_DRIVER=str(DRIVER)), capture_output=True,
                                        text=True, encoding="utf-8", timeout=15)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = [hosted and not (root or key or command)
                            for hosted, root, key, command in itertools.product((False, True), repeat=4)]
                self.assertEqual(json.loads(result.stdout), expected)


if __name__ == "__main__":
    unittest.main()
