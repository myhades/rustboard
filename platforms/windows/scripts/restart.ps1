$ErrorActionPreference='Stop'
& "$env:LOCALAPPDATA/Rustboard/bin/rustboard.exe" restart
if ($LASTEXITCODE) {throw 'Rustboard restart failed'}
