# The installer job (issue #166): installs the Bardo built from this commit
# on a clean Windows runner and reads what the installer and Bardo's first
# start leave behind, through to the uninstall.
#
#   fresh:   first install through the wizard, the agent's task, uninstall
#            with the window open and closed, reinstall, an uninstall
#            before Bardo ever opened.
#   upgrade: the latest published release, then this build over it while
#            Bardo and its agent run, then this build over itself through
#            the wizard.
#
# Step ids (`S13.13`) are the Windows test script's, as e2e/README.md lists.

param(
    [Parameter(Mandatory)][ValidateSet('fresh', 'upgrade')][string]$Scenario,
    # This build's installer.
    [Parameter(Mandatory)][string]$Setup,
    # The bardo.exe this build staged for the installer.
    [Parameter(Mandatory)][string]$Program,
    # The latest published release's installer, for the upgrade.
    [string]$Previous,
    [Parameter(Mandatory)][string]$Out,
    # How long the scenario may run before the watchdog ends it (a passing
    # run takes about two minutes).
    [int]$WatchdogMinutes = 10
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'Checks.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Watchdog.psm1') -Force
Import-Module (Join-Path $PSScriptRoot 'Bardo.psm1') -Force

$paths = Get-BardoPaths
# Absolute: the uninstaller runs from a copy in another folder.
$Out = (New-Item -ItemType Directory -Force $Out).FullName
$Setup = (Resolve-Path $Setup).Path
$built = Get-FileSha256 $Program

# Reads the agent's task: registered for this user, running this copy's
# bardo.exe with --agent at sign-in every 15 minutes, as the user, only
# while signed in, without administrator rights.
function Test-AgentTask {
    param([string]$Id)
    $task = Get-AgentTask
    Test-Check $Id "the agent's task '$($paths.TaskName)' exists" { $null -ne $task }
    if (-not $task) { return }
    Test-Check $Id "the task runs $($paths.Exe) --agent" {
        $task.Task.Actions.Exec.Command -eq $paths.Exe -and $task.Task.Actions.Exec.Arguments -eq '--agent'
    } { "$($task.Task.Actions.Exec.Command) $($task.Task.Actions.Exec.Arguments)" }
    Test-Check $Id 'the task starts at sign-in and repeats every 15 minutes' {
        $null -ne $task.Task.Triggers.LogonTrigger -and
        $task.Task.Triggers.LogonTrigger.Repetition.Interval -eq 'PT15M'
    }
    # Task Scheduler gives the user back as a SID, and leaves out settings
    # at their defaults: no RunLevel is least privilege, no WakeToRun false.
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    Test-Check $Id 'the task runs as the user, only while signed in, without highest privileges' {
        $principal = $task.Task.Principals.Principal
        $runLevel = $principal.SelectSingleNode("*[local-name()='RunLevel']")
        $principal.UserId -eq $sid -and $principal.LogonType -eq 'InteractiveToken' -and
        ($null -eq $runLevel -or $runLevel.InnerText -eq 'LeastPrivilege')
    } { $task.Task.Principals.Principal.OuterXml }
    Test-Check $Id 'the task is not stopped on battery or after a time limit, and never wakes the PC' {
        $settings = $task.Task.Settings
        $wake = $settings.SelectSingleNode("*[local-name()='WakeToRun']")
        $settings.StopIfGoingOnBatteries -eq 'false' -and $settings.ExecutionTimeLimit -eq 'PT0S' -and
        ($null -eq $wake -or $wake.InnerText -eq 'false')
    } { $task.Task.Settings.OuterXml }
}

# The program folder as the installer lays it out.
function Test-ProgramFolder {
    param([string]$Id)
    $ffmpeg = Join-Path $paths.Program 'ffmpeg'
    Test-Check $Id 'bardo.exe and THIRD-PARTY-NOTICES.html are in %LOCALAPPDATA%\Programs\Bardo' {
        (Test-Path $paths.Exe) -and (Test-Path (Join-Path $paths.Program 'THIRD-PARTY-NOTICES.html'))
    }
    Test-Check $Id 'ffmpeg\ holds ffmpeg.exe, ffprobe.exe, its DLLs, LICENSE.txt and SOURCE.txt, and no ffplay' {
        (Test-Path "$ffmpeg\ffmpeg.exe") -and (Test-Path "$ffmpeg\ffprobe.exe") -and
        (Test-Path "$ffmpeg\LICENSE.txt") -and (Test-Path "$ffmpeg\SOURCE.txt") -and
        @(Get-ChildItem "$ffmpeg\*.dll").Count -gt 0 -and -not (Test-Path "$ffmpeg\ffplay.exe")
    } { (Get-ChildItem $ffmpeg -Name) -join ', ' }
    $models = Join-Path $paths.Program 'models'
    Test-Check $Id 'DirectML.dll is beside bardo.exe, and models\ holds the vision models and NOTICES.txt' {
        (Test-Path (Join-Path $paths.Program 'DirectML.dll')) -and (Test-Path "$models\NOTICES.txt") -and
        @(Get-ChildItem "$models\*.onnx").Count -ge 8
    } { (Get-ChildItem $models -Name) -join ', ' }
    Test-Check $Id 'the installed bardo.exe is the one this commit built' {
        (Get-FileSha256 $paths.Exe) -eq $built
    }
    Test-Check $Id 'the Start menu has Bardo, pointing at bardo.exe' {
        $shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($paths.StartMenu)
        (Test-Path $paths.StartMenu) -and $shortcut.TargetPath -eq $paths.Exe
    }
    Test-Check $Id 'no desktop icon unless it was chosen' { -not (Test-Path $paths.Desktop) }
    Test-Check $Id 'Installed apps lists Bardo with its version and icon' {
        $entry = Get-ItemProperty $paths.UninstallKey
        $entry.DisplayName -eq 'Bardo' -and $entry.DisplayVersion -and $entry.DisplayIcon -like "*bardo.exe*"
    }
}

# Bardo's first start after an install: it reads the installer's answer,
# deletes the file, and sets the agent up (or not).
function Test-FirstStart {
    param([string]$Id, [bool]$AgentOn)
    Test-Check $Id "Bardo's window starts and stays up" {
        $started = Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30
        Start-Sleep -Seconds 15
        $started -and @(Get-BardoProcesses Window).Count -ge 1
    }
    Test-Check $Id 'setup-choices.toml is read and deleted' {
        Wait-Until { -not (Test-Path $paths.SetupChoices) } 30
    }
    Test-Check $Id 'the database is at %APPDATA%\Bardo\bardo.db and the log at %LOCALAPPDATA%\Bardo\logs\bardo.log' {
        (Test-Path $paths.Database) -and (Test-Path (Join-Path $paths.Logs 'bardo.log'))
    }
    if ($AgentOn) {
        Test-Check $Id 'the background agent runs, with no window' {
            (Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 30) -and
            (Get-Process -Id (Get-BardoProcesses Agent)[0].ProcessId).MainWindowHandle -eq 0
        } { "$(@(Get-BardoProcesses Agent).Count) agents" }
        Test-AgentTask $Id
    } else {
        Start-Sleep -Seconds 5
        Test-Check $Id 'no agent task and no agent running' {
            $null -eq (Get-AgentTask) -and @(Get-BardoProcesses Agent).Count -eq 0
        }
    }
}

function Invoke-Fresh {
    $settingsDir = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Configurações de teste'
    New-Item -ItemType Directory -Force $settingsDir | Out-Null
    $settingsFile = Join-Path $settingsDir 'cópia antiga.bardo-settings'
    Set-Content -Path $settingsFile -Value 'not a real settings file'

    # First install, through the wizard, with the agent ticked on the
    # command line (the wizard keeps it ticked).
    Enter-Stage 'first install through the wizard'
    $wizard = Start-Process -FilePath $Setup -ArgumentList @('/TASKS="backgroundagent"', "/LOG=`"$Out\install-wizard.log`"") -PassThru
    Import-Module (Join-Path $PSScriptRoot 'Wizard.psm1') -Force
    $window = Find-WizardWindow
    $pages = Invoke-WizardWalk $window {
        param($texts)
        if ($texts -contains 'Import settings') { Set-WizardEdit $window $settingsFile }
    }
    $all = $pages | ForEach-Object { $_ }
    Test-Check 'S1.01' 'a first install asks "Publish even when Bardo is closed?"' {
        @($all | Where-Object { $_ -like 'Publish even when Bardo is closed*' -or $_ -like 'Publish scheduled posts while Bardo is closed*' }).Count -gt 0
    } { ($pages | ForEach-Object { $_ -join ' | ' }) -join ' || ' }
    Test-Check 'S13.14' 'a first install shows the Import settings page' { $all -contains 'Import settings' }
    Test-Check 'S1.01' 'the finished page offers to launch Bardo' {
        @($all | Where-Object { $_ -like 'Launch Bardo*' }).Count -gt 0
    }
    Test-Check 'S1.01' 'no Bardo opens before Finish' { @(Get-BardoProcesses Any).Count -eq 0 }
    Test-Check 'S13.14' 'setup-choices.toml keeps the agent answer and the settings file, accents intact (UTF-8)' {
        $text = [System.IO.File]::ReadAllText($paths.SetupChoices, [System.Text.Encoding]::UTF8)
        $text -match 'background_agent = true' -and $text.Contains("settings_file = '''$settingsFile'''")
    } { Get-Content -Raw -Encoding utf8 $paths.SetupChoices }
    Test-ProgramFolder 'S1.01'
    Enter-Stage "the wizard's Finish, Bardo's first start"
    Invoke-WizardButton $window '^&?Finish'
    [void]$wizard.WaitForExit(60000)
    Test-Check 'S1.01' "the finished page's Launch Bardo opens it" {
        Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30
    }
    Test-FirstStart 'S1.01' $true
    $colours = Save-Screen "$Out\screens\first-start.png"
    Test-Check 'S2.05' 'without a GPU, Bardo draws its window (the screen is not blank)' { $colours -gt 16 } { "$colours colours" }

    # Uninstall while the window is open: the uninstaller refuses.
    Enter-Stage 'uninstall with the window open'
    $code = Uninstall-Bardo "$Out\uninstall-window-open.log"
    Test-Check 'S13.13' 'with the window open, the uninstall stops and leaves Bardo installed' {
        (Test-Path $paths.Exe) -and (Test-Path $paths.UninstallKey) -and @(Get-BardoProcesses Window).Count -ge 1
    } { "exit $code" }

    # Uninstall with the window closed and the agent running.
    Enter-Stage 'uninstall with the agent running'
    New-Item -ItemType Directory -Force $paths.Backups, $paths.Projects | Out-Null
    Set-Content (Join-Path $paths.Backups 'bardo-auto-test.zip') 'kept'
    Set-Content (Join-Path $paths.Projects 'kept.txt') 'kept'
    Stop-BardoWindow
    Test-Check 'S13.13' 'the agent still runs after the window closed' { @(Get-BardoProcesses Agent).Count -eq 1 }
    $running = (Get-BardoProcesses Any | ForEach-Object { $_.CommandLine }) -join '; '
    $code = Uninstall-Bardo "$Out\uninstall.log"
    Test-Check 'S13.13' 'the uninstall finishes without a file in use' {
        $code -eq 0 -and -not (Select-String -Quiet -Path "$Out\uninstall.log" -Pattern 'in use|DeleteFile failed')
    } { "exit $code; running before: $running" }
    Test-Check 'S13.13' 'the agent process is gone' { Wait-Until { @(Get-BardoProcesses Any).Count -eq 0 } 20 }
    Test-Check 'S13.13' "the agent's task is removed" { $null -eq (Get-AgentTask) }
    Test-Check 'S13.13' 'the program folder, the Start menu entry and the uninstall entry are gone' {
        -not (Test-Path $paths.Program) -and -not (Test-Path $paths.StartMenu) -and -not (Test-Path $paths.UninstallKey)
    } { (Get-ChildItem $paths.Program -Recurse -Name -ErrorAction SilentlyContinue) -join ', ' }
    Test-Check 'S13.13' 'the database, projects, logs and backups stay' {
        (Test-Path $paths.Database) -and (Test-Path (Join-Path $paths.Projects 'kept.txt')) -and
        (Test-Path (Join-Path $paths.Logs 'bardo.log')) -and (Test-Path (Join-Path $paths.Backups 'bardo-auto-test.zip'))
    }

    # Reinstall without the agent: the data is found again, the agent is off.
    Enter-Stage 'reinstall without the agent'
    $code = Install-Bardo $Setup '' "$Out\reinstall.log"
    Test-Check 'S13.13' 'the reinstall finishes' { $code -eq 0 } { "exit $code" }
    Test-Check 'S1.01' 'an install with the box unticked leaves background_agent = false' {
        (Get-Content -Raw $paths.SetupChoices) -match 'background_agent = false'
    }
    Start-BardoWindow | Out-Null
    Test-FirstStart 'S13.13' $false
    Test-Check 'S13.13' 'the reinstalled Bardo finds the projects left behind' { Test-Path (Join-Path $paths.Projects 'kept.txt') }
    Stop-BardoWindow

    # An uninstall before Bardo ever opened creates no database.
    Enter-Stage 'install and uninstall without opening Bardo'
    $code = Uninstall-Bardo "$Out\uninstall-2.log"
    Remove-Item -Recurse -Force $paths.Roaming, $paths.Local -ErrorAction SilentlyContinue
    $code = Install-Bardo $Setup '' "$Out\install-unopened.log"
    $code = Uninstall-Bardo "$Out\uninstall-unopened.log"
    Test-Check 'S13.13' 'uninstalling a Bardo that never opened leaves no database behind' {
        $code -eq 0 -and -not (Test-Path $paths.Database) -and $null -eq (Get-AgentTask)
    } { "exit $code" }
}

function Invoke-Upgrade {
    $Previous = (Resolve-Path $Previous).Path
    Enter-Stage "the published release ($(Split-Path -Leaf $Previous))"
    $code = Install-Bardo $Previous 'backgroundagent' "$Out\install-previous.log"
    Test-Check 'S1.01' 'the latest published release installs with the agent ticked' { $code -eq 0 } { "exit $code" }
    Start-BardoWindow | Out-Null
    Test-Check 'S1.01' 'the published release starts with its agent' {
        (Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 -and @(Get-BardoProcesses Agent).Count -eq 1 } 60)
    }
    $previousExe = Get-FileSha256 $paths.Exe

    # This build over it, while Bardo and its agent run.
    Enter-Stage 'this build over the running release'
    $code = Install-Bardo $Setup '' "$Out\upgrade.log"
    Test-Check 'S1.08' 'the upgrade finishes over a running Bardo and agent' { $code -eq 0 } { "exit $code" }
    Test-Check 'S1.08' 'the upgrade closed Bardo and the agent' { Wait-Until { @(Get-BardoProcesses Any).Count -eq 0 } 20 }
    Test-Check 'S1.08' "this build's files replaced the release's" {
        (Get-FileSha256 $paths.Exe) -eq $built -and $built -ne $previousExe
    }
    Test-Check 'S1.08' 'the upgrade does not ask the agent question again' { -not (Test-Path $paths.SetupChoices) }
    Test-ProgramFolder 'S1.08'
    Test-AgentTask 'S1.08'
    # The upgrade ends the agent; Task Scheduler starts it again within 15
    # minutes or at the next sign-in. Running the task now stands in for
    # that wait. An agent that finds itself off exits at once, and so does
    # one started within 30 seconds of the ended agent's last sign of life
    # (RUNNER_ALIVE), which still counts as running.
    Enter-Stage "running the agent's task"
    Start-Sleep -Seconds 35
    schtasks.exe /Run /TN $paths.TaskName | Out-Null
    Test-Check 'S1.08' 'the agent stays on and its task still works' {
        $started = Wait-Until { @(Get-BardoProcesses Agent).Count -eq 1 } 30
        Start-Sleep -Seconds 10
        $started -and @(Get-BardoProcesses Agent).Count -eq 1
    }
    Enter-Stage 'the upgraded window'
    Start-BardoWindow | Out-Null
    [void](Wait-Until { @(Get-BardoProcesses Window).Count -ge 1 } 30)
    Start-Sleep -Seconds 10
    $colours = Save-Screen "$Out\screens\after-upgrade.png"
    Test-Check 'S2.05' 'without a GPU, the upgraded Bardo draws its window' { $colours -gt 16 } { "$colours colours" }
    Stop-BardoAll

    # This build over itself, through the wizard: an upgrade shows no
    # Import settings page and no agent question, and does not reopen Bardo.
    Enter-Stage 'this build over itself through the wizard'
    $wizard = Start-Process -FilePath $Setup -ArgumentList @("/LOG=`"$Out\upgrade-wizard.log`"") -PassThru
    Import-Module (Join-Path $PSScriptRoot 'Wizard.psm1') -Force
    $window = Find-WizardWindow
    $pages = Invoke-WizardWalk $window
    $all = $pages | ForEach-Object { $_ }
    Test-Check 'S1.10' 'an upgrade shows no Import settings page' { $all -notcontains 'Import settings' } {
        ($pages | ForEach-Object { $_ -join ' | ' }) -join ' || '
    }
    Test-Check 'S1.10' 'an upgrade does not ask the agent question' {
        @($all | Where-Object { $_ -like 'Publish*Bardo is closed*' }).Count -eq 0
    }
    Test-Check 'S1.10' 'without /RELAUNCH=1 the installer does not reopen Bardo' {
        Start-Sleep -Seconds 5
        @(Get-BardoProcesses Any).Count -eq 0
    }
    Enter-Stage "the wizard's Finish"
    Invoke-WizardButton $window '^&?Finish'
    [void]$wizard.WaitForExit(60000)
    Stop-BardoAll
}

# A stuck run ends with its logs, results and a report of what hung (in
# hang.txt) instead of holding the job until its timeout, which leaves none.
Start-Watchdog -Minutes $WatchdogMinutes -Folder $Out
try {
    if ($Scenario -eq 'fresh') { Invoke-Fresh } else { Invoke-Upgrade }
    Enter-Stage 'done'
} catch {
    Test-Check 'run' "the $Scenario scenario ran to the end" { $false } { "$($_.Exception.Message) at $($_.InvocationInfo.PositionMessage)" }
} finally {
    Stop-Watchdog
    $hang = Get-WatchdogReport
    if ($hang) {
        Write-Host $hang
        Test-Check 'run' "the $Scenario scenario finished within $WatchdogMinutes minutes" { $false } { 'see hang.txt' }
    }
    Save-Screen "$Out\screens\last.png" | Out-Null
    Get-ChildItem $paths.Logs -ErrorAction SilentlyContinue | Copy-Item -Destination $Out -ErrorAction SilentlyContinue
    # Nothing the run started may outlive it: a process left behind can
    # keep the step from ending.
    $left = Stop-Leftovers
    if ($left) { Write-Host "Ended what the run left running: $($left -join ', ')" }
}
$failed = Complete-Checks "Installer ($Scenario)" $Out
# Explicit: Actions otherwise ends with the last native command's exit code.
if ($failed -gt 0) { exit 1 } else { exit 0 }
