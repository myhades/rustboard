param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [string]$Binary = "$PSScriptRoot/../../../target/release/examples/stream_throughput.exe",
    [long[]]$Sizes = @(4096, 67108864, 1073741824)
)
$ErrorActionPreference = 'Stop'
$binaryPath = (Resolve-Path -LiteralPath $Binary).Path
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Choose a new output directory' }
$directory = (New-Item -ItemType Directory -Path $OutputDirectory).FullName
$vectors = Get-Content -Raw -LiteralPath "$PSScriptRoot/../../../tests/protocol/vectors.json" | ConvertFrom-Json
$results = @()
foreach ($size in $Sizes) {
    $expected = @($vectors.hashes | Where-Object size -eq $size)
    if ($expected.Count -ne 1) { throw 'Size not in shared vectors' }
    $target = Join-Path $directory "$size.bin"
    $stdout = Join-Path $directory "$size-stdout.json"
    $stderr = Join-Path $directory "$size-stderr.txt"
    $process = Start-Process -FilePath $binaryPath -ArgumentList @([string]$size, ('"' + $target + '"')) -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $peak = 0L
    $cpu = 0.0
    while (-not $process.HasExited) {
        $process.Refresh()
        try {
            $peak = [Math]::Max($peak, $process.WorkingSet64)
            $cpu = [Math]::Max($cpu, $process.TotalProcessorTime.TotalSeconds)
        } catch { if (-not $process.HasExited) { throw } }
        Start-Sleep -Milliseconds 50
    }
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) { throw "Probe failed: $(Get-Content -Raw -LiteralPath $stderr)" }
    $measurement = Get-Content -Raw -LiteralPath $stdout | ConvertFrom-Json
    # Hashing is deliberately outside the probe's timed transfer.
    $hash = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $expected[0].sha256 -or (Get-Item -LiteralPath $target).Length -ne $size) { throw 'Hash/size mismatch' }
    $results += [pscustomobject]@{ transfer=$measurement; sha256=$hash; sampled_peak_rss_bytes=$peak; sampled_cpu_seconds=$cpu; sample_interval_ms=50 }
    $results | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $directory 'results.json') -Encoding utf8
}
$results | ConvertTo-Json -Depth 6
