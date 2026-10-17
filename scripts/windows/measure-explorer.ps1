param(
    [Parameter(Mandatory=$true)][string]$Destination,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][int]$AgentPid
)
$ErrorActionPreference='Stop'
$destinationPath=(Resolve-Path -LiteralPath $Destination).Path
if (Test-Path -LiteralPath $OutputDirectory) {throw 'Choose a new output directory'}
$output=(New-Item -ItemType Directory -Path $OutputDirectory).FullName
$expected=(Get-Content -Raw "$PSScriptRoot/../../tests/protocol/vectors.json" | ConvertFrom-Json).hashes
$samples=[Collections.Generic.List[object]]::new()
$checks=@()
for ($i=0;$i -lt 3000;$i++) {
    $p=Get-Process -Id $AgentPid
    $now=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $samples.Add([pscustomobject]@{time_ms=$now;cpu_seconds=$p.CPU;rss=$p.WorkingSet64})
    $files=@(Get-ChildItem -LiteralPath $destinationPath -File -Filter '*.bin')
    if ($files.Count -eq 3 -and ($files | Measure-Object Length -Sum).Sum -eq 1140854784) {
        $handles=[Collections.Generic.List[IO.FileStream]]::new()
        try {
            foreach($file in $files) {$handles.Add([IO.File]::Open($file.FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None))}
            $ready=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            # All targets are closed before hashing starts; hashing is outside elapsed copy time.
            for($j=0;$j -lt $files.Count;$j++) {
                $sha=[Security.Cryptography.SHA256]::Create()
                try {$hash=[Convert]::ToHexString($sha.ComputeHash($handles[$j])).ToLowerInvariant()} finally {$sha.Dispose()}
                $checks += [pscustomobject]@{name=$files[$j].Name;size=$files[$j].Length;sha256=$hash;match=($hash -eq ($expected | Where-Object size -eq $files[$j].Length).sha256)}
            }
            break
        } catch [IO.IOException] {} finally {foreach($h in $handles){$h.Dispose()}}
    }
    Start-Sleep -Milliseconds 100
}
$samples | ConvertTo-Json -Depth 4 | Set-Content -Encoding utf8 (Join-Path $output 'samples.json')
if($checks.Count -ne 3){throw 'Native copy did not finish before the measurement timeout'}
$start=[long](Get-Content (Join-Path $output 'paste-start-ms.txt'))
$active=@($samples | Where-Object time_ms -ge $start)
$result=[pscustomobject]@{paste_start_ms=$start;all_files_ready_ms=$ready;seconds=($ready-$start)/1000;agent_cpu_seconds=$active[-1].cpu_seconds-$active[0].cpu_seconds;agent_peak_rss=($active | Measure-Object rss -Maximum).Maximum;checks=$checks;sample_interval_ms=100}
$result | ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $output 'result.json')
$result | ConvertTo-Json -Depth 5
if($checks.Count -ne 3 -or @($checks | Where-Object {-not $_.match}).Count){throw 'Native copy failed validation'}
