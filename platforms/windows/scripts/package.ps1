param([string]$OutputRoot='', [string]$ReleaseUrl='')
$ErrorActionPreference='Stop'
$root=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if (!$OutputRoot) {$OutputRoot=Join-Path $root 'target/prototype'}
$out=Join-Path $OutputRoot 'windows'
if (Test-Path -LiteralPath $out) {throw 'Package staging already exists; preserve or remove it explicitly before repackaging'}
New-Item -ItemType Directory -Path "$out/bin","$out/platforms/windows/scripts","$out/docs" -Force | Out-Null
foreach ($name in 'rustboard.exe','rustboardd.exe') {Copy-Item -LiteralPath "$root/target/release/$name" -Destination "$out/bin/$name"}
foreach ($name in 'install.ps1','uninstall.ps1','autostart.ps1','restart.ps1','path.ps1') {Copy-Item -LiteralPath "$PSScriptRoot/$name" -Destination "$out/platforms/windows/scripts/$name"}
Copy-Item -LiteralPath "$root/LICENSE" -Destination $out
foreach ($name in "INSTALLATION.md","STATUS.md","SECURITY.md") {Copy-Item -LiteralPath "$root/docs/$name" -Destination "$out/docs"}
$archive=Join-Path $OutputRoot 'rustboard-0.1.0-prototype-windows-x64.zip'
Compress-Archive -Path "$out/*" -DestinationPath $archive
$hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$archive.sha256","$hash  $([IO.Path]::GetFileName($archive))`n")
Write-Output $archive
Write-Output "SHA256 $hash"

if ($ReleaseUrl) {
    if (!$ReleaseUrl.StartsWith('https://')) {throw 'Release URL must be HTTPS'}
    $manifest=[ordered]@{
        version='0.1.0'
        description='Native clipboard synchronization with lazy file transfer'
        homepage='https://github.com/myhades/rustboard'
        license='MIT'
        architecture=@{'64bit'=@{url=$ReleaseUrl;hash=$hash}}
        bin=@('bin/rustboard.exe')
        pre_install="if (`$global) { throw 'Rustboard must be installed per user, without --global' }"
        installer=@{script='& "$dir\platforms\windows\scripts\install.ps1" -Source "$dir\bin" -NoPath'}
        uninstaller=@{script='if ($cmd -eq "uninstall") { & "$dir\platforms\windows\scripts\uninstall.ps1" }'}
        notes=@('Configure listen and peers with rustboard config, then rustboard reload.','Uninstall retains identity and pairing. Scoop manages the CLI shim.')
    }
    $bucket=Join-Path $OutputRoot 'scoop-bucket/bucket'
    New-Item -ItemType Directory -Path $bucket -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $bucket 'rustboard.json'),($manifest | ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
}
