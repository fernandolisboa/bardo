# Drives Bardo's Inno Setup wizard through UI Automation (issue #166), to
# read which pages a first install and an upgrade show.

Set-StrictMode -Version Latest

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$script:Auto = [System.Windows.Automation.AutomationElement]
$script:Tree = [System.Windows.Automation.TreeScope]

# The wizard's window, once it shows: Inno Setup's loader starts a second
# process that owns it, so it is found by its title.
function Find-WizardWindow {
    param([int]$Seconds = 60)
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $windows = $script:Auto::RootElement.FindAll($script:Tree::Children,
            [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($window in $windows) {
            if ($window.Current.Name -like 'Setup - Bardo*') { return $window }
        }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw 'the setup wizard did not show'
}

function Get-Descendants {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)]$ControlType)
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        $script:Auto::ControlTypeProperty, $ControlType)
    return @($Window.FindAll($script:Tree::Descendants, $condition))
}

# Every text the current page shows (labels, list items, checkboxes).
function Get-WizardTexts {
    param([Parameter(Mandatory)]$Window)
    $types = @(
        [System.Windows.Automation.ControlType]::Text,
        [System.Windows.Automation.ControlType]::CheckBox,
        [System.Windows.Automation.ControlType]::ListItem
    )
    $texts = foreach ($type in $types) {
        Get-Descendants $Window $type | ForEach-Object { $_.Current.Name }
    }
    return @($texts | Where-Object { $_ })
}

function Find-WizardButton {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Pattern)
    return Get-Descendants $Window ([System.Windows.Automation.ControlType]::Button) |
        Where-Object { $_.Current.Name -match $Pattern -and $_.Current.IsEnabled } |
        Select-Object -First 1
}

function Invoke-WizardButton {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Pattern)
    $button = Find-WizardButton $Window $Pattern
    if (-not $button) { throw "no enabled button matches '$Pattern'" }
    $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}

# Types `Value` into the page's first edit box.
function Set-WizardEdit {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Value)
    $edit = Get-Descendants $Window ([System.Windows.Automation.ControlType]::Edit) |
        Where-Object { $_.Current.IsEnabled } | Select-Object -First 1
    if (-not $edit) { throw 'the page has no edit box' }
    $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
}

# Waits for the page whose texts include `Text`.
function Wait-WizardPage {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Text, [int]$Seconds = 120)
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        if ((Get-WizardTexts $Window) -contains $Text) { return }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw "the wizard never showed '$Text'"
}

# Walks the wizard from its first page to the finished page with Next and
# Install, calling `OnPage` with each page's texts first. `OnPage` may fill
# the page in. Returns each page's texts, in order.
function Invoke-WizardWalk {
    param([Parameter(Mandatory)]$Window, [scriptblock]$OnPage, [int]$MaxPages = 12)
    $pages = [System.Collections.Generic.List[object]]::new()
    for ($n = 0; $n -lt $MaxPages; $n++) {
        Start-Sleep -Milliseconds 700
        $texts = Get-WizardTexts $Window
        $pages.Add($texts)
        if ($OnPage) { & $OnPage $texts }
        if (Find-WizardButton $Window '^&?Finish') { return $pages }
        if (Find-WizardButton $Window '^&?Install$') {
            Invoke-WizardButton $Window '^&?Install$'
            # Copying the files takes a while.
            $deadline = (Get-Date).AddMinutes(5)
            while (-not (Find-WizardButton $Window '^&?Finish') -and (Get-Date) -lt $deadline) {
                Start-Sleep -Seconds 1
            }
            continue
        }
        Invoke-WizardButton $Window '^&?Next'
    }
    throw "the wizard did not finish within $MaxPages pages"
}

Export-ModuleMember -Function Find-WizardWindow, Get-WizardTexts, Find-WizardButton,
    Invoke-WizardButton, Set-WizardEdit, Wait-WizardPage, Invoke-WizardWalk
