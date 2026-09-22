# Local debug probe: drops the Windows page cache for one file by opening it
# with FILE_FLAG_NO_BUFFERING, so a cold Model Loading can be measured without
# a reboot. Prints nothing on success.
param([Parameter(Mandatory=$true)][string]$Path)

$signature = @'
using System;
using System.Runtime.InteropServices;
public static class CachePurge {
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr CreateFileW(
        string lpFileName, uint dwDesiredAccess, uint dwShareMode, IntPtr lpSecurityAttributes,
        uint dwCreationDisposition, uint dwFlagsAndAttributes, IntPtr hTemplateFile);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);
}
'@
Add-Type -TypeDefinition $signature

$GENERIC_READ = [uint32]2147483648
$FILE_SHARE_READ_WRITE = [uint32]3
$OPEN_EXISTING = [uint32]3
$FILE_FLAG_NO_BUFFERING = [uint32]0x20000000

$handle = [CachePurge]::CreateFileW(
    $Path, $GENERIC_READ, $FILE_SHARE_READ_WRITE, [IntPtr]::Zero,
    $OPEN_EXISTING, $FILE_FLAG_NO_BUFFERING, [IntPtr]::Zero)

if ($handle -eq [IntPtr]::new(-1)) {
    throw "CreateFileW failed: $([ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error()).Message)"
}
[void][CachePurge]::CloseHandle($handle)
