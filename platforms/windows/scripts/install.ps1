param([string]$Source='', [switch]$NoPath)
$ErrorActionPreference='Stop'
if (!$Source) {
    $Source=Join-Path $PSScriptRoot '../../../bin'
    if (!(Test-Path -LiteralPath $Source)) { $Source=Join-Path $PSScriptRoot '../../../target/release' }
}
$Source=(Resolve-Path -LiteralPath $Source).Path
foreach ($name in 'rustboard.exe','rustboardd.exe') {
    if (!(Test-Path -LiteralPath (Join-Path $Source $name))) { throw "Missing $name" }
}
$base=Join-Path $env:LOCALAPPDATA 'Rustboard'
$bin=Join-Path $base 'bin'
New-Item -ItemType Directory -Path $bin -Force | Out-Null
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls.exe $base /inheritance:r /grant:r "*${sid}:(OI)(CI)F" | Out-Null
if ($LASTEXITCODE) { throw 'Cannot protect installation directory' }
foreach ($name in 'rustboard.exe','rustboardd.exe') {
    Copy-Item -LiteralPath (Join-Path $Source $name) -Destination (Join-Path $bin $name) -Force
}
if (!$NoPath) { & (Join-Path $PSScriptRoot 'path.ps1') -Action Add }
Write-Output "Installed in $bin."
