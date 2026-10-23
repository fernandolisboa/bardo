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
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)] static extern IntPtr CreateJobObject(IntPtr attributes, string name);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, int length);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool QueryInformationJobObject(IntPtr job, int infoClass, IntPtr info, int length, IntPtr returned);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool TerminateJobObject(IntPtr job, uint exitCode);

    const int JobObjectBasicLimitInformation = 2, JobObjectBasicProcessIdList = 3;
    // Lets a process that asks to leave the job leave it, instead of failing
    // to start.
    const int JOB_OBJECT_LIMIT_BREAKAWAY_OK = 0x800;

    static readonly object gate = new object();
    static Timer timer;
    static int fired, reported;
    // The job this process and everything it starts belong to, even once
    // their parent is gone: ending the job ends them all.
    static IntPtr job = IntPtr.Zero;

    public static bool Fired { get { return Volatile.Read(ref fired) == 1; } }
    public static bool Reported { get { return Volatile.Read(ref reported) == 1; } }

    // After `seconds`, reports to `report` and ends the processes `names`
    // match; after `graceSeconds` more without a Disarm, ends this process
    // and everything it started. Whether this process is in its own job.
    public static bool Arm(int seconds, int graceSeconds, string report, string[] names) {
        lock (gate) {
            Disarm();
            Volatile.Write(ref fired, 0);
            Volatile.Write(ref reported, 0);
            if (job == IntPtr.Zero) {
                try { Contain(); } catch (Exception) { }
            }
            timer = new Timer(_ => Fire(graceSeconds, report, names), null, seconds * 1000L, Timeout.Infinite);
            return job != IntPtr.Zero;
        }
    }

    static void Contain() {
        var created = CreateJobObject(IntPtr.Zero, null);
        if (created == IntPtr.Zero) return;
        // JOBOBJECT_BASIC_LIMIT_INFORMATION is 64 bytes on x64, LimitFlags at 16.
        var limits = Marshal.AllocHGlobal(64);
        try {
            for (int i = 0; i < 64; i += 8) Marshal.WriteInt64(limits, i, 0);
            Marshal.WriteInt32(limits, 16, JOB_OBJECT_LIMIT_BREAKAWAY_OK);
            if (SetInformationJobObject(created, JobObjectBasicLimitInformation, limits, 64) &&
                AssignProcessToJobObject(created, Process.GetCurrentProcess().Handle)) job = created;
        } finally {
            Marshal.FreeHGlobal(limits);
        }
    }

    // Ends every process in this process's job but this one; their ids.
    public static int[] KillContained() {
        var ended = new List<int>();
        if (job == IntPtr.Zero) return ended.ToArray();
        // JOBOBJECT_BASIC_PROCESS_ID_LIST: two DWORDs, then the ids.
        const int capacity = 1024, size = 8 + capacity * 8;
        var list = Marshal.AllocHGlobal(size);
        try {
            if (!QueryInformationJobObject(job, JobObjectBasicProcessIdList, list, size, IntPtr.Zero)) return ended.ToArray();
            int count = Marshal.ReadInt32(list, 4), self = Environment.ProcessId;
            for (int i = 0; i < count && i < capacity; i++) {
                int id = (int)Marshal.ReadInt64(list, 8 + i * 8);
                if (id == self) continue;
                try {
                    using (var process = Process.GetProcessById(id)) { process.Kill(); }
                    ended.Add(id);
                } catch (Exception) { }
            }
        } finally {
            Marshal.FreeHGlobal(list);
        }
        return ended.ToArray();
    }

    public static void Disarm() {
        lock (gate) {
            if (timer != null) { timer.Dispose(); timer = null; }
        }
    }

    static void Fire(int graceSeconds, string report, string[] names) {
        // Set first, so the run stops at its next wait instead of going on.
        Volatile.Write(ref fired, 1);
        // An exception here would end the run at once, without its results.
        try { Report(report, "The run outlived its budget."); }
        catch (Exception e) { Append(report, "The run outlived its budget; no report: " + e.Message); }
        Append(report, "Ended: " + string.Join(", ", Kill(names)));
        Volatile.Write(ref reported, 1);
        Thread.Sleep(graceSeconds * 1000);
        lock (gate) {
            if (timer == null) return;
        }
        Append(report, "Still stuck " + graceSeconds + " s later: the run is ended, with everything it started.");
        // A child left without its parent could keep the step from ending.
        if (job == IntPtr.Zero || !TerminateJobObject(job, 1)) Process.GetCurrentProcess().Kill();
    }

    // Throws once the watchdog fired, so a wait stops instead of running on.
    public static void ThrowIfFired(string what) {
        if (Fired) throw new TimeoutException("the watchdog stopped the run: " + what);
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
    $contained = [BardoWatchdog]::Arm($Minutes * 60, $GraceMinutes * 60, (Join-Path $script:Folder 'hang.txt'), $script:Leftovers)
    if (-not $contained) { Write-Host 'The run is not in a job of its own: only the processes named in $Leftovers can be ended.' }
}

function Stop-Watchdog { [BardoWatchdog]::Disarm() }

# Whether the watchdog fired, and what it found.
function Get-WatchdogReport {
    if (-not [BardoWatchdog]::Fired) { return $null }
    # The run can get here while the watchdog is still writing.
    $deadline = (Get-Date).AddSeconds(60)
    while (-not [BardoWatchdog]::Reported -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200 }
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

# Ends whatever a run left running: what $Leftovers names and every
# process it started (through its job), even one whose parent is gone;
# what it ended.
function Stop-Leftovers {
    $named = @([BardoWatchdog]::Kill($script:Leftovers))
    $started = @([BardoWatchdog]::KillContained() | ForEach-Object { "process $_" })
    return @($named + $started)
}

Export-ModuleMember -Function Start-Watchdog, Stop-Watchdog, Get-WatchdogReport, Enter-Stage, Save-ProcessReport, Stop-Leftovers
