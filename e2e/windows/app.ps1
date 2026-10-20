# The app job (issue #166): the installed Bardo, driven through its app
# state by `bardo-e2e` (the copy beside bardo.exe acts as the installed
# Bardo; a copy beside a bardo.exe outside the installed layout acts as a
# copy built from source), and read from outside: Credential Manager, Task
# Scheduler, processes, files and zips.
#
# Step ids (`S3.06`) are the Windows test script's, as e2e/README.md lists.

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
Add-Type -AssemblyName System.IO.Compression.FileSystem

$paths = Get-BardoPaths
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$work = Join-Path ($env:RUNNER_TEMP ?? [System.IO.Path]::GetTempPath()) 'bardo-e2e'
New-Item -ItemType Directory -Force $work | Out-Null
$password = 'correct horse battery staple'

# The copy built from source: bardo.exe outside the installed layout.
$devDir = Join-Path $work 'source build'
$devRoaming = Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'Bardo Dev'
$devLocal = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Bardo Dev'

function Installed { Invoke-Harness (Join-Path $paths.Program 'bardo-e2e.exe') @args }
function Dev { Invoke-Harness (Join-Path $devDir 'bardo-e2e.exe') @args }

# The texts of every entry in `Zip`, as one string.
function Read-ZipText {
    param([Parameter(Mandatory)][string]$Zip)
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        $texts = foreach ($entry in $archive.Entries) {
            $reader = [System.IO.StreamReader]::new($entry.Open())
            try { $reader.ReadToEnd() } finally { $reader.Dispose() }
        }
        return $texts -join "`n"
    } finally { $archive.Dispose() }
}

function Get-ZipEntries {
    param([Parameter(Mandatory)][string]$Zip)
    $archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
    try { return @($archive.Entries | ForEach-Object { $_.FullName }) } finally { $archive.Dispose() }
}

function Test-NoneIn {
    param([string]$Text, [string[]]$Secrets)
    $found = @($Secrets | Where-Object { $_ -and $Text.Contains($_) })
    if ($found.Count -gt 0) { throw "found: $($found -join ', ')" }
    return $true
}

function Invoke-App {
    # The installed Bardo, opened once, with the agent on.
    $code = Install-Bardo $Setup 'backgroundagent' "$Out\install.log"
    if ($code -ne 0) { throw "the install failed with $code" }
    Copy-Item $Harness $paths.Program
    Start-BardoWindow | Out-Null
    if (-not (Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 60)) { throw 'the agent did not start' }
    Stop-BardoWindow

    # Keys (S3.06, S3.08).
    $saved = Installed save-keys
    $bardoProfile = $saved.profile
    $keys = @($saved.keys.PSObject.Properties | ForEach-Object { $_.Value })
    $credentials = Get-Credentials
    foreach ($key in $saved.keys.PSObject.Properties) {
        Test-Check 'S3.06' "Credential Manager holds Bardo/<profile>/$($key.Name)" {
            @($credentials | Where-Object { $_.Target -eq "Bardo/$bardoProfile/$($key.Name)" }).Count -eq 1
        } { ($credentials | ForEach-Object Target) -join ', ' }
    }
    $state = Installed state
    Test-Check 'S3.08' 'the privacy policy stays accepted after a restart' { $state.privacy_accepted }
    Test-Check 'S3.06' 'the keys show only their last characters' {
        foreach ($key in $saved.keys.PSObject.Properties) {
            $hint = $state.keys.($key.Name).hint
            if (-not $state.keys.($key.Name).saved -or $hint.Length -gt 5 -or -not $key.Value.EndsWith($hint.TrimStart([char]0x2026))) {
                throw "$($key.Name): $hint"
            }
        }
        $true
    }

    # A copy built from source keeps its own data (S2.08, S2.09, S2.10).
    New-Item -ItemType Directory -Force $devDir | Out-Null
    Copy-Item $paths.Exe, $Harness $devDir
    Installed agent off | Out-Null
    [void](Wait-Until { @(Get-BardoProcesses Agent).Count -eq 0 } 20)
    $installedDb = (Get-Item $paths.Database).LastWriteTimeUtc
    $first = Dev state
    Test-Check 'S2.09' "the source copy's first start offers to bring settings in" { $null -ne $first.settings_offer }
    $second = Dev state
    Test-Check 'S2.09' 'the offer does not come back at the next start' { $null -eq $second.settings_offer }
    Test-Check 'S2.08' 'the source copy keeps its data as Bardo Dev, with a profile of its own' {
        $second.data_home -eq 'Bardo Dev' -and $second.profile -ne $bardoProfile
    } { "$($second.data_home) $($second.profile)" }
    Test-Check 'S2.10' 'the source copy reads as built from source, with no update banner' {
        $second.installation -eq 'Source' -and -not $second.update_banner
    } { $second.installation }
    $devWindow = Start-Process -FilePath (Join-Path $devDir 'bardo.exe') -PassThru
    Test-Check 'S2.08' "the source copy's window writes %APPDATA%\Bardo Dev\bardo.db and %LOCALAPPDATA%\Bardo Dev\logs\bardo.log" {
        Wait-Until { (Test-Path "$devRoaming\bardo.db") -and (Test-Path "$devLocal\logs\bardo.log") } 30
    }
    Start-Sleep -Seconds 10
    Save-Screen "$Out\screens\source-copy.png" | Out-Null
    if (-not $devWindow.HasExited) { [void]$devWindow.CloseMainWindow(); [void]$devWindow.WaitForExit(15000) }
    Test-Check 'S2.08' "the installed copy's bardo.db is untouched" {
        (Get-Item $paths.Database).LastWriteTimeUtc -eq $installedDb
    }

    # Settings between copies (S3.10, S3.11, S8.34).
    $devKeys = Dev save-keys
    $devCredentials = Dev save-app-credentials
    $devSecrets = @($devKeys.keys.PSObject.Properties | ForEach-Object { $_.Value }) +
        @($devCredentials.app_credentials.PSObject.Properties | ForEach-Object { $_.Value.secret })
    $file = Join-Path $work 'Configurações\from the source copy.bardo-settings'
    New-Item -ItemType Directory -Force (Split-Path $file) | Out-Null
    $export = Dev export-settings $file $password
    Test-Check 'S3.10' 'the source copy exports its keys and app credentials to a .bardo-settings file' {
        (Test-Path $file) -and @($export.items).Count -ge @($devSecrets).Count
    } { $export.items -join ', ' }
    Test-Check 'S3.10' 'no key or secret is readable in the file' {
        $bytes = [System.IO.File]::ReadAllBytes($file)
        Test-NoneIn ([System.Text.Encoding]::UTF8.GetString($bytes) + [System.Text.Encoding]::Unicode.GetString($bytes)) $devSecrets
    }
    $wrong = Installed import-settings $file 'not the password at all'
    Test-Check 'S3.11' 'a wrong password says wrong password or damaged' {
        -not $wrong.opened -and $wrong.message -match 'password'
    } { $wrong.message }
    $import = Installed import-settings $file $password
    Test-Check 'S3.11' 'the right password lists the Claude key as replacing the one here and the app credentials as new' {
        $claude = @($import.listed | Where-Object label -eq 'Claude')
        $instagram = @($import.listed | Where-Object label -like 'Instagram*')
        $claude.Count -eq 1 -and $claude[0].effect -eq 'replaces' -and $instagram.Count -eq 1 -and $instagram[0].effect -eq 'adds'
    } { ($import.listed | ForEach-Object { "$($_.label)=$($_.effect)" }) -join ', ' }
    Test-Check 'S3.11' 'everything ticked is brought in' { $import.opened -and @($import.failed).Count -eq 0 } { $import.failed -join ', ' }
    $after = Installed state
    $credentials = Get-Credentials
    Test-Check 'S3.11' "the keys are the source copy's, under the installed profile in Credential Manager" {
        $after.keys.claude.saved -and $devKeys.keys.claude.EndsWith($after.keys.claude.hint.TrimStart([char]0x2026)) -and
        @($credentials | Where-Object Target -eq "Bardo/$bardoProfile/claude").Count -eq 1 -and
        @($credentials | Where-Object Target -eq "Bardo/$($devKeys.profile)/claude").Count -eq 1
    }

    # App credentials (S5.09, S5.15, S5.22, S5.29).
    $apps = Installed save-app-credentials
    $credentials = Get-Credentials
    $after = Installed state
    foreach ($network in $apps.app_credentials.PSObject.Properties) {
        $id = @{ youtube = 'S5.09'; instagram_reels = 'S5.15'; tiktok = 'S5.22'; x = 'S5.29' }[$network.Name]
        Test-Check $id "the $($network.Name) app credentials are a generic Bardo/<profile>/app/$($network.Name) credential" {
            @($credentials | Where-Object { $_.Target -eq "Bardo/$bardoProfile/app/$($network.Name)" -and $_.Type -eq 'Generic' }).Count -eq 1
        } { ($credentials | ForEach-Object { "$($_.Target) ($($_.Type))" }) -join ', ' }
        Test-Check $id "the $($network.Name) secret shows only its end" {
            $hint = $after.app_credentials.($network.Name).hint
            $after.app_credentials.($network.Name).saved -and -not $hint.Contains($network.Value.secret)
        }
    }

    # Diagnostics (S13.01-S13.04), with the agent running.
    Installed seed-channel | Out-Null
    Installed agent on | Out-Null
    $agentUp = Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 30
    $zip = Join-Path $work 'bardo-diagnostics.zip'
    $diagnostics = Installed diagnostics $zip
    Test-Check 'S13.01' 'the logs folder is %LOCALAPPDATA%\Bardo\logs' { $diagnostics.logs -eq $paths.Logs }
    $entries = Get-ZipEntries $zip
    Test-Check 'S13.03' 'the zip holds system.txt, jobs.txt, accounts.txt, bardo.log and agent.log' {
        $agentUp -and -not (@('system.txt', 'jobs.txt', 'accounts.txt', 'bardo.log', 'agent.log') | Where-Object { $entries -notcontains $_ })
    } { $entries -join ', ' }
    Test-Check 'S13.03' 'the agent keeps running after the export' { @(Get-BardoProcesses Agent).Count -eq 1 }
    $text = Read-ZipText $zip
    Test-Check 'S13.03' 'system.txt names Windows with its build number' { $text -match 'Windows[^\n]*\(10\.0\.\d+\.\d+\)' }
    Test-Check '#164' 'system.txt has the decision engine line' { $text -match 'Decision engine: jev' }
    $secretValues = $keys + @($apps.app_credentials.PSObject.Properties | ForEach-Object { $_.Value.secret })
    $secrets = $secretValues + @('Canal Único de Teste', 'bardo_e2e_handle', $env:USERNAME)
    Test-Check 'S13.04' 'no key, secret, channel name, @handle or Windows user name in the zip' { Test-NoneIn $text $secrets }
    Test-Check 'S13.04' 'a path in the home folder shows only as %USERPROFILE%' {
        -not $text.Contains($env:USERPROFILE)
    } { "%USERPROFILE% appears: $($text.Contains('%USERPROFILE%'))" }
    $before = (Get-Item $zip).LastWriteTimeUtc
    Start-Sleep -Seconds 2
    $again = Installed diagnostics $zip
    Test-Check 'S13.02' 'saving over an existing zip replaces it' { $again.written -and (Get-Item $zip).LastWriteTimeUtc -gt $before }
    $locked = Join-Path $work 'not writable'
    New-Item -ItemType Directory -Force $locked | Out-Null
    icacls.exe $locked /deny "$($env:USERNAME):(WD,AD)" | Out-Null
    $refused = Installed diagnostics (Join-Path $locked 'bardo-diagnostics.zip')
    Test-Check 'S13.02' "saving where Bardo can't write fails and leaves no .zip.part" {
        -not $refused.written -and @(Get-ChildItem $locked -Force).Count -eq 0
    } { "written=$($refused.written) $(Get-ChildItem $locked -Force -Name)" }

    # The background agent from Settings (S10.36, S10.37, S10.38, S10.43).
    Installed agent off | Out-Null
    Test-Check 'S10.37' 'off: the agent exits within 5 seconds and the task goes' {
        (Wait-Until { @(Get-BardoProcesses Agent).Count -eq 0 } 8) -and $null -eq (Get-AgentTask)
    }
    $on = Installed agent on
    Test-Check 'S10.37' 'on: the agent runs within seconds, with no window' {
        $null -eq $on.error -and (Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 15) -and
        (Get-Process -Id (Get-BardoProcesses Agent)[0].ProcessId).MainWindowHandle -eq 0
    } { $on.error }
    $task = Get-AgentTask
    Test-Check 'S10.36' "the task sits in Task Scheduler's root folder as '$($paths.TaskName)', running bardo.exe --agent" {
        $task.Task.RegistrationInfo.URI -eq "\$($paths.TaskName)" -and $task.Task.Actions.Exec.Command -eq $paths.Exe
    } { $task.Task.RegistrationInfo.URI }
    Test-Check 'S10.36' 'the task starts at sign-in, every 15 minutes, as the user, only while signed in, without highest privileges' {
        $principal = $task.Task.Principals.Principal
        $principal.LogonType -eq 'InteractiveToken' -and
        $null -eq $principal.SelectSingleNode("*[local-name()='RunLevel' and text()='HighestAvailable']") -and
        $task.Task.Triggers.LogonTrigger.Repetition.Interval -eq 'PT15M'
    }
    Test-Check 'S10.37' 'Settings reads Running' { (Installed state).agent -eq 'running' }
    Get-BardoProcesses Agent | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
    Test-Check 'S10.38' 'an agent ended by hand reads On, but not running' {
        Wait-Until { (Installed state).agent -eq 'not-running' } 60
    }
    Installed agent start | Out-Null
    Test-Check 'S10.38' 'Start now runs it again' { Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 15 }
    Dev agent on | Out-Null
    $devTask = "Bardo Dev publishing agent ($env:USERNAME)"
    Test-Check 'S10.43' "the source copy's agent has its own task, running its own bardo.exe" {
        $xml = [xml]((schtasks.exe /Query /TN $devTask /XML) -join "`n")
        $xml.Task.Actions.Exec.Command -eq (Join-Path $devDir 'bardo.exe') -and $null -ne (Get-AgentTask)
    }
    Test-Check 'S10.43' 'both agents run, each from its own bardo.exe' {
        Wait-Until {
            $agents = @(Get-BardoProcesses Agent)
            $agents.Count -eq 2 -and @($agents | Where-Object { $_.ExecutablePath -eq (Join-Path $devDir 'bardo.exe') }).Count -eq 1
        } 20
    }
    Dev agent off | Out-Null

    # Backup and restore (S13.06, S13.09, S13.10, S13.11, S13.12).
    $project = Join-Path $paths.Projects 'e2e-project'
    New-Item -ItemType Directory -Force $project | Out-Null
    Set-Content (Join-Path $project 'clip.mp4') 'backed up'
    Set-Content (Join-Path $project 'proxy-clip.mp4.mkv') 'rebuilt'
    Set-Content (Join-Path $project 'peaks-clip.mp4.json') '[]'
    $backupZip = Join-Path $work 'bardo-backup.zip'
    $backup = Installed backup $backupZip media
    $entries = Get-ZipEntries $backupZip
    Test-Check 'S13.06' 'a backup with media, the agent running, holds bardo-backup.toml, bardo.db and the projects' {
        $backup.job.state -eq 'done' -and $entries -contains 'bardo-backup.toml' -and $entries -contains 'bardo.db' -and
        $entries -contains 'projects/e2e-project/clip.mp4'
    } { "$($backup.job.state) $($backup.job.failure): $($entries -join ', ')" }
    Test-Check 'S13.06' 'no proxy-* or peaks-* in it' { -not ($entries | Where-Object { $_ -match '/(proxy|peaks)-' }) }
    Set-Content (Join-Path $project 'clip.mp4') 'changed after the backup'

    Start-BardoWindow | Out-Null
    [void](Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30)
    Start-Sleep -Seconds 5
    Test-Check 'S13.09' 'beside an open window, another start is not the window that holds the data' {
        -not (Invoke-Harness (Join-Path $paths.Program 'bardo-e2e.exe') window-lock).sole
    }
    Stop-BardoWindow
    Test-Check 'S13.09' 'with the window closed, a start holds the data again' {
        (Invoke-Harness (Join-Path $paths.Program 'bardo-e2e.exe') window-lock).sole
    }

    $restore = Installed restore $backupZip
    Test-Check 'S13.11' 'the restore waits for the restart that puts it in place' {
        $restore.started -and $restore.job.state -eq 'done' -and $restore.restart_needed
    } { $restore | ConvertTo-Json -Compress }
    Test-Check 'S13.11' 'the agent stops while a restore waits' { Wait-Until { @(Get-BardoProcesses Agent).Count -eq 0 } 60 }

    # A program holding a project folder open makes the swap fail.
    $holder = Start-Process pwsh -ArgumentList '-NoProfile', '-Command', 'Start-Sleep -Seconds 600' -WorkingDirectory $project -PassThru -WindowStyle Hidden
    Start-Sleep -Seconds 2
    Start-BardoWindow | Out-Null
    [void](Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30)
    Start-Sleep -Seconds 20
    Stop-BardoWindow
    $holder | Stop-Process -Force
    $rolledBack = Installed state
    Test-Check 'S13.10' 'with a project folder in use, the restore is rolled back and says so' {
        $null -ne $rolledBack.last_restore -and -not $rolledBack.last_restore.succeeded
    } { $rolledBack.last_restore | ConvertTo-Json -Compress }
    Test-Check 'S13.10' 'the data is as it was' {
        (Get-Content (Join-Path $project 'clip.mp4')) -eq 'changed after the backup'
    }

    # As on another computer: none of the keys or app credentials here.
    Installed agent off | Out-Null
    Get-Credentials | Where-Object { $_.Target -like "Bardo/$bardoProfile/*" } |
        ForEach-Object { cmdkey.exe "/delete:$($_.Target)" | Out-Null }
    $restore = Installed restore $backupZip
    Start-BardoWindow | Out-Null
    [void](Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30)
    Start-Sleep -Seconds 20
    Stop-BardoWindow
    $restored = Installed state
    Test-Check 'S13.11' 'the next start puts the restore in place' {
        $restored.last_restore.succeeded -and (Get-Content (Join-Path $project 'clip.mp4')) -eq 'backed up'
    } { $restored.last_restore | ConvertTo-Json -Compress }
    Test-Check 'S13.12' 'without the keys and app credentials here, the restore lists each to set up again' {
        @($restored.set_up_again | Where-Object { $_ -like 'Key(*' }).Count -ge @($keys).Count -and
        @($restored.set_up_again | Where-Object { $_ -like 'AppCredentials(*' }).Count -ge 1
    } { $restored.set_up_again -join ', ' }

    # Nothing secret left in the database or the logs (S3.06, S5.34).
    Stop-BardoAll
    $logs = (Get-ChildItem $paths.Logs, "$devLocal\logs" -Filter *.log | ForEach-Object { Get-Content -Raw $_.FullName }) -join "`n"
    Test-Check 'S3.06' 'bardo.log and agent.log hold no key or secret' { Test-NoneIn $logs ($secretValues + $devSecrets) }
    $db = [System.Text.Encoding]::UTF8.GetString([System.IO.File]::ReadAllBytes($paths.Database))
    Test-Check 'S5.34' 'bardo.db holds no key or secret' { Test-NoneIn $db ($secretValues + $devSecrets) }
}

try {
    Invoke-App
} catch {
    Test-Check 'run' 'the app scenario ran to the end' { $false } { "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" }
} finally {
    Save-Screen "$Out\screens\last.png" | Out-Null
    Stop-BardoAll
    Get-ChildItem $paths.Logs, "$devLocal\logs" -ErrorAction SilentlyContinue | Copy-Item -Destination $Out -ErrorAction SilentlyContinue
}
$failed = Complete-Checks 'App' $Out
if ($failed -gt 0) { exit 1 }
