# Restart the installed current-user Agent to load config.json.
$ErrorActionPreference='Stop'
$base=Join-Path $env:LOCALAPPDATA 'Rustboard'
$agent=Join-Path $base 'bin/rustboardd.exe'
$cli=Join-Path $base 'bin/rustboard.exe'
$previousProfile=$env:RUSTBOARD_PROFILE
$env:RUSTBOARD_PROFILE='default'
try {
    & $cli stop 2>$null | Out-Null
    for ($i=0; $i -lt 50; $i++) {
        $running=Get-Process rustboardd -ErrorAction SilentlyContinue | Where-Object {$_.Path -eq $agent}
        if (!$running) {break}
        Start-Sleep -Milliseconds 100
    }
    if ($running) {throw 'Agent has not stopped; restart aborted'}
    Start-Process -FilePath $agent -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'agent.out.log') -RedirectStandardError (Join-Path $base 'agent.err.log') | Out-Null
    for ($i=0; $i -lt 50; $i++) {
        $status=& $cli doctor 2>$null
        if ($LASTEXITCODE -eq 0) {$status;return}
        Start-Sleep -Milliseconds 100
    }
    throw "Agent could not load config or protected identity; inspect $base/agent.err.log"
} finally {$env:RUSTBOARD_PROFILE=$previousProfile}
