# Checks for the Windows end-to-end jobs (issue #166).
#
# Each check names the step of the Windows test script it covers (`S13.13`
# is session 13, step 13), says what it reads, and passes or fails without
# stopping the job: every check runs, and the job fails at the end when one
# failed. The results go to the job summary and to `results.json`, which the
# item-by-item comparison with a manual run reads.

Set-StrictMode -Version Latest

$script:Results = [System.Collections.Generic.List[object]]::new()

# Runs one check. `Test` returns $true when it passes; $false, or an
# exception, fails it. `Detail` is shown when it fails.
function Test-Check {
    param(
        [Parameter(Mandatory)][string]$Id,
        [Parameter(Mandatory)][string]$What,
        [Parameter(Mandatory)][scriptblock]$Test,
        [scriptblock]$Detail
    )
    $passed = $false
    $note = ''
    try {
        $passed = [bool](& $Test)
    } catch {
        $note = "$($_.Exception.Message)"
    }
    if (-not $passed -and $Detail) {
        try { $note = (@($note, "$(& $Detail)") | Where-Object { $_ }) -join ' / ' } catch { }
    }
    $script:Results.Add([pscustomobject]@{ id = $Id; check = $What; passed = $passed; note = $note })
    $mark = if ($passed) { 'PASS' } else { 'FAIL' }
    Write-Host "[$mark] $Id $What$(if ($note) { " ($note)" })"
}

# Waits up to `Seconds` for `Condition` to hold; whether it did.
function Wait-Until {
    param([Parameter(Mandatory)][scriptblock]$Condition, [int]$Seconds = 30)
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        try { if (& $Condition) { return $true } } catch { }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    return $false
}

# Writes the results to the job summary and `results.json` in `Folder`, and
# returns how many failed.
function Complete-Checks {
    param([Parameter(Mandatory)][string]$Job, [Parameter(Mandatory)][string]$Folder)
    New-Item -ItemType Directory -Force $Folder | Out-Null
    $script:Results | ConvertTo-Json -Depth 3 -AsArray |
        Set-Content -Encoding utf8 (Join-Path $Folder 'results.json')
    $failed = @($script:Results | Where-Object { -not $_.passed }).Count
    $lines = @("### $Job", '', "$($script:Results.Count - $failed) passed, $failed failed.", '',
        '| Step | Check | Result |', '|---|---|---|')
    foreach ($result in $script:Results) {
        $verdict = if ($result.passed) { 'pass' } else { "**fail**: $($result.note -replace '\|', '/')" }
        $lines += "| $($result.id) | $($result.check) | $verdict |"
    }
    if ($env:GITHUB_STEP_SUMMARY) {
        $lines -join "`n" | Add-Content -Encoding utf8 $env:GITHUB_STEP_SUMMARY
    }
    return $failed
}

Export-ModuleMember -Function Test-Check, Wait-Until, Complete-Checks
