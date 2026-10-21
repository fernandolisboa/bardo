# The look job (issue #166): every screen in both layouts, the ten themes
# and both languages, photographed and checked, and the installed window
# read from outside.
#
# A build with the `showcase` feature (`bardo --showcase <folder>`) walks
# through the shots; it runs as a copy built from source (Bardo Dev) over
# data `bardo-e2e seed-showcase` writes, with the installed ffmpeg. For
# each shot it writes `<shot>.json` once the shot has settled; this script
# photographs the window's inner area to `<shot>.png`, and Bardo moves on.
# `bardo-e2e check-shots` then reads every screenshot.
#
# Step ids (`S12.05`) are the Windows test script's, as e2e/README.md lists.

param(
    # This build's installer.
    [Parameter(Mandatory)][string]$Setup,
    # This build's bardo-e2e.exe.
    [Parameter(Mandatory)][string]$Harness,
    # The bardo.exe built with the `showcase` feature.
    [Parameter(Mandatory)][string]$Showcase,
    [Parameter(Mandatory)][string]$Out
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Checks.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bardo.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Window.psm1') -Force

$paths = Get-BardoPaths
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$shots = Join-Path $Out 'shots'
New-Item -ItemType Directory -Force $shots | Out-Null
$work = Join-Path ($env:RUNNER_TEMP ?? [System.IO.Path]::GetTempPath()) 'bardo-e2e-look'
$devDir = Join-Path $work 'showcase build'
New-Item -ItemType Directory -Force $devDir | Out-Null

function Dev { Invoke-Harness (Join-Path $devDir 'bardo-e2e.exe') @args }

# A fault in one shot fails its checks, not the walk.
function Invoke-Walk {
    $process = Start-Process -FilePath (Join-Path $devDir 'bardo.exe') -ArgumentList '--showcase', "`"$shots`"" -PassThru
    $handle = Get-BardoWindowHandle $process.Id
    $titles = [System.Collections.Generic.List[object]]::new()
    $deadline = (Get-Date).AddMinutes(25)
    while (-not $process.HasExited -and (Get-Date) -lt $deadline) {
        $ready = @(Get-ChildItem $shots -Filter '*.json' |
            Where-Object { $_.BaseName -notin 'finished', 'stopped' -and -not (Test-Path ([IO.Path]::ChangeExtension($_.FullName, 'png'))) })
        foreach ($file in $ready) {
            $shot = Get-Content -Raw $file.FullName | ConvertFrom-Json
            [BardoWindowWin32]::Front($handle)
            Start-Sleep -Milliseconds 150
            $titles.Add([pscustomobject]@{ shot = $shot.name; language = $shot.language; expected = $shot.title; seen = [BardoWindowWin32]::Title($handle) })
            $partial = Join-Path $shots "$($shot.name).partial.png"
            Save-BardoWindow $handle $partial
            Move-Item $partial (Join-Path $shots "$($shot.name).png")
        }
        Start-Sleep -Milliseconds 100
    }
    if (-not $process.HasExited) {
        $process.Kill()
        throw 'the walk did not end within 25 minutes'
    }
    return $titles
}

function Invoke-Look {
    $code = Install-Bardo $Setup '' "$Out\install.log"
    if ($code -ne 0) { throw "the install failed with $code" }
    Copy-Item $Showcase (Join-Path $devDir 'bardo.exe')
    Copy-Item $Harness $devDir
    # The source copy plays and renders with the ffmpeg Bardo installed.
    $env:BARDO_FFMPEG_DIR = Join-Path $paths.Program 'ffmpeg'

    # The data the screens show, and one render so the Publish stage opens.
    $seeded = Dev seed-showcase
    Test-Check 'look' 'the showcase data is seeded' { $seeded.project } { $seeded | ConvertTo-Json -Compress }
    $rendered = Dev render youtube
    Test-Check 'look' 'the seeded project renders for YouTube' { $rendered.job.state -eq 'done' } { $rendered.job | ConvertTo-Json -Compress }

    $titles = Invoke-Walk
    $finished = Join-Path $shots 'finished.json'
    $stopped = Join-Path $shots 'stopped.json'
    $taken = @(Get-ChildItem $shots -Filter '*.png').Count
    Test-Check 'look' "the walk showed every shot ($taken)" {
        (Test-Path $finished) -and ((Get-Content -Raw $finished | ConvertFrom-Json).shots -eq $taken)
    } { if (Test-Path $stopped) { Get-Content -Raw $stopped } else { "$taken screenshots" } }

    $seen = Invoke-Harness $Harness check-shots $shots
    $seen | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $Out 'check-shots.json')
    $list = { param($items) ($items | ForEach-Object { $_ | ConvertTo-Json -Compress }) -join '; ' }
    Test-Check 'S2.05' "every screen draws without a GPU ($($seen.shots) screenshots, none blank)" {
        @($seen.blank).Count -eq 0
    } { & $list $seen.blank }
    Test-Check 'look' 'every screenshot is the size the shot asked for (1000 x 700, 640 x 600)' {
        @($seen.wrong_size).Count -eq 0
    } { & $list $seen.wrong_size }
    Test-Check 'look' 'every screen is drawn in its theme: the most common colour is one of its grounds' {
        @($seen.off_theme).Count -eq 0
    } { & $list $seen.off_theme }
    Test-Check 'look' "each place looks unlike the others in its layout, theme, language and size ($($seen.groups) groups)" {
        @($seen.alike).Count -eq 0
    } { & $list $seen.alike }
    Test-Check 'look' "Portuguese and English differ on every place ($($seen.places_in_both_languages) places)" {
        @($seen.same_in_both_languages).Count -eq 0
    } { & $list $seen.same_in_both_languages }

    # S12.05: the title follows the interface language.
    $wrong = @($titles | Where-Object { $_.seen -ne $_.expected })
    Test-Check 'S12.05' "the window's title is the interface language's at every shot ($($titles.Count))" {
        $titles.Count -gt 0 -and $wrong.Count -eq 0
    } { & $list ($wrong | Select-Object -First 5) }

    # The installed Bardo's own window.
    $window = Start-BardoWindow
    $handle = Get-BardoWindowHandle $window.Id
    Start-Sleep -Seconds 10
    $cpu = Measure-IdleCpu $window.Id 15
    Test-Check 'S2.07' ('idle on a screen with nothing moving, Bardo uses {0:P0} of a core under software rendering' -f $cpu) {
        $cpu -lt 0.2
    } { '{0:P1}' -f $cpu }
    Test-Check 'S12.07' 'bardo.exe and its window carry an icon (taskbar, Alt+Tab)' {
        [BardoWindowWin32]::HasIcon($handle) -and $null -ne [System.Drawing.Icon]::ExtractAssociatedIcon($paths.Exe)
    }
    [BardoWindowWin32]::Front($handle)
    [BardoWindowWin32]::Resize($handle, 300, 300)
    Start-Sleep -Seconds 1
    $inner = [BardoWindowWin32]::Client($handle)
    Test-Check 'S12.31' 'dragged smaller, the window stops at 640 x 600 (100%)' {
        $inner[2] -ge 640 -and $inner[3] -ge 600 -and $inner[2] -le 660 -and $inner[3] -le 620
    } { "$($inner[2]) x $($inner[3])" }
    Save-BardoWindow $handle (Join-Path $Out 'smallest-window.png')
    Stop-BardoWindow
}

try {
    Invoke-Look
} catch {
    Test-Check 'run' 'the look scenario ran to the end' { $false } { "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" }
} finally {
    Save-Screen "$Out\screens\last.png" | Out-Null
    Stop-BardoAll
    $devLogs = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Bardo Dev\logs'
    Get-ChildItem $paths.Logs, $devLogs -ErrorAction SilentlyContinue | Copy-Item -Destination $Out -ErrorAction SilentlyContinue
}
$failed = Complete-Checks 'Look' $Out
# Explicit: Actions otherwise ends with the last native command's exit code.
if ($failed -gt 0) { exit 1 } else { exit 0 }
