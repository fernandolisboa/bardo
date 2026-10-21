# Drives Bardo's Inno Setup wizard through UI Automation (issue #166), to
# read which pages a first install and an upgrade show.

Set-StrictMode -Version Latest

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

# Inno Setup's controls are Delphi windows (TNewButton, TNewStaticText…)
# that UI Automation lists only as panes with no names, so the wizard is
# read and driven through their window handles, and only its check lists
# (the tasks, the finished page's Launch Bardo) through UI Automation.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class BardoWizardWin32 {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int max);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wParam, StringBuilder lParam);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wParam, string lParam);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr hwnd, int index);
    [DllImport("user32.dll")] static extern IntPtr GetParent(IntPtr hwnd);
    [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr hwnd);

    const uint WM_SETTEXT = 0x000C, WM_GETTEXT = 0x000D, WM_GETTEXTLENGTH = 0x000E, WM_COMMAND = 0x0111;
    const uint LB_GETTEXT = 0x0189, LB_GETTEXTLEN = 0x018A, LB_GETCOUNT = 0x018B;
    const int GWL_STYLE = -16, LBS_HASSTRINGS = 0x0040;

    public static string ClassOf(IntPtr hwnd) {
        var name = new StringBuilder(256);
        GetClassName(hwnd, name, name.Capacity);
        return name.ToString();
    }

    public static string TextOf(IntPtr hwnd) {
        int length = (int)SendMessage(hwnd, WM_GETTEXTLENGTH, IntPtr.Zero, IntPtr.Zero);
        var text = new StringBuilder(length + 1);
        SendMessage(hwnd, WM_GETTEXT, (IntPtr)(length + 1), text);
        return text.ToString();
    }

    public static IntPtr[] TopWindows(string className) {
        var found = new List<IntPtr>();
        EnumWindows((hwnd, _) => { if (ClassOf(hwnd) == className) found.Add(hwnd); return true; }, IntPtr.Zero);
        return found.ToArray();
    }

    public static IntPtr[] Children(IntPtr parent) {
        var found = new List<IntPtr>();
        EnumChildWindows(parent, (hwnd, _) => { found.Add(hwnd); return true; }, IntPtr.Zero);
        return found.ToArray();
    }

    // A list box's item texts, when it keeps them as strings.
    public static string[] ListItems(IntPtr hwnd) {
        var items = new List<string>();
        if ((GetWindowLong(hwnd, GWL_STYLE) & LBS_HASSTRINGS) == 0) return items.ToArray();
        int count = (int)SendMessage(hwnd, LB_GETCOUNT, IntPtr.Zero, IntPtr.Zero);
        for (int i = 0; i < count; i++) {
            int length = (int)SendMessage(hwnd, LB_GETTEXTLEN, (IntPtr)i, IntPtr.Zero);
            if (length <= 0) continue;
            var text = new StringBuilder(length + 1);
            SendMessage(hwnd, LB_GETTEXT, (IntPtr)i, text);
            items.Add(text.ToString());
        }
        return items.ToArray();
    }

    public static void SetText(IntPtr hwnd, string value) { SendMessage(hwnd, WM_SETTEXT, IntPtr.Zero, value); }

    // The button's parent hears it was clicked (BN_CLICKED), as when a
    // person clicks it, whether or not the wizard has the focus. Posted, so
    // a click that opens a message box does not block the caller.
    public static void Click(IntPtr hwnd) {
        PostMessage(GetParent(hwnd), WM_COMMAND, (IntPtr)(GetDlgCtrlID(hwnd) & 0xFFFF), hwnd);
    }
}
'@

$script:Win32 = [BardoWizardWin32]

# The wizard's window (its handle), once it shows: Inno Setup's loader
# starts a second process that owns it, a TWizardForm titled
# 'Setup - Bardo …'.
function Find-WizardWindow {
    param([int]$Seconds = 60)
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $window = $script:Win32::TopWindows('TWizardForm') |
            Where-Object { $script:Win32::IsWindowVisible($_) -and $script:Win32::TextOf($_) -like 'Setup - Bardo*' } |
            Select-Object -First 1
        if ($window) { return $window }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)
    throw 'the setup wizard did not show'
}

# The page's visible controls: each one's handle, class, text and whether
# it is enabled.
function Get-WizardControls {
    param([Parameter(Mandatory)]$Window)
    foreach ($hwnd in $script:Win32::Children($Window)) {
        if (-not $script:Win32::IsWindowVisible($hwnd)) { continue }
        [pscustomobject]@{
            Handle  = $hwnd
            Class   = $script:Win32::ClassOf($hwnd)
            Text    = $script:Win32::TextOf($hwnd)
            Enabled = $script:Win32::IsWindowEnabled($hwnd)
        }
    }
}

# Every text the current page shows: labels, buttons, and the items of its
# check lists.
function Get-WizardTexts {
    param([Parameter(Mandatory)]$Window)
    $texts = foreach ($control in Get-WizardControls $Window) {
        $control.Text
        if ($control.Class -like '*CheckListBox*' -or $control.Class -like '*ListBox*') {
            $script:Win32::ListItems($control.Handle)
            try {
                $list = [System.Windows.Automation.AutomationElement]::FromHandle($control.Handle)
                $list.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.Condition]::TrueCondition) | ForEach-Object { $_.Current.Name }
            } catch {
                Write-Verbose "no UI Automation items in $($control.Class): $_"
            }
        }
    }
    return @($texts | Where-Object { $_ } | Select-Object -Unique)
}

function Find-WizardButton {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Pattern)
    return Get-WizardControls $Window |
        Where-Object { $_.Class -like '*Button*' -and $_.Text -match $Pattern -and $_.Enabled } |
        Select-Object -First 1
}

function Invoke-WizardButton {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Pattern)
    $button = Find-WizardButton $Window $Pattern
    if (-not $button) {
        $controls = Get-WizardControls $Window | ForEach-Object { "$($_.Class) '$($_.Text)' enabled=$($_.Enabled)" }
        throw "no enabled button matches '$Pattern'; the page has: $($controls -join ', ')"
    }
    $script:Win32::Click($button.Handle)
}

# Types `Value` into the page's first edit box.
function Set-WizardEdit {
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Value)
    $edit = Get-WizardControls $Window | Where-Object { $_.Class -like '*Edit*' -and $_.Enabled } | Select-Object -First 1
    if (-not $edit) { throw 'the page has no edit box' }
    $script:Win32::SetText($edit.Handle, $Value)
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
