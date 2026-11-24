param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$ExpectedVersion,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedSha256,
    [string]$SummaryPath
)

$ErrorActionPreference = 'Stop'

function Assert-FreshInstallerRunner {
    param([bool]$HostedWindowsRunner, [bool]$InstallRootExists,
          [bool]$UninstallKeyExists, [bool]$CommandAvailable)
    if (-not $HostedWindowsRunner -or $InstallRootExists -or $UninstallKeyExists -or $CommandAvailable) {
        throw 'A fresh GitHub-hosted Windows runner with no Light Whisper installation is required.'
    }
}

function Get-UserPathState {
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
    try {
        if ($null -eq $key -or $null -eq $key.GetValue('Path', $null)) {
            return [pscustomobject]@{ Exists = $false; Value = ''; Kind = $null }
        }
        $kind = $key.GetValueKind('Path')
        if ($kind -notin @([Microsoft.Win32.RegistryValueKind]::String,
                          [Microsoft.Win32.RegistryValueKind]::ExpandString)) {
            throw 'The existing user PATH must be a string registry value.'
        }
        return [pscustomobject]@{
            Exists = $true
            Value = [string]$key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            Kind = $kind
        }
    } finally { if ($null -ne $key) { $key.Dispose() } }
}

function Assert-CommandPath {
    param($Original, [bool]$Installed)
    $current = Get-UserPathState
    $foreign = [Collections.Generic.List[string]]::new()
    $ownCount = 0
    if ($current.Value -cne '') {
        foreach ($token in $current.Value.Split(';')) {
            if ($token.Trim().Trim('"').Replace('/', '\').TrimEnd('\') -ieq $bin) { $ownCount++ }
            else { $foreign.Add($token) }
        }
    }
    $expected = if ($Original.Value -ceq '') { @() } else { @($Original.Value.Split(';')) }
    if ($ownCount -ne [int]$Installed -or $foreign.Count -ne $expected.Count) {
        throw 'The installer changed unrelated user PATH tokens or command entry count.'
    }
    for ($i = 0; $i -lt $expected.Count; $i++) {
        if ($foreign[$i] -cne $expected[$i]) { throw 'An unrelated raw user PATH token changed.' }
    }
    if ($Original.Exists -and $current.Kind -ne $Original.Kind) { throw 'The user PATH registry kind changed.' }
}

function Invoke-CapturedProcess {
    param([string]$Executable, [string]$Arguments, [string]$Label, [int]$TimeoutSeconds = 60)
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Executable
    $start.Arguments = $Arguments
    $start.WorkingDirectory = $work
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    # Read registry scopes, not this process's stale installation-time PATH.
    $start.EnvironmentVariables['PATH'] = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
        [Environment]::GetEnvironmentVariable('Path', 'User')
    $start.EnvironmentVariables['LIGHT_WHISPER_DATA_DIR'] = Join-Path $work 'fresh-config'
    $start.EnvironmentVariables['PYTHONIOENCODING'] = 'utf-8'
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $start
    try {
        if (-not $process.Start()) { throw 'A smoke-test process could not start.' }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
            # Stop only this owned process tree, including any child it started.
            & (Join-Path $env:SystemRoot 'System32\taskkill.exe') /PID $process.Id /T /F *> $null
            if (-not $process.WaitForExit(5000)) { throw 'An owned smoke-test process could not be stopped.' }
            throw 'A smoke-test process exceeded its deadline.'
        }
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $work ($Label + '.stdout.txt')), $output)
        [IO.File]::WriteAllText((Join-Path $work ($Label + '.stderr.txt')), $errors)
        return [pscustomobject]@{ Code = $process.ExitCode; Output = $output }
    } finally { $process.Dispose() }
}

function Invoke-ShellCommand {
    param($Shell, [string]$Command, [string]$Label)
    if ($Shell.Name -eq 'cmd') {
        return Invoke-CapturedProcess $Shell.Path ('/d /s /c "chcp 65001>nul & ' + $Command + '"') $Label
    }
    $script = '[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); ' +
        '$PSNativeCommandUseErrorActionPreference = $false; ' + $Command + '; exit $LASTEXITCODE'
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
    return Invoke-CapturedProcess $Shell.Path ('-NoLogo -NoProfile -NonInteractive -EncodedCommand ' + $encoded) $Label
}

function Assert-InstalledCommands {
    param([string]$Phase)
    foreach ($file in @($desktop, $console, $alias, $uninstaller)) {
        if (-not [IO.File]::Exists($file)) { throw 'An expected installed executable is missing.' }
    }
    if ((Get-FileHash -LiteralPath $console).Hash -cne (Get-FileHash -LiteralPath $alias).Hash) {
        throw 'The command alias differs from the packaged console executable.'
    }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
    try {
        if ($null -eq $key -or $key.GetValue('DisplayVersion') -cne $ExpectedVersion) {
            throw 'The installed registry version does not match the candidate.'
        }
    } finally { if ($null -ne $key) { $key.Dispose() } }
    Assert-CommandPath $originalPath $true
    foreach ($shell in $shells) {
        $prefix = $Phase + '-' + $shell.Name
        $lookup = if ($shell.Name -eq 'cmd') { 'where.exe light-whisper' }
                  else { '(Get-Command light-whisper -CommandType Application -ErrorAction Stop).Path' }
        $resolved = Invoke-ShellCommand $shell $lookup ($prefix + '-lookup')
        if ($resolved.Code -ne 0 -or $resolved.Output.Trim() -ine $alias) {
            throw 'A fresh shell did not resolve the installed console alias.'
        }
        $result = Invoke-ShellCommand $shell 'light-whisper doctor' ($prefix + '-doctor')
        if ($result.Code -ne 0) { throw 'The installed plain command doctor failed.' }
        $doctor = $result.Output | ConvertFrom-Json
        if ($doctor.success -ne $true -or $doctor.cli_runtime -cne 'rust' -or $doctor.frozen -ne $false -or
            $doctor.version -cne $ExpectedVersion -or $doctor.target -cne 'app' -or
            $doctor.runtime -ine $desktop -or $doctor.runtime_present -ne $true -or
            $doctor.app_protocol_supported -ne $true -or $doctor.model_load_checked -ne $false) {
            throw 'The native CLI version or actual desktop discovery contract failed.'
        }
        $help = Invoke-ShellCommand $shell 'light-whisper --help' ($prefix + '-help')
        if ($help.Code -ne 0 -or [string]::IsNullOrWhiteSpace($help.Output)) { throw 'Plain command help failed.' }
        $invalid = Invoke-ShellCommand $shell 'light-whisper --installer-smoke-invalid' ($prefix + '-invalid')
        if ($invalid.Code -ne 2 -or ($invalid.Output | ConvertFrom-Json).success -ne $false) {
            throw 'Plain command invalid-argument handling failed.'
        }
        $summary[$Phase + '_' + $shell.Name] = $true
    }
}

# These values follow tauri.conf.json and the pinned Tauri 2.10.0 NSIS template.
# Native smoke supplements formal/cli-command-entry; it is not another formal proof.
$productName = ([char]0x8f7b).ToString() + [char]0x8bed + ' Whisper'
$installRoot = Join-Path $env:LOCALAPPDATA $productName
$bin = Join-Path $installRoot 'bin'
$alias = Join-Path $bin 'light-whisper.exe'
$desktop = Join-Path $installRoot 'light-whisper.exe'
$console = Join-Path $installRoot 'light-whisper-cli.exe'
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$uninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\' + $productName
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
$registered = $null -ne $key
if ($null -ne $key) { $key.Dispose() }
$hosted = $env:GITHUB_ACTIONS -ceq 'true' -and $env:RUNNER_OS -ceq 'Windows' -and
    $env:RUNNER_ENVIRONMENT -ceq 'github-hosted'
# All four guards run before any installer, directory creation, registry write or summary output.
Assert-FreshInstallerRunner $hosted ($null -ne (Get-Item -LiteralPath $installRoot -Force -ErrorAction SilentlyContinue)) `
    $registered ($null -ne (Get-Command light-whisper -ErrorAction SilentlyContinue))
$originalPath = Get-UserPathState
Assert-CommandPath $originalPath $false
$installer = (Get-Item -LiteralPath $InstallerPath).FullName
$installerHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
if ($installerHash -cne $ExpectedSha256.ToLowerInvariant()) { throw 'The candidate installer SHA-256 does not match.' }
$shells = @(
    @{ Name = 'powershell5'; Path = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe' },
    @{ Name = 'powershell7'; Path = (Get-Command pwsh -CommandType Application -ErrorAction Stop).Source },
    @{ Name = 'cmd'; Path = $env:ComSpec }
)
if (-not $env:RUNNER_TEMP) { throw 'A runner-owned temporary directory is required.' }
if (-not $SummaryPath) { $SummaryPath = Join-Path $env:RUNNER_TEMP 'installer-smoke-summary.json' }
$work = Join-Path $env:RUNNER_TEMP ('light-whisper-installer-smoke-' + [guid]::NewGuid().ToString('N'))
$summary = [ordered]@{
    success = $false; expected_version = $ExpectedVersion; installer_sha256 = $installerHash
    fresh_preconditions = $true; installed = $false; upgraded = $false
    install_powershell5 = $false; install_powershell7 = $false; install_cmd = $false
    upgrade_powershell5 = $false; upgrade_powershell7 = $false; upgrade_cmd = $false
    uninstalled = $false; path_preserved = $false; user_data_preserved = $false; cleanup_complete = $false
}
$started = $false
$removed = $false
$sentinels = @()
$stage = 'setup'
$failed = $false
try {
    [IO.Directory]::CreateDirectory($work) | Out-Null
    foreach ($directory in @((Join-Path $env:APPDATA 'com.light-whisper.app'),
                             (Join-Path $env:APPDATA 'com.light-whisper.desktop'),
                             (Join-Path $env:LOCALAPPDATA 'com.light-whisper.desktop'))) {
        $existed = [IO.Directory]::Exists($directory)
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $file = Join-Path $directory ('installer-smoke-' + [guid]::NewGuid().ToString('N') + '.txt')
        [IO.File]::WriteAllText($file, 'synthetic installer preservation sentinel')
        $sentinels += @{ File = $file; Directory = $directory; Existed = $existed; Hash = (Get-FileHash -LiteralPath $file).Hash }
    }
    $stage = 'install'
    $started = $true
    $result = Invoke-CapturedProcess $installer '/S' 'install' 180
    if ($result.Code -ne 0) { throw 'Silent candidate installation failed.' }
    Assert-InstalledCommands 'install'
    $summary.installed = $true
    $stage = 'upgrade'
    $result = Invoke-CapturedProcess $installer '/S /UPDATE' 'upgrade' 180
    if ($result.Code -ne 0) { throw 'Silent candidate upgrade failed.' }
    Assert-InstalledCommands 'upgrade'
    $summary.upgraded = $true
    $stage = 'uninstall'
    # NSIS _?= is last and unquoted so it waits in-place rather than detaching a temp uninstaller.
    $result = Invoke-CapturedProcess $uninstaller ('/S _?=' + $installRoot) 'uninstall' 120
    if ($result.Code -ne 0) { throw 'Silent candidate uninstall failed.' }
    $removed = $true
    if ([IO.File]::Exists($alias) -or [IO.File]::Exists($desktop)) { throw 'An installed command or desktop executable remains.' }
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
    if ($null -ne $key) { $key.Dispose(); throw 'The product uninstall registry key remains.' }
    Assert-CommandPath $originalPath $false
    foreach ($shell in $shells) {
        $lookup = if ($shell.Name -eq 'cmd') { 'where.exe light-whisper' }
                  else { 'if (Get-Command light-whisper -ErrorAction SilentlyContinue) { exit 0 }; exit 1' }
        if ((Invoke-ShellCommand $shell $lookup ('removed-' + $shell.Name)).Code -eq 0) {
            throw 'A fresh shell still resolves the uninstalled command.'
        }
    }
    $summary.uninstalled = $true
    $summary.path_preserved = $true
    foreach ($sentinel in $sentinels) {
        if (-not [IO.File]::Exists($sentinel.File) -or (Get-FileHash -LiteralPath $sentinel.File).Hash -cne $sentinel.Hash) {
            throw 'Uninstallation changed a synthetic user-data preservation sentinel.'
        }
    }
    $summary.user_data_preserved = $true
} catch {
    $failed = $true
    if ([IO.Directory]::Exists($work)) { [IO.File]::WriteAllText((Join-Path $work 'failure.txt'), $_.Exception.ToString()) }
} finally {
    try {
        if ($started -and -not $removed) {
            if (-not [IO.File]::Exists($uninstaller)) { throw 'Partial installation has no owned uninstaller.' }
            if ((Invoke-CapturedProcess $uninstaller ('/S _?=' + $installRoot) 'cleanup-uninstall' 120).Code -ne 0) {
                throw 'Owned installation cleanup failed.'
            }
        }
        foreach ($sentinel in $sentinels) {
            if ([IO.File]::Exists($sentinel.File)) { [IO.File]::Delete($sentinel.File) }
            if (-not $sentinel.Existed -and [IO.Directory]::Exists($sentinel.Directory) -and
                [IO.Directory]::GetFileSystemEntries($sentinel.Directory).Length -eq 0) {
                [IO.Directory]::Delete($sentinel.Directory, $false)
            }
        }
        # _?= leaves the owned uninstaller itself behind; remove only that exact file.
        if ($started -and [IO.File]::Exists($uninstaller)) { [IO.File]::Delete($uninstaller) }
        if ($started -and [IO.Directory]::Exists($installRoot) -and
            [IO.Directory]::GetFileSystemEntries($installRoot).Length -eq 0) {
            [IO.Directory]::Delete($installRoot, $false)
        }
        if ($started -and [IO.Directory]::Exists($installRoot)) { throw 'Owned installation files remain after cleanup.' }
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($uninstallKey)
        if ($null -ne $key) { $key.Dispose(); throw 'The owned uninstall registration remains after cleanup.' }
        Assert-CommandPath $originalPath $false
        $summary.cleanup_complete = $true
    } catch { $failed = $true }
    $summary.success = -not $failed
    [IO.File]::WriteAllText([IO.Path]::GetFullPath($SummaryPath), ($summary | ConvertTo-Json))
}
if ($failed) { throw ('Installer command smoke failed during ' + $stage + '; raw diagnostics remain only in runner temporary storage.') }
