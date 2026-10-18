$ErrorActionPreference='Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {throw 'Run only on a disposable hosted runner'}
$env:SCOOP=Join-Path $env:RUNNER_TEMP 'rustboard-scoop'
$bootstrap=Join-Path $env:RUNNER_TEMP 'install-scoop.ps1'
Invoke-WebRequest 'https://raw.githubusercontent.com/ScoopInstaller/Install/1e2f334083d609986d8c8bc9e31ae8e87c39fab4/install.ps1' -OutFile $bootstrap
& $bootstrap -RunAsAdmin -ScoopDir $env:SCOOP
$scoop=Join-Path $env:SCOOP 'shims/scoop.ps1'
$bucket=Join-Path $env:RUNNER_TEMP 'rustboard-test-bucket'
New-Item -ItemType Directory -Force "$bucket/bucket" | Out-Null
$manifest=Get-Content target/manifests/scoop-bucket/bucket/rustboard.json -Raw | ConvertFrom-Json
$archive=Get-ChildItem target/release-assets/*.zip
$manifest.architecture.'64bit'.url="http://127.0.0.1:18765/$($archive.Name)"
$path=Join-Path $bucket 'bucket/rustboard.json'
$manifest | ConvertTo-Json -Depth 16 | Set-Content $path
git -C $bucket init -b main
git -C $bucket -c user.name=CI -c user.email=ci@example.invalid add .
git -C $bucket -c user.name=CI -c user.email=ci@example.invalid commit -m fixture
$server=Start-Process python -ArgumentList '-m','http.server','18765','--bind','127.0.0.1','--directory','target/release-assets' -WindowStyle Hidden -PassThru
try {
    & $scoop bucket add rustboard $bucket
    & $scoop install rustboard/rustboard
    $cli=Join-Path $env:SCOOP 'shims/rustboard.exe'
    & $cli status --json
    if ($LASTEXITCODE) {throw 'Installed CLI/service not ready'}
    $config=Join-Path $env:LOCALAPPDATA 'Rustboard/default/config.json'
    $identity=(Get-Content $config -Raw | ConvertFrom-Json).identity | ConvertTo-Json -Depth 16 -Compress
    & $cli stop
    if ($LASTEXITCODE) {throw 'Stop failed'}
    # New package version, identical test binaries: exercise Scoop's update hooks.
    $manifest.version='999.0.0'
    $manifest | ConvertTo-Json -Depth 16 | Set-Content $path
    git -C $bucket -c user.name=CI -c user.email=ci@example.invalid commit -am upgrade
    & $scoop update rustboard
    if (Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name Rustboard -ErrorAction SilentlyContinue) {throw 'Update enabled a disabled service'}
    $after=(Get-Content $config -Raw | ConvertFrom-Json).identity | ConvertTo-Json -Depth 16 -Compress
    if ($after -cne $identity) {throw 'Update changed device identity'}
    & $cli start
    if ($LASTEXITCODE) {throw 'Restart failed'}
    & $scoop uninstall rustboard
    if (Test-Path $cli) {throw 'Uninstall left CLI shim'}
    if (Test-Path (Join-Path $env:LOCALAPPDATA 'Rustboard/bin/rustboard.exe')) {throw 'Uninstall left installed binary'}
    if (!(Test-Path $config)) {throw 'Uninstall deleted user config'}
} finally {
    Stop-Process -Id $server.Id -ErrorAction SilentlyContinue
}
