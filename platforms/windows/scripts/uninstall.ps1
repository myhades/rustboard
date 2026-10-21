param([switch]$Purge)
$ErrorActionPreference='Stop'
$base=Join-Path $env:LOCALAPPDATA 'Rustboard'
$bin=[IO.Path]::GetFullPath((Join-Path $base 'bin'))
if ($bin -ne [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Rustboard/bin'))) { throw 'Unexpected installation path' }
& (Join-Path $PSScriptRoot 'autostart.ps1') -Action Remove
$previousProfile=$env:RUSTBOARD_PROFILE
$env:RUSTBOARD_PROFILE='default'
try {
    $cli=Join-Path $bin 'rustboard.exe'
    if (Test-Path -LiteralPath $cli) { & $cli stop 2>$null | Out-Null }
    for ($i=0; $i -lt 50; $i++) {
        $running=Get-Process rustboardd -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq (Join-Path $bin 'rustboardd.exe')}
        if (!$running) {break}
        Start-Sleep -Milliseconds 100
    }
    if ($running) {throw 'Agent did not stop; binaries retained'}
    foreach ($name in 'rustboard.exe','rustboardd.exe') {
        $file=Join-Path $bin $name
        if (Test-Path -LiteralPath $file) {Remove-Item -LiteralPath $file}
    }
} finally {$env:RUSTBOARD_PROFILE=$previousProfile}
& (Join-Path $PSScriptRoot 'path.ps1') -Action Remove
if ($Purge -and (Test-Path -LiteralPath $base)) {
    $target=[IO.Path]::GetFullPath($base)
    $expected=[IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Rustboard'))
    if ($target -ne $expected -or ((Get-Item -LiteralPath $target).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected data directory' }
    Remove-Item -LiteralPath $target -Recurse -Force
}
Write-Output 'Uninstalled.'
