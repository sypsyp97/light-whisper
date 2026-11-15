param(
    [Parameter(Mandatory = $true)][string]$Java,
    [Parameter(Mandatory = $true)][string]$Jar,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$expectedJarHash = '936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Jar).Hash.ToLowerInvariant() -ne $expectedJarHash) {
    throw 'Unexpected TLA+ tool digest'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
Push-Location $PSScriptRoot
try {
    $sany = & $Java '-Xmx512m' '-cp' $Jar 'tla2sany.SANY' 'AsrAgentSettings.tla' 2>&1
    $sanyExit = $LASTEXITCODE
    $sany | Set-Content -LiteralPath (Join-Path $OutputDirectory 'sany.log')
    if ($sanyExit -ne 0 -or ($sany -join "`n") -match '(?m)^\*\*\* Errors:') {
        throw 'SANY preflight failed; inspect sany.log'
    }
    $cases = [ordered]@{
        AsrAgentSettings = ''
        ReadEpochNegative = 'NoStalePublication'
        UiEpochNegative = 'NoStalePublication'
        FlushNegative = 'NoObsoletePersistence'
        FinallyNegative = 'LatestReadKeepsLoading'
        UiReadNegative = 'NoStalePublication'
        ReadinessNegative = 'UnknownIdentityBlocksCredentials'
        MountedNegative = 'NoStalePublication'
        ListenerOwnerNegative = 'OnlyCurrentListenerReads'
        CleanupNegative = 'RetiredListenerReleased'
        DiagnosticNegative = 'SubscriptionFailureVisible'
        ReadErrorNegative = 'CurrentReadFailureVisible'
        KeyVersionNegative = 'UserDraftSurvives'
        ProgressNegative = 'Temporal properties were violated'
        TwoReadsWitness = 'TwoReadsCompeteNotReached'
        LateListenerWitness = 'LateListenerNotReached'
        DraftOverlapWitness = 'DraftOverlapNotReached'
        RegionMutationWitness = 'RegionMutationOverlapNotReached'
    }
    $receipts = @()
    foreach ($case in $cases.GetEnumerator()) {
        $log = & $Java '-Xmx512m' '-cp' $Jar 'tlc2.TLC' '-workers' '1' '-coverage' '1' '-metadir' (Join-Path $OutputDirectory "states-$($case.Key)") '-config' "$($case.Key).cfg" 'AsrAgentSettings.tla' 2>&1
        $code = $LASTEXITCODE
        $log | Set-Content -LiteralPath (Join-Path $OutputDirectory "$($case.Key).log")
        $outputText = $log -join "`n"
        $expected = if ($case.Key -eq 'AsrAgentSettings') {
            $code -eq 0 -and $outputText.Contains('Model checking completed. No error has been found.')
        } elseif ($case.Key -eq 'ProgressNegative') {
            $code -eq 13 -and $outputText.Contains($case.Value)
        } else {
            $code -eq 12 -and $outputText.Contains("Invariant $($case.Value) is violated.")
        }
        $counts = [regex]::Match($outputText, '(\d+) states generated, (\d+) distinct states found')
        $receipts += [ordered]@{ case = $case.Key; exit = $code; expected = $expected;
            generated = $counts.Groups[1].Value; distinct = $counts.Groups[2].Value }
        Write-Output ($receipts[-1] | ConvertTo-Json -Compress)
        if (-not $expected) { throw "Unexpected TLC outcome: $($case.Key)" }
    }
    $receipts | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputDirectory 'results.json')
} finally {
    Pop-Location
}
