import itertools
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
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
        jobs = dict(re.findall(r"(?ms)^  ([\w-]+):\s*\n(.*?)(?=^  [\w-]+:\s*\n|\Z)",
                               source.split("\njobs:\n", 1)[1]))
        self.assertEqual(set(jobs), {"download", "command"},
                         "Draft-asset download must be isolated from installed-code execution")
        download, command = jobs["download"], jobs["command"]
        self.assertRegex(download, r"(?m)^    runs-on:\s*ubuntu-latest\s*$")
        self.assertRegex(command, r"(?m)^    runs-on:\s*windows-latest\s*$")
        for job in (download, command):
            self.assertRegex(job, r"(?m)^    timeout-minutes:\s*10\s*$")
        self.assertRegex(download, r"(?m)^    permissions:\s*\n      contents:\s*write\s*$")
        writes = r"(?m)^\s*([\w-]+):\s*write\s*$"
        self.assertEqual(re.findall(writes, source), ["contents"], "Only the draft downloader may write")
        self.assertEqual(re.findall(writes, download), ["contents"])
        self.assertNotRegex(command, writes, "Installed code must retain read-only repository permissions")
        self.assertNotRegex(command, r"(?m)^    permissions:", "The command job must inherit workflow contents: read")
        self.assertRegex(command, r"(?m)^    needs:\s*download\s*$")
        self.assertNotIn("GH_TOKEN", command)
        self.assertNotRegex(command, r"(?:github\.token|secrets\.GITHUB_TOKEN)")
        token_bindings = re.findall(r"(?m)^( *)GH_TOKEN:\s*\$\{\{\s*(?:github\.token|secrets\.GITHUB_TOKEN)\s*\}\}", source)
        self.assertEqual(token_bindings, [" " * 10], "The token belongs only to the downloader step environment")
        self.assertEqual(len(re.findall(r"\$\{\{\s*(?:github\.token|secrets\.GITHUB_TOKEN)\s*\}\}", source)), 1)
        self.assertRegex(download, r"(?m)^          GH_TOKEN:\s*\$\{\{\s*(?:github\.token|secrets\.GITHUB_TOKEN)\s*\}\}")
        ci = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        pinned_checkout = re.search(r"actions/checkout@[0-9a-f]{40}", ci)[0]
        self.assertIn(pinned_checkout, command)
        self.assertNotIn("actions/checkout@", download)
        self.assertRegex(command, r"ref:\s*\$\{\{\s*(?:github\.event\.)?inputs\.candidate_sha\s*\}\}")
        self.assertRegex(command, r"persist-credentials:\s*false")
        for variable, field in (("ASSET_ID", "asset_id"), ("EXPECTED_SHA256", "sha256"),
                                ("CANDIDATE_SHA", "candidate_sha"), ("EXPECTED_VERSION", "expected_version")):
            self.assertRegex(source, rf"{variable}:\s*\$\{{\{{\s*(?:github\.event\.)?inputs\.{field}\s*\}}\}}")
        run_pattern = r"(?m)^([ ]+)run:\s*\|\s*\n((?:\1[ ]+[^\n]*\n|[ \t]*\n)*)"
        download_scripts = "\n".join(body for _, body in re.findall(run_pattern, download))
        command_scripts = "\n".join(body for _, body in re.findall(run_pattern, command))
        self.assertTrue(download_scripts and command_scripts, "Download and installed-command scripts are required")
        scripts = download_scripts + "\n" + command_scripts
        self.assertNotIn("${{", scripts, "Dispatch input must enter PowerShell through environment variables")
        for value in ("Invoke-WebRequest", "application/octet-stream", "releases/assets/",
                      "$env:GITHUB_REPOSITORY", "$env:ASSET_ID", "Get-FileHash"):
            self.assertIn(value, download_scripts)
        self.assertRegex(download_scripts, r"(?i)-OutFile\b")
        self.assertRegex(download_scripts, r"(?i)-Algorithm\s+SHA256")
        self.assertRegex(download_scripts, r"-(?:c|i)?ne\s+\$env:EXPECTED_SHA256")
        self.assertNotRegex(download_scripts, r"(?im)^\s*(?:&|\.\s|Start-Process\b|Invoke-Expression\b|iex\b)")
        self.assertNotIn("test_installer_command.ps1", download)
        self.assertNotIn("Invoke-WebRequest", command_scripts)
        self.assertIn("git rev-parse HEAD", command_scripts)
        self.assertRegex(command_scripts, r"-(?:c|i)?ne\s+\$env:CANDIDATE_SHA")
        self.assertIn("scripts/test_installer_command.ps1", command_scripts.replace("\\", "/"))
        self.assertRegex(command_scripts, r"-InstallerPath\s+\$")
        self.assertRegex(command_scripts, r"-ExpectedVersion\s+\$env:EXPECTED_VERSION")
        self.assertRegex(command_scripts, r"-ExpectedSha256\s+\$env:EXPECTED_SHA256")
        self.assertIn("candidate-installer", command_scripts)
        upload = "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"
        self.assertEqual(re.findall(r"(?m)^\s+uses:\s*(\S+)", download), [upload])
        for setting in (r"name:\s*candidate-installer", r"compression-level:\s*0", r"retention-days:\s*1"):
            self.assertRegex(download, r"(?m)^          " + setting + r"\s*$")
        self.assertRegex(download, r"path:\s*\$\{\{\s*runner\.temp\s*\}\}/candidate-installer(?:/[^\n]+)?\s*(?:\n|$)")
        artifact_download = "actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333"
        self.assertIn(artifact_download, command)
        artifact_steps = [step for step in re.split(r"(?m)^      - ", command) if artifact_download in step]
        self.assertEqual(len(artifact_steps), 1)
        self.assertRegex(artifact_steps[0], r"(?m)^          name:\s*candidate-installer\s*$")
        self.assertRegex(artifact_steps[0], r"path:\s*\$\{\{\s*runner\.temp\s*\}\}/candidate-installer\s*(?:\n|$)")
        self.assertNotRegex(artifact_steps[0], r"(?m)^\s+(?:github-token|run-id|repository):")
        summary_steps = [step for step in re.split(r"(?m)^      - ", command) if upload in step]
        self.assertEqual(len(summary_steps), 1)
        self.assertRegex(summary_steps[0], r"(?m)^          name:\s*installer-command-summary\s*$")
        self.assertRegex(summary_steps[0], r"path:\s*\$\{\{\s*runner\.temp\s*\}\}/installer-command-summary\.json")
        self.assertRegex(summary_steps[0], r"if:\s*always\(\)")
        for expression in (r"\^\[0-9\]\+\$", r"\^\[0-9a-fA-F\]\{64\}\$", r"\^\[0-9a-fA-F\]\{40\}\$"):
            self.assertRegex(scripts, expression)
        for action in re.findall(r"(?m)^\s+uses:\s*(\S+)", source):
            self.assertRegex(action, r"^[\w.-]+/[\w./-]+@[0-9a-f]{40}$")


@unittest.skipUnless(os.name == "nt" and shutil.which("pwsh"), "requires Windows PowerShell parsers")
class InstallerDriverSafetyTests(unittest.TestCase):
    def test_cmd_preserves_unicode_lookup_and_nonzero_exit_from_oem_start(self):
        with tempfile.TemporaryDirectory(prefix="lw-installer-cmd-") as temporary:
            directory = Path(temporary).resolve()
            alias = directory / "安装 轻语" / "light-whisper.exe"
            alias.parent.mkdir()
            alias.write_bytes(b"synthetic where-only fixture; never execute")
            script = r'''
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$tokens = $null
$errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($env:SMOKE_DRIVER, [ref]$tokens, [ref]$errors)
if ($errors.Count -ne 0) { throw 'The native smoke driver does not parse.' }
$function = $ast.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Name -eq 'Invoke-ShellCommand' }, $true)
if ($null -eq $function) { throw 'The actual shell-command function is missing.' }
Invoke-Expression $function.Extent.Text
function Invoke-CapturedProcess {
    param([string]$Executable, [string]$Arguments, [string]$Label)
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    # Force the initial OEM page before the actual function's command, including its prefix.
    $start.Arguments = $Arguments.Insert($Arguments.IndexOf('"') + 1, 'chcp 437>nul & ')
    $start.WorkingDirectory = $env:SMOKE_WORK
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.EnvironmentVariables['PATH'] = $env:SMOKE_BIN + ';' + (Join-Path $env:SystemRoot 'System32')
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        $process.Start() | Out-Null
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(5000)) { $process.Kill($true); throw 'Owned cmd fixture exceeded its deadline.' }
        return [pscustomobject]@{ Code = $process.ExitCode; Output = $stdout.GetAwaiter().GetResult();
                                 Errors = $stderr.GetAwaiter().GetResult() }
    } finally { $process.Dispose() }
}
$shell = @{ Name = 'cmd'; Path = $env:ComSpec }
$lookup = Invoke-ShellCommand $shell 'where.exe light-whisper' 'lookup'
$invalid = Invoke-ShellCommand $shell 'exit /b 2' 'exit-code'
@{ lookup_code = $lookup.Code; path = $lookup.Output.Trim(); errors = $lookup.Errors;
   invalid_code = $invalid.Code } | ConvertTo-Json -Compress
'''
            result = subprocess.run([shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-Command", script],
                                    env=dict(os.environ, SMOKE_DRIVER=str(DRIVER), SMOKE_WORK=str(directory),
                                             SMOKE_BIN=str(alias.parent)), capture_output=True,
                                    text=True, encoding="utf-8", timeout=15,
                                    creationflags=subprocess.CREATE_NO_WINDOW)
            self.assertEqual(result.returncode, 0, result.stderr)
            observed = json.loads(result.stdout)
            self.assertEqual(observed["lookup_code"], 0)
            self.assertEqual(observed["invalid_code"], 2)
            self.assertEqual(observed["errors"], "")
            self.assertEqual(observed["path"], str(alias))

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
