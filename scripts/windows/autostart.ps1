param(
    [ValidateSet('Install','Remove')][string]$Action='Install',
    [string]$Agent=(Join-Path $PSScriptRoot '../../target/release/rustboardd.exe'),
    [string]$Listen=''
)
$ErrorActionPreference='Stop'
$key='HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if ($Action -eq 'Remove') {
    Remove-ItemProperty -LiteralPath $key -Name Rustboard -ErrorAction SilentlyContinue
    Write-Output 'Rustboard user autostart removed.'
    exit
}
$Agent=(Resolve-Path -LiteralPath $Agent).Path
if ($Agent.Contains('"')) { throw 'Invalid executable path' }
$command='"'+$Agent+'"'
if ($Listen) {
    $endpoint=[System.Net.IPEndPoint]::Parse($Listen)
    $command+=' --listen '+$endpoint.ToString()
}
New-Item -Path $key -Force | Out-Null
New-ItemProperty -LiteralPath $key -Name Rustboard -Value $command -PropertyType String -Force | Out-Null
Write-Output 'Rustboard will start in this user session at next login.'
