param(
    [Parameter(Mandatory = $true)][string]$InstallRoot,
    [switch]$Uninstall
)

function Invoke-CliCommandInstall {
    param(
        [string]$InstallRoot,
        [Microsoft.Win32.RegistryKey]$EnvironmentKey,
        [switch]$Uninstall
    )

    $ErrorActionPreference = 'Stop'
    $root = [IO.Path]::GetFullPath($InstallRoot)
    $bin = Join-Path $root 'bin'
    $alias = Join-Path $bin 'light-whisper.exe'
    if ($bin.Contains(';')) {
        throw 'The command directory cannot contain a semicolon.'
    }
    foreach ($path in @($bin, $alias)) {
        $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        if ($null -ne $item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'The command directory and executable must not be links.'
        }
    }

    $original = $EnvironmentKey.GetValue('Path', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    if ($null -ne $original) {
        $kind = $EnvironmentKey.GetValueKind('Path')
        if ($original -isnot [string] -or $kind -notin @(
            [Microsoft.Win32.RegistryValueKind]::String,
            [Microsoft.Win32.RegistryValueKind]::ExpandString
        )) {
            throw 'The user Path must be a string registry value.'
        }
    }

    $tokens = [Collections.Generic.List[string]]::new()
    $found = $false
    if (-not [string]::IsNullOrEmpty($original)) {
        foreach ($token in $original.Split(';')) {
            $normalized = $token.Trim().Trim('"').Replace('/', '\').TrimEnd('\')
            if ($normalized -ieq $bin) {
                if (-not $Uninstall -and -not $found) {
                    $tokens.Add($token)
                    $found = $true
                }
            } else {
                $tokens.Add($token)
            }
        }
    }
    if (-not $Uninstall) {
        $source = Join-Path $root 'light-whisper-cli.exe'
        if (-not [IO.File]::Exists($source)) {
            throw 'The installed CLI executable is missing.'
        }
        [IO.Directory]::CreateDirectory($bin) | Out-Null
        [IO.File]::Copy($source, $alias, $true)
        if (-not $found) { $tokens.Add($bin) }
    }
    $updated = [string]::Join(';', $tokens)
    if (($null -ne $original -or -not $Uninstall) -and $updated -cne $original) {
        $EnvironmentKey.SetValue('Path', $updated, $kind)
    }
    if ($Uninstall) {
        if ([IO.File]::Exists($alias)) { [IO.File]::Delete($alias) }
        if ([IO.Directory]::Exists($bin) -and [IO.Directory]::GetFileSystemEntries($bin).Length -eq 0) {
            [IO.Directory]::Delete($bin, $false)
        }
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    $ErrorActionPreference = 'Stop'
    try {
        $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
        try {
            Invoke-CliCommandInstall -InstallRoot $InstallRoot -EnvironmentKey $key -Uninstall:$Uninstall
        } finally {
            $key.Dispose()
        }
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class LightWhisperCommandEnvironment {
    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr SendMessageTimeout(IntPtr window, uint message,
        UIntPtr parameter, string value, uint flags, uint timeout, out UIntPtr result);
}
'@
        $result = [UIntPtr]::Zero
        $notified = [LightWhisperCommandEnvironment]::SendMessageTimeout(
            [IntPtr]0xffff, 0x001a, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result)
        if ($notified -eq [IntPtr]::Zero) {
            Write-Warning 'The user PATH was updated. Open a new terminal to use light-whisper.'
        }
    } catch {
        Write-Error -Message $_.Exception.Message -ErrorAction Continue
        exit 1
    }
}
