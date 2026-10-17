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
Write-Output 'Login startup and binaries removed. Identity, pairing, configuration and logs retained.'
