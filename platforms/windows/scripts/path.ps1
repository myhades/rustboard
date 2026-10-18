param([ValidateSet('Add','Remove')][string]$Action)
$ErrorActionPreference='Stop'
$entry=Join-Path $env:LOCALAPPDATA 'Rustboard\bin'
$marker=Join-Path $env:LOCALAPPDATA 'Rustboard\path-owned'
$current=[Environment]::GetEnvironmentVariable('Path','User')
$parts=if ([string]::IsNullOrEmpty($current)) {@()} else {@($current -split ';')}
function SamePath($a,$b) { return $a.Trim().TrimEnd('\') -ieq $b.TrimEnd('\') }
if ($Action -eq 'Add') {
    if (!(@($parts | Where-Object {SamePath $_ $entry}).Count)) {
        [Environment]::SetEnvironmentVariable('Path',(($parts + $entry) -join ';'),'User')
        [IO.File]::WriteAllText($marker,$entry)
    }
    if (!(@($env:Path -split ';' | Where-Object {SamePath $_ $entry}).Count)) {$env:Path += ';'+$entry}
} elseif (Test-Path -LiteralPath $marker) {
    if ((Get-Content -Raw -LiteralPath $marker) -ne $entry) {throw 'Unexpected PATH ownership marker'}
    [Environment]::SetEnvironmentVariable('Path',(@($parts | Where-Object {!(SamePath $_ $entry)}) -join ';'),'User')
    $env:Path=@($env:Path -split ';' | Where-Object {!(SamePath $_ $entry)}) -join ';'
    Remove-Item -LiteralPath $marker
}
if (-not ('RustboardEnvironment' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class RustboardEnvironment {
    [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint msg, UIntPtr wParam, string lParam, uint flags, uint timeout, out UIntPtr result);
}
'@
}
$result=[UIntPtr]::Zero
[void][RustboardEnvironment]::SendMessageTimeout([IntPtr]0xffff,0x1a,[UIntPtr]::Zero,'Environment',2,2000,[ref]$result)
