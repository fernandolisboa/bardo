# Bardo's window from outside (issue #166): found by its process, moved,
# sized, read (title, icon, inner size) and photographed, through Win32.

Set-StrictMode -Version Latest

Add-Type -AssemblyName System.Drawing
# The C# names no System.Drawing type: on .NET 10 those spread over
# assemblies Add-Type does not reference. Pictures are taken in PowerShell.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class BardoWindowWin32 {
    delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }

    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int max);
    [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr hwnd, out RECT rect);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref POINT point);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll", EntryPoint = "GetClassLongPtrW")] static extern IntPtr GetClassLongPtr(IntPtr hwnd, int index);

    const uint SWP_NOSIZE = 0x0001, SWP_NOMOVE = 0x0002, SWP_SHOWWINDOW = 0x0040;
    const uint WM_GETICON = 0x007F;
    static readonly IntPtr HWND_TOPMOST = new IntPtr(-1);

    // The process's visible top-level window with a title, or zero.
    public static IntPtr WindowOf(int processId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hwnd, _) => {
            uint pid;
            GetWindowThreadProcessId(hwnd, out pid);
            if (pid == processId && IsWindowVisible(hwnd) && GetWindowTextLength(hwnd) > 0) {
                found = hwnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static string Title(IntPtr hwnd) {
        var text = new StringBuilder(GetWindowTextLength(hwnd) + 1);
        GetWindowText(hwnd, text, text.Capacity);
        return text.ToString();
    }

    // The inner area, in screen pixels: left, top, width, height.
    public static int[] Client(IntPtr hwnd) {
        RECT rect;
        GetClientRect(hwnd, out rect);
        var origin = new POINT();
        ClientToScreen(hwnd, ref origin);
        return new[] { origin.X, origin.Y, rect.Right - rect.Left, rect.Bottom - rect.Top };
    }

    // Above every other window, its corner at the screen's.
    public static void Front(IntPtr hwnd) {
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOSIZE | SWP_SHOWWINDOW);
    }

    // Asks for an outer size, as dragging the window's edge does.
    public static void Resize(IntPtr hwnd, int width, int height) {
        SetWindowPos(hwnd, IntPtr.Zero, 0, 0, width, height, SWP_NOMOVE);
    }

    // Whether the window has an icon for the taskbar and Alt+Tab.
    public static bool HasIcon(IntPtr hwnd) {
        return SendMessage(hwnd, WM_GETICON, (IntPtr)1, IntPtr.Zero) != IntPtr.Zero
            || GetClassLongPtr(hwnd, -14) != IntPtr.Zero;
    }
}
'@

# The window of process `ProcessId`, waiting up to `Seconds` for it.
function Get-BardoWindowHandle {
    param([Parameter(Mandatory)][int]$ProcessId, [int]$Seconds = 60)
    $deadline = (Get-Date).AddSeconds($Seconds)
    do {
        $handle = [BardoWindowWin32]::WindowOf($ProcessId)
        if ($handle -ne [IntPtr]::Zero) { return $handle }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw "no window for process $ProcessId within $Seconds s"
}

# Saves the window's inner area as a PNG.
function Save-BardoWindow {
    param([Parameter(Mandatory)][IntPtr]$Handle, [Parameter(Mandatory)][string]$Path)
    $area = [BardoWindowWin32]::Client($Handle)
    $bitmap = [System.Drawing.Bitmap]::new($area[2], $area[3])
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($area[0], $area[1], 0, 0, [System.Drawing.Size]::new($area[2], $area[3]))
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

# The process's processor time over `Seconds`, as a share of one core.
function Measure-IdleCpu {
    param([Parameter(Mandatory)][int]$ProcessId, [int]$Seconds = 15)
    $process = Get-Process -Id $ProcessId
    $before = $process.TotalProcessorTime
    Start-Sleep -Seconds $Seconds
    $process.Refresh()
    return ($process.TotalProcessorTime - $before).TotalSeconds / $Seconds
}

Export-ModuleMember -Function Get-BardoWindowHandle, Save-BardoWindow, Measure-IdleCpu
