# Keeps a stuck run from holding a Windows end-to-end job until its timeout
# (issue #214). A job that times out is killed before its artifact upload,
# so nothing it wrote survives; a run that ends on its own, even in failure,
# leaves its logs, screenshots and results.
#
# The watchdog runs on a thread of its own: a run blocked in a call that
# never returns (a wait on an installer, a message to a hung window) cannot
# stop it. When the run outlives its budget it writes what was running and
# which windows were open, then ends the installers and Bardo, which frees
# the blocked call. A run still stuck after that is ended itself.

Set-StrictMode -Version Latest

if (-not ('BardoWatchdog' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class BardoWatchdog {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool IsHungAppWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd, StringBuilder name, int max);
    // Reads a window's text without sending it a message, so a hung window
    // cannot block the report.
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int InternalGetWindowText(IntPtr hwnd, StringBuilder text, int max);

    static readonly object gate = new object();
    static Timer timer;
    static int fired;

    public static bool Fired { get { return Volatile.Read(ref fired) == 1; } }

    // After `seconds`, reports to `report` and ends the processes `names`
    // match; after `graceSeconds` more without a Disarm, ends this process.
    public static void Arm(int seconds, int graceSeconds, string report, string[] names) {
        lock (gate) {
            Disarm();
            Volatile.Write(ref fired, 0);
            timer = new Timer(_ => Fire(graceSeconds, report, names), null, seconds * 1000L, Timeout.Infinite);
        }
    }

    public static void Disarm() {
        lock (gate) {
            if (timer != null) { timer.Dispose(); timer = null; }
        }
    }

    static void Fire(int graceSeconds, string report, string[] names) {
        Volatile.Write(ref fired, 1);
        // An exception here would end the run at once, without its results.
        try { Report(report, "The run outlived its budget."); }
        catch (Exception e) { Append(report, "The run outlived its budget; no report: " + e.Message); }
        Append(report, "Ended: " + string.Join(", ", Kill(names)));
        Thread.Sleep(graceSeconds * 1000);
        lock (gate) {
            if (timer == null) return;
        }
        Append(report, "Still stuck " + graceSeconds + " s later: the run is ended.");
        Process.GetCurrentProcess().Kill();
    }

    static bool Matches(string name, string[] names) {
        foreach (var pattern in names) {
            if (pattern.EndsWith("*")
                ? name.StartsWith(pattern.TrimEnd('*'), StringComparison.OrdinalIgnoreCase)
                : string.Equals(name, pattern, StringComparison.OrdinalIgnoreCase)) return true;
        }
        return false;
    }

    // Ends every process `names` match, with its children; what it ended.
    public static string[] Kill(string[] names) {
        var ended = new List<string>();
        foreach (var process in Process.GetProcesses()) {
            try {
                if (!Matches(process.ProcessName, names)) continue;
                ended.Add(process.ProcessName + " (" + process.Id + ")");
                process.Kill(true);
            } catch (Exception) {
                // Gone already, or not ours to end.
            } finally {
                process.Dispose();
            }
        }
        return ended.ToArray();
    }

    static void Append(string path, string text) {
        try { File.AppendAllText(path, text + Environment.NewLine); } catch (Exception) { }
    }

    static string Text(IntPtr hwnd) {
        var text = new StringBuilder(512);
        InternalGetWindowText(hwnd, text, text.Capacity);
        return text.ToString();
    }

    // Writes `why`, every process, and every visible window with its owner,
    // class, title, whether it is hung, and its controls' texts (a message
    // box's question, a wizard's page).
    public static void Report(string path, string why) {
        var lines = new StringBuilder();
        lines.AppendLine(DateTime.Now.ToString("o") + " " + why);
        lines.AppendLine();
        lines.AppendLine("Processes:");
        var owners = new Dictionary<uint, string>();
        foreach (var process in Process.GetProcesses()) {
            try {
                owners[(uint)process.Id] = process.ProcessName;
                string started;
                try { started = process.StartTime.ToString("HH:mm:ss"); } catch (Exception) { started = "?"; }
                lines.AppendLine("  " + process.Id + "\t" + process.ProcessName + "\tstarted " + started);
            } catch (Exception) {
            } finally {
                process.Dispose();
            }
        }
        lines.AppendLine();
        lines.AppendLine("Visible windows:");
        EnumWindows((hwnd, _) => {
            if (!IsWindowVisible(hwnd)) return true;
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            string owner;
            if (!owners.TryGetValue(pid, out owner)) owner = "?";
            var name = new StringBuilder(256);
            GetClassName(hwnd, name, name.Capacity);
            lines.AppendLine("  " + owner + " (" + pid + ") " + name + " '" + Text(hwnd) + "'" +
                (IsHungAppWindow(hwnd) ? " NOT RESPONDING" : ""));
            var texts = new List<string>();
            EnumChildWindows(hwnd, (child, __) => {
                var text = Text(child);
                if (text.Length > 0 && IsWindowVisible(child) && texts.Count < 40) texts.Add(text.Replace("\r", " ").Replace("\n", " "));
                return true;
            }, IntPtr.Zero);
            if (texts.Count > 0) lines.AppendLine("    " + string.Join(" | ", texts));
            return true;
        }, IntPtr.Zero);
        Append(path, lines.ToString());
    }
}
'@
}

# The processes a stuck run leaves behind: Bardo and its agent, an
# installer (its loader and the copy it runs from a temporary folder), an
# uninstaller (and its temporary copy, _iu*.tmp).
$script:Leftovers = @('bardo', 'Bardo-Setup*', 'unins000', '_iu*')
$script:Folder = $null

# Arms the watchdog for `Minutes`; its report goes to hang.txt in `Folder`,
# and each stage the run enters to stages.txt there.
function Start-Watchdog {
    param([Parameter(Mandatory)][int]$Minutes, [int]$GraceMinutes = 3, [Parameter(Mandatory)][string]$Folder)
    $script:Folder = (New-Item -ItemType Directory -Force $Folder).FullName
    [BardoWatchdog]::Arm($Minutes * 60, $GraceMinutes * 60, (Join-Path $script:Folder 'hang.txt'), $script:Leftovers)
}

function Stop-Watchdog { [BardoWatchdog]::Disarm() }

# Whether the watchdog fired, and what it found.
function Get-WatchdogReport {
    if (-not [BardoWatchdog]::Fired) { return $null }
    return Get-Content -Raw -ErrorAction SilentlyContinue (Join-Path $script:Folder 'hang.txt')
}

# Marks the start of a stage, in the job's log and stages.txt, so a stuck
# run shows where it stopped. Once the watchdog has fired, the run stops
# here instead of going on with what is left.
function Enter-Stage {
    param([Parameter(Mandatory)][string]$What)
    $line = "$((Get-Date).ToString('HH:mm:ss')) $What"
    Write-Host "== $line"
    if ($script:Folder) { Add-Content -Path (Join-Path $script:Folder 'stages.txt') -Value $line }
    if ([BardoWatchdog]::Fired) { throw "the watchdog stopped the run before: $What" }
}

# Writes what runs and which windows are open to `Path`, saying `Why`.
function Save-ProcessReport {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)][string]$Why)
    [BardoWatchdog]::Report($Path, $Why)
}

# Ends whatever a run left running (see $Leftovers); what it ended.
function Stop-Leftovers {
    return @([BardoWatchdog]::Kill($script:Leftovers))
}

Export-ModuleMember -Function Start-Watchdog, Stop-Watchdog, Get-WatchdogReport, Enter-Stage, Save-ProcessReport, Stop-Leftovers
