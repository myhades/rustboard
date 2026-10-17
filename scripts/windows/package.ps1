$ErrorActionPreference='Stop'
$root=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$out=Join-Path $root 'target/prototype/windows'
if (Test-Path -LiteralPath $out) {throw 'Package staging already exists; preserve or remove it explicitly before repackaging'}
New-Item -ItemType Directory -Path "$out/bin","$out/scripts/windows","$out/docs" -Force | Out-Null
foreach ($name in 'rustboard.exe','rustboardd.exe') {Copy-Item -LiteralPath "$root/target/release/$name" -Destination "$out/bin/$name"}
foreach ($name in 'install.ps1','uninstall.ps1','autostart.ps1') {Copy-Item -LiteralPath "$PSScriptRoot/$name" -Destination "$out/scripts/windows/$name"}
Copy-Item -LiteralPath "$root/LICENSE" -Destination $out
foreach ($name in "PROTOTYPE_0_1_0.md","PROTOTYPE_RESULTS.md") {Copy-Item -LiteralPath "$root/docs/$name" -Destination "$out/docs"}
$archive=Join-Path $root 'target/prototype/rustboard-0.1.0-prototype-windows-x64.zip'
Compress-Archive -Path "$out/*" -DestinationPath $archive
$hash=(Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText("$archive.sha256","$hash  $([IO.Path]::GetFileName($archive))`n")
Write-Output $archive
Write-Output "SHA256 $hash"
