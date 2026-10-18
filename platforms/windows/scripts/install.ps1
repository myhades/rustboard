param([string]$Listen, [string]$Source='')
$ErrorActionPreference='Stop'
if (!$Source) {
    $Source=Join-Path $PSScriptRoot '../../../bin'
    if (!(Test-Path -LiteralPath $Source)) { $Source=Join-Path $PSScriptRoot '../../../target/release' }
}
$Source=(Resolve-Path -LiteralPath $Source).Path
foreach ($name in 'rustboard.exe','rustboardd.exe') {
    if (!(Test-Path -LiteralPath (Join-Path $Source $name))) { throw "Missing $name" }
}
if ($Listen) { $Listen=([System.Net.IPEndPoint]::new([System.Net.IPAddress]::Parse($Listen.Substring(0,$Listen.LastIndexOf(':')).Trim('[',']')), [int]$Listen.Substring($Listen.LastIndexOf(':')+1))).ToString() }
$base=Join-Path $env:LOCALAPPDATA 'Rustboard'
$bin=Join-Path $base 'bin'
$data=Join-Path $base 'default'
New-Item -ItemType Directory -Path $bin,$data -Force | Out-Null
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls.exe $base /inheritance:r /grant:r "*${sid}:(OI)(CI)F" | Out-Null
if ($LASTEXITCODE) { throw 'Cannot protect installation directory' }
$previousProfile=$env:RUSTBOARD_PROFILE
$env:RUSTBOARD_PROFILE='default'
try {
    $cli=Join-Path $bin 'rustboard.exe'
    $agent=Join-Path $bin 'rustboardd.exe'
    if (Test-Path -LiteralPath $cli) {
        & $cli stop 2>$null | Out-Null
        for ($i=0; $i -lt 50; $i++) {
            $running=Get-Process rustboardd -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $agent }
            if (!$running) { break }
            Start-Sleep -Milliseconds 100
        }
        if ($running) { throw 'Installed Agent has not stopped; installation aborted' }
    }
    foreach ($name in 'rustboard.exe','rustboardd.exe') {Copy-Item -LiteralPath (Join-Path $Source $name) -Destination (Join-Path $bin $name) -Force}
    $configPath=Join-Path $data 'config.json'
    $config=[pscustomobject]@{schema=2;listen=$null;mode='all';peers=@();identity=$null}
    if (Test-Path -LiteralPath $configPath) { $config=Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json }
    if ($Listen) {$config.listen=$Listen}
    if (!$config.listen) {throw 'First install requires -Listen <LAN-IP:port>'}
    [IO.File]::WriteAllText($configPath,($config | ConvertTo-Json -Depth 16),[Text.UTF8Encoding]::new($false))
    & (Join-Path $PSScriptRoot 'autostart.ps1') -Action Install -Agent $agent
    Start-Process -FilePath $agent -WindowStyle Hidden -RedirectStandardOutput (Join-Path $base 'agent.out.log') -RedirectStandardError (Join-Path $base 'agent.err.log') | Out-Null
    for ($i=0; $i -lt 50; $i++) {
        $result=& $cli status --json 2>$null
        if ($LASTEXITCODE -eq 0) { $result; Write-Output "Installed in $bin; user login startup enabled."; return }
        Start-Sleep -Milliseconds 100
    }
    throw "Agent did not become ready. Inspect $base\agent.err.log"
} finally { $env:RUSTBOARD_PROFILE=$previousProfile }
