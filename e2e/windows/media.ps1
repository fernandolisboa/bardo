# The media job (issue #166): the installed Bardo's editor, Render and
# Publish stages, with the ffmpeg it bundles, on the project `bardo-e2e
# seed-showcase` writes (recorded fixtures: no provider, no key). The
# harness beside the installed bardo.exe acts as the installed Bardo; the
# results are read from what it prints and from disk.
#
# Step ids (`S4.09`) are the Windows test script's, as e2e/README.md lists.

param(
    # This build's installer.
    [Parameter(Mandatory)][string]$Setup,
    # This build's bardo-e2e.exe.
    [Parameter(Mandatory)][string]$Harness,
    [Parameter(Mandatory)][string]$Out
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Checks.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bardo.psm1') -Force

$paths = Get-BardoPaths
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$work = Join-Path ($env:RUNNER_TEMP ?? [System.IO.Path]::GetTempPath()) 'bardo-e2e-media'
New-Item -ItemType Directory -Force $work | Out-Null

function Installed { Invoke-Harness (Join-Path $paths.Program 'bardo-e2e.exe') @args }

# Saves what a command printed beside the results, for a failed check.
function Keep {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)]$Value)
    $Value | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 (Join-Path $Out "$Name.json")
    return $Value
}

function Get-Render {
    param($Result, [string]$Network)
    return @($Result.renders | Where-Object { $_.network -eq $Network })[0]
}

function Invoke-Media {
    $code = Install-Bardo $Setup '' "$Out\install.log"
    if ($code -ne 0) { throw "the install failed with $code" }
    Copy-Item $Harness $paths.Program

    # S8.19: a user whose temporary folder has accents and spaces.
    $temp = New-Item -ItemType Directory -Force (Join-Path $work 'Usuário de Teste\Temp')
    $env:TEMP = $temp.FullName
    $env:TMP = $temp.FullName

    $seeded = Keep 'seed' (Installed seed-showcase)
    $folder = $seeded.project_folder

    # S4.02: the editor's proxies, with the bundled ffmpeg.
    $proxies = Keep 'proxies' (Installed proxies)
    Test-Check 'S4.02' "the editor builds every proxy with the bundled ffmpeg ($($proxies.ready) of $($proxies.total))" {
        $proxies.job.state -eq 'done' -and $proxies.total -gt 0 -and $proxies.ready -eq $proxies.total -and
        @($proxies.files | Where-Object { $_ -like 'proxy-*' }).Count -gt 0 -and @($proxies.problems).Count -eq 0
    } { $proxies | ConvertTo-Json -Compress -Depth 4 }

    # S9.03, S9.13: two files, cancelled during the second, resumed.
    $cancel = Keep 'render-cancel' (Installed render 'youtube,x' cancel-second)
    $first = Get-Render $cancel.cancelled 'youtube'
    Test-Check 'S9.03' 'cancelled during the second file, the first stays rendered and the second has no file' {
        $cancel.cancelled.job.state -eq 'cancelled' -and $first -and -not (Get-Render $cancel.cancelled 'x')
    } { $cancel.cancelled | ConvertTo-Json -Compress -Depth 5 }
    Test-Check 'S9.03' 'a cancelled render leaves no render-*.partial.mp4' {
        @($cancel.cancelled.files | Where-Object { $_ -like '*.partial*' }).Count -eq 0
    } { $cancel.cancelled.files -join ', ' }
    Test-Check 'S9.03' 'Resume renders only the second file' {
        $cancel.job.state -eq 'done' -and (Get-Render $cancel 'x') -and
        (Get-Render $cancel 'youtube').rendered_at -eq $first.rendered_at
    } { $cancel | ConvertTo-Json -Compress -Depth 5 }
    Test-Check 'S9.13' 'while a render runs, Export is refused' { $cancel.export_while_rendering -eq 'Rendering' } { $cancel.export_while_rendering }
    Test-Check 'S4.08' 'without a GPU, the render review names OpenH264 on the processor' {
        @($cancel.review | Where-Object { $_.encoder -ne 'OpenH264, on the processor' }).Count -eq 0
    } { ($cancel.review | ForEach-Object encoder) -join ', ' }

    # S9.04: Bardo ends mid-render; the next start resumes it.
    $runner = Start-Process -FilePath (Join-Path $paths.Program 'bardo-e2e.exe') -ArgumentList 'render', 'tiktok' -PassThru -WindowStyle Hidden
    $started = Wait-Until { @(Get-ChildItem $folder -Filter 'render-tiktok*.partial*' -ErrorAction SilentlyContinue).Count -gt 0 } 120
    Start-Sleep -Seconds 2
    Stop-Process -Id $runner.Id -Force
    $resumed = Keep 'render-resumed' (Installed wait-jobs)
    Test-Check 'S9.04' 'a render ended with Bardo resumes at the next start and leaves no partial file' {
        $started -and $resumed.job.state -eq 'done' -and (Get-Render $resumed 'tiktok') -and
        @($resumed.files | Where-Object { $_ -like '*.partial*' }).Count -eq 0
    } { "partial seen: $started; $($resumed | ConvertTo-Json -Compress -Depth 5)" }

    # S4.09, S4.10, S8.32: every network, both frame shapes.
    $all = Keep 'render-all' (Installed render all)
    Test-Check 'S4.09' 'every network renders in software' { $all.job.state -eq 'done' -and @($all.renders).Count -eq 4 } { $all.job | ConvertTo-Json -Compress }
    foreach ($expected in @(
            @{ network = 'youtube'; width = 1080; height = 1920 },
            @{ network = 'tiktok'; width = 1080; height = 1920 },
            @{ network = 'instagram_reels'; width = 1080; height = 1920 },
            @{ network = 'x'; width = 1280; height = 720 })) {
        $render = Get-Render $all $expected.network
        Test-Check 'S4.09' "$($expected.network): H.264 at $($expected.width) x $($expected.height)" {
            $render.probe.video.codec -eq 'h264' -and $render.probe.video.width -eq $expected.width -and
            $render.probe.video.height -eq $expected.height -and $render.probe.audio
        } { $render.probe | ConvertTo-Json -Compress }
        Test-Check 'S4.10' "$($expected.network): loudness within 1 LU of its target" {
            [math]::Abs($render.loudness - $render.target) -le 1
        } { "$($render.loudness) LUFS for $($render.target)" }
        Test-Check 'S8.32' "$($expected.network): the file is as long as the review said" {
            [math]::Abs($render.probe.seconds - $render.seconds) -le 0.1
        } { "file $($render.probe.seconds) s, review $($render.seconds) s" }
    }

    # S9.08, S9.09: the export package.
    $exported = Keep 'export' (Installed export all)
    $videos = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'Videos\Bardo'
    Test-Check 'S9.08' 'every network exports' { $exported.job.state -eq 'done' -and @($exported.exports).Count -eq 4 } { $exported.job | ConvertTo-Json -Compress }
    foreach ($export in $exported.exports) {
        Test-Check 'S9.08' "$($export.network): %USERPROFILE%\Videos\Bardo\<title> (<id8>)\<Network>\ holds <title>.mp4 and metadata.txt, no .partial" {
            $export.folder.StartsWith($videos, [StringComparison]::OrdinalIgnoreCase) -and
            (Split-Path (Split-Path $export.folder) -Leaf) -match '^The probe that never came back \([0-9a-f]{8}\)$' -and
            (@($export.files) -join '|') -eq 'metadata.txt|The probe that never came back.mp4'
        } { "$($export.folder): $($export.files -join ', ')" }
        $bytes = [System.IO.File]::ReadAllBytes((Join-Path $export.folder 'metadata.txt'))
        $text = [System.Text.UTF8Encoding]::new($false, $true).GetString($bytes)
        Test-Check 'S9.09' "$($export.network): metadata.txt is UTF-8 with CRLF, accents intact, with the disclosure line" {
            $text.Contains('Narração') -and $text.Contains("`r`n") -and -not ($text -replace "`r`n", '').Contains("`n") -and
            $export.disclosure -and $text.Contains($export.disclosure)
        } { $text.Substring(0, [math]::Min(300, $text.Length)) }
    }

    # S9.12: a title changed only in letter case keeps one video.
    $recased = Keep 'export-recased' (Installed export youtube 'THE PROBE THAT NEVER CAME BACK')
    $youtube = @($recased.exports)[0]
    $mp4 = @(Get-ChildItem $youtube.folder -Filter '*.mp4')
    Test-Check 'S9.12' 'a title changed only in letter case leaves one video, under the new name' {
        $mp4.Count -eq 1 -and $mp4[0].Name -ceq 'THE PROBE THAT NEVER CAME BACK.mp4'
    } { ($mp4 | ForEach-Object Name) -join ', ' }

    # S8.06, S8.11, S9.06: the cut and mix survive a restart, renders go
    # out of date.
    $edited = Keep 'edit-cut' (Installed edit-cut)
    $state = Keep 'cut-state' (Installed cut-state)
    Test-Check 'S8.06' 'an edit to the cut is there after a restart, and Undo is empty' {
        ($edited.after -join '|') -ne ($edited.before -join '|') -and
        ($state.clips -join '|') -eq ($edited.after -join '|') -and -not $state.can_undo
    } { "after edit: $($edited.after -join ', '); after restart: $($state.clips -join ', '), undo $($state.can_undo)" }
    Test-Check 'S8.11' 'a change in the mixer (the effects lane muted) is there after a restart' {
        -not $edited.sfx_muted_before -and $edited.sfx_muted -and $state.sfx_muted
    } { "before $($edited.sfx_muted_before), after edit $($edited.sfx_muted), after restart $($state.sfx_muted)" }
    Test-Check 'S9.06' 'after an edit to the cut, every render is out of date' {
        @($state.targets).Count -eq 4 -and @($state.targets | Where-Object { $_.current }).Count -eq 0
    } { $state.targets | ConvertTo-Json -Compress }

    # S8.36: a scene image deleted from the project folder.
    Remove-Item (Join-Path $folder 'scene-5.png')
    $missing = Keep 'cut-missing' (Installed cut-state)
    Test-Check 'S8.36' "a scene image deleted from the project folder marks its clip Media missing" {
        $state.media_missing -eq 0 -and $missing.media_missing -eq 1
    } { "before $($state.media_missing), after $($missing.media_missing)" }

    # S8.13–S8.15: imports, from a folder with accents and spaces, another
    # drive and a network share; originals untouched.
    $source = Join-Path $work 'Mídia de teste ção'
    $made = Installed media-fixtures $source
    $readOnly = Get-Item (Join-Path $source 'retrato.mov')
    $readOnly.IsReadOnly = $true
    $hashes = @{}
    Get-ChildItem $source | ForEach-Object { $hashes[$_.Name] = Get-FileHash $_.FullName }
    subst.exe B: $source | Out-Null
    New-SmbShare -Name 'BardoE2E' -Path $source -ReadAccess 'Everyone' | Out-Null
    $files = @(Get-ChildItem $source | ForEach-Object FullName) +
        @('B:\tom de teste.wav', '\\localhost\BardoE2E\paisagem ação.mp4')
    $imported = Keep 'import-media' (Installed import-media @files)
    foreach ($name in $made.takes) {
        $result = @($imported.results | Where-Object { (Split-Path $_.file -Leaf) -eq $name -and $_.file.StartsWith($source) })[0]
        Test-Check 'S8.13' "$name imports, with its kind and size" { $result.imported } { $result.message }
    }
    $kinds = @{ 'paisagem ação.mp4' = 'Video 1280x720'; 'retrato.mov' = 'Video 720x1280'; 'tom de teste.mp3' = 'Audio ' }
    foreach ($name in $kinds.Keys) {
        $result = @($imported.results | Where-Object { (Split-Path $_.file -Leaf) -eq $name -and $_.file.StartsWith($source) })[0]
        Test-Check 'S8.13' "$name reads as $($kinds[$name])" { "$($result.kind) $($result.picture)" -eq $kinds[$name] } { "$($result.kind) $($result.picture)" }
    }
    foreach ($name in $made.refuses) {
        $result = @($imported.results | Where-Object { (Split-Path $_.file -Leaf) -eq $name })[0]
        Test-Check 'S8.14' "$name is refused" { -not $result.imported } { $result | ConvertTo-Json -Compress }
    }
    $added = @($imported.after | Where-Object { $_ -notin $imported.before })
    $accepted = @($imported.results | Where-Object { $_.imported })
    Test-Check 'S8.14' 'a refused file adds nothing to the project folder' { $added.Count -eq $accepted.Count } { "added $($added -join ', ')" }
    foreach ($path in 'B:\tom de teste.wav', '\\localhost\BardoE2E\paisagem ação.mp4') {
        $result = @($imported.results | Where-Object { $_.file -eq $path })[0]
        Test-Check 'S8.15' "a file on $(if ($path -like 'B:*') { 'another drive' } else { 'a network share' }) imports" { $result.imported } { $result.message }
    }
    $changed = @(Get-ChildItem $source | Where-Object { (Get-FileHash $_.FullName).Hash -ne $hashes[$_.Name].Hash })
    Test-Check 'S8.15' 'the originals, a read-only one among them, are unchanged' {
        $changed.Count -eq 0 -and (Get-Item (Join-Path $source 'retrato.mov')).IsReadOnly
    } { $changed.Name -join ', ' }
    Test-Check 'S8.19' 'with %TEMP% under a folder with accents and spaces, proxies, renders and exports ran' {
        $env:TEMP -match 'Usuário de Teste' -and $proxies.job.state -eq 'done' -and $all.job.state -eq 'done' -and $exported.job.state -eq 'done'
    } { $env:TEMP }
}

try {
    Invoke-Media
} catch {
    Test-Check 'run' 'the media scenario ran to the end' { $false } { "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" }
} finally {
    Stop-BardoAll
    Remove-SmbShare -Name 'BardoE2E' -Force -ErrorAction SilentlyContinue
    subst.exe B: /D 2>$null | Out-Null
    Get-ChildItem $paths.Logs -ErrorAction SilentlyContinue | Copy-Item -Destination $Out -ErrorAction SilentlyContinue
}
$failed = Complete-Checks 'Media' $Out
# Explicit: Actions otherwise ends with the last native command's exit code.
if ($failed -gt 0) { exit 1 } else { exit 0 }
