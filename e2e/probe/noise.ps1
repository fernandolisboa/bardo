# Throwaway probe for #200; removed before merge. Writes and deletes
# Local Machine generic credentials for a while, as another process of
# the same user would.
param([int]$Seconds = 900, [string]$Tag = "noise")
$ErrorActionPreference = "Stop"
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Cred {
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  public struct CREDENTIAL {
    public int Flags; public int Type; public string TargetName; public string Comment;
    public long LastWritten; public int CredentialBlobSize; public IntPtr CredentialBlob;
    public int Persist; public int AttributeCount; public IntPtr Attributes;
    public string TargetAlias; public string UserName;
  }
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CredWriteW(ref CREDENTIAL c, int flags);
  [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
  public static extern bool CredDeleteW(string target, int type, int flags);
  public static bool Write(string target) {
    var blob = Marshal.StringToHGlobalUni("x");
    var c = new CREDENTIAL { Type = 1, TargetName = target, CredentialBlobSize = 2, CredentialBlob = blob, Persist = 2, UserName = "noise" };
    var ok = CredWriteW(ref c, 0);
    Marshal.FreeHGlobal(blob);
    return ok;
  }
  public static bool Delete(string target) { return CredDeleteW(target, 1, 0); }
}
"@
$log = Join-Path $env:TEMP "bardo-noise-$Tag.log"
"start pid=$PID session=$((Get-Process -Id $PID).SessionId) logon=$(whoami /logonid)" | Out-File $log
$end = (Get-Date).AddSeconds($Seconds)
$n = 0
while ((Get-Date) -lt $end) {
  $t = "BardoNoise/$Tag/$($n % 20)"
  [void][Cred]::Write($t)
  [void][Cred]::Delete($t)
  $n++
}
"done writes=$n" | Out-File $log -Append
