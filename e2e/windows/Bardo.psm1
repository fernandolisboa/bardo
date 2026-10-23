# Where the installed Bardo lives on Windows and how to read its state from
# outside: files, the uninstall entry, processes, the agent's task, the
# screen (issue #166).

Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot 'Watchdog.psm1')

# Never changes: installer/bardo.iss names it AppGuid.
$script:AppGuid = 'C6E790A6-186B-4ED1-8997-82360B3BE325'

function Get-BardoPaths {
    $local = [Environment]::GetFolderPath('LocalApplicationData')
    $roaming = [Environment]::GetFolderPath('ApplicationData')
    [pscustomobject]@{
        Program      = Join-Path $local 'Programs\Bardo'
        Exe          = Join-Path $local 'Programs\Bardo\bardo.exe'
        Roaming      = Join-Path $roaming 'Bardo'
        Database     = Join-Path $roaming 'Bardo\bardo.db'
        SetupChoices = Join-Path $roaming 'Bardo\setup-choices.toml'
        Local        = Join-Path $local 'Bardo'
        Logs         = Join-Path $local 'Bardo\logs'
        Backups      = Join-Path $local 'Bardo\backups'
        Projects     = Join-Path $local 'Bardo\projects'
        StartMenu    = Join-Path $roaming 'Microsoft\Windows\Start Menu\Programs\Bardo.lnk'
        Desktop      = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Bardo.lnk'
        UninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{$($script:AppGuid)}_is1"
        TaskName     = "Bardo publishing agent ($env:USERNAME)"
    }
}

# Runs an installer or uninstaller and waits; its exit code. One that has
# not finished within `Minutes` (a silent install takes seconds) is ended
# with its children (Inno Setup's loader runs the setup from a temporary
# copy), after `Report` gets what ran and which windows were open.
function Invoke-Setup {
    param([Parameter(Mandatory)][string]$Path, [string[]]$Arguments = @(), [int]$Minutes = 5, [string]$Report)
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru
    if (-not $process.WaitForExit($Minutes * 60 * 1000)) {
        $name = Split-Path -Leaf $Path
        if ($Report) { Save-ProcessReport $Report "$name did not finish within $Minutes minutes." }
        $process.Kill($true)
        throw "$name did not finish within $Minutes minutes$(if ($Report) { " (see $(Split-Path -Leaf $Report))" })"
    }
    return $process.ExitCode
}

# `Log`'s name with .hang.txt for .log: where a stuck setup's report goes.
function Get-HangReportPath {
    param([Parameter(Mandatory)][string]$Log)
    return [System.IO.Path]::ChangeExtension($Log, '.hang.txt')
}

function Install-Bardo {
    param([Parameter(Mandatory)][string]$Setup, [string]$Tasks = '', [Parameter(Mandatory)][string]$Log)
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$Log`"", "/TASKS=`"$Tasks`"")
    return Invoke-Setup $Setup $arguments -Report (Get-HangReportPath $Log)
}

function Uninstall-Bardo {
    param([Parameter(Mandatory)][string]$Log)
    $uninstaller = Join-Path (Get-BardoPaths).Program 'unins000.exe'
    $code = Invoke-Setup $uninstaller @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$Log`"") -Report (Get-HangReportPath $Log)
    # The uninstaller hands the removal of its own files to a copy of itself.
    Start-Sleep -Seconds 3
    return $code
}

# The bardo.exe processes: `Agent` for the background agent, `Window` for
# Bardo's window.
function Get-BardoProcesses {
    param([ValidateSet('Agent', 'Window', 'Any')][string]$Kind = 'Any')
    $all = @(Get-CimInstance Win32_Process -Filter "Name = 'bardo.exe'")
    switch ($Kind) {
        'Agent' { return @($all | Where-Object { $_.CommandLine -match '--agent' }) }
        'Window' { return @($all | Where-Object { $_.CommandLine -notmatch '--(agent|uninstall)' }) }
        default { return $all }
    }
}

function Start-BardoWindow {
    param([string]$Exe = (Get-BardoPaths).Exe)
    return Start-Process -FilePath $Exe -PassThru
}

# Closes Bardo's window the way a person does, then makes sure it is gone.
function Stop-BardoWindow {
    foreach ($process in Get-BardoProcesses Window) {
        $running = Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
        if ($running) {
            [void]$running.CloseMainWindow()
            if (-not $running.WaitForExit(15000)) { $running.Kill() }
        }
    }
}

function Stop-BardoAll {
    Get-Process bardo -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}

# The agent's task as Task Scheduler holds it, or $null.
function Get-AgentTask {
    $xml = schtasks.exe /Query /TN (Get-BardoPaths).TaskName /XML 2>$null
    if ($LASTEXITCODE -ne 0) { return $null }
    return [xml]($xml -join "`n")
}

function Get-FileSha256 {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 $Path).Hash
}

# A screenshot of the whole screen, saved as PNG; returns how many distinct
# colours a sample of its pixels has (1 for a black or blank screen).
function Save-Screen {
    param([Parameter(Mandatory)][string]$Path)
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = [System.Drawing.Bitmap]::new($bounds.Width, $bounds.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
        New-Item -ItemType Directory -Force (Split-Path $Path) | Out-Null
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $colours = [System.Collections.Generic.HashSet[int]]::new()
        for ($x = 0; $x -lt $bitmap.Width; $x += 16) {
            for ($y = 0; $y -lt $bitmap.Height; $y += 16) {
                [void]$colours.Add($bitmap.GetPixel($x, $y).ToArgb())
            }
        }
        return $colours.Count
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

# Runs `bardo-e2e` (the copy at `Harness`) with `Arguments`; what it
# printed, read as JSON.
function Invoke-Harness {
    param([Parameter(Mandatory)][string]$Harness, [Parameter(ValueFromRemainingArguments)][string[]]$Arguments)
    $output = & $Harness @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) { throw "bardo-e2e $($Arguments -join ' ') failed: $($output -join "`n")" }
    return ($output | Where-Object { $_ -is [string] }) -join "`n" | ConvertFrom-Json
}

# Credential Manager's entries for this user, as `cmdkey /list` shows
# them: each one's target name and type.
function Get-Credentials {
    $entries = @()
    $target = $null
    foreach ($line in (cmdkey.exe /list)) {
        if ($line -match '^\s*Target:\s*(?:[^:=]+:target=)?(.+?)\s*$') {
            $target = $Matches[1]
        } elseif ($line -match '^\s*Type:\s*(.+?)\s*$' -and $target) {
            $entries += [pscustomobject]@{ Target = $target; Type = $Matches[1] }
            $target = $null
        }
    }
    return $entries
}

Export-ModuleMember -Function Invoke-Harness, Get-Credentials, Get-BardoPaths, Invoke-Setup, Install-Bardo, Uninstall-Bardo,
    Get-BardoProcesses, Start-BardoWindow, Stop-BardoWindow, Stop-BardoAll, Get-AgentTask,
    Get-FileSha256, Save-Screen
