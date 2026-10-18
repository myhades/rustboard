param([string]$OutputRoot='')
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
$version=(cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
$archive=Join-Path $OutputRoot "rustboard-$version-windows-x64.zip"
Compress-Archive -Path "$out/*" -DestinationPath $archive
$hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$archive.sha256","$hash  $([IO.Path]::GetFileName($archive))`n")
Write-Output $archive
Write-Output "SHA256 $hash"
