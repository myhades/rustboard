$ErrorActionPreference='Stop'
$work=Join-Path ([IO.Path]::GetTempPath()) ('rustboard-uninstall-'+[Guid]::NewGuid())
$previous=$env:LOCALAPPDATA
try {
    New-Item -ItemType Directory -Path $work | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'uninstall.ps1') -Destination $work
    # Keep the real user's registry and PATH outside this filesystem test.
    foreach ($name in 'autostart.ps1','path.ps1') {
        Set-Content -LiteralPath (Join-Path $work $name) -Value 'param($Action)'
    }
    $env:LOCALAPPDATA=Join-Path $work 'local'
    $data=Join-Path $env:LOCALAPPDATA 'Rustboard'
    New-Item -ItemType Directory -Path "$data/default/logs","$data/bin","$env:LOCALAPPDATA/Other" | Out-Null
    Set-Content -LiteralPath "$data/default/config.json" -Value 'preserve identity and peers'
    Set-Content -LiteralPath "$data/default/logs/agent.log" -Value 'log'
    $output=& "$work/uninstall.ps1"
    if ($output -cne 'Uninstalled.' -or !(Test-Path "$data/default/config.json")) {throw 'Normal uninstall did not preserve state'}
    $output=& "$work/uninstall.ps1" -Purge
    if ($output -cne 'Uninstalled.' -or (Test-Path $data)) {throw 'Purge left state'}
    if (!(Test-Path "$env:LOCALAPPDATA/Other")) {throw 'Purge removed unrelated data'}
    & "$work/uninstall.ps1" -Purge | Out-Null
    Write-Output 'PASS: uninstall preserves state; purge removes only Rustboard data and is repeatable'
} finally {
    $env:LOCALAPPDATA=$previous
    $target=[IO.Path]::GetFullPath($work)
    $temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')+'\'
    if (!$target.StartsWith($temp,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($target) -notlike 'rustboard-uninstall-*') {throw 'Unexpected test directory'}
    if (Test-Path -LiteralPath $target) {Remove-Item -LiteralPath $target -Recurse -Force}
}
