<#
.SYNOPSIS
  T008 ANN benchmark on this Windows PC: USearch/HNSW at 256d, 100k and 1M vectors.

.DESCRIPTION
  Builds lumen-bench (release) and runs `lumen-bench ann`:
    - 100k vectors, f32/f16/bf16/i8, ef sweep 16..256
    - 1M vectors, f32/f16/i8, ef sweep 32..256
  Synthetic "embedding-like" data calibrated on real EmbeddingGemma 2 vectors; nothing is
  downloaded. Needs ~3 GB free RAM and ~1.5 GB temporary disk. Takes ~10-20 min.
  Keep the PC plugged in and idle.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t008\run-windows-ann.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$Quick
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem
$battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
$onAc = $true; if ($battery) { $onAc = ($battery.BatteryStatus -eq 2) }
$machine = [ordered]@{
    cpu = $cpu.Name.Trim(); cores = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors
    ram_gb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
    os = "$($os.Caption) $($os.Version) (build $($os.BuildNumber))"
    on_ac_power = $onAc; power_scheme = ((powercfg /getactivescheme) -join " ")
    date = (Get-Date).ToString("s")
}
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t008\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$machine | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutDir "machine.json") -Encoding UTF8
if (-not $onAc) { Write-Warning "running on battery: results are not comparable" }

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench"
        cargo build --release -p lumen-bench
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $work = Join-Path $Repo ".cache\t008"
    New-Item -ItemType Directory -Force -Path $work | Out-Null

    $runs = @(
        @{ name = "ann-100k"; args = @("--sizes", "100000", "--scalars", "f32,f16,bf16,i8", "--efs", "16,32,64,128,256", "--queries", "500") },
        @{ name = "ann-1m";   args = @("--sizes", "1000000", "--scalars", "f32,f16,i8", "--efs", "32,64,128,256", "--queries", "300") }
    )
    if ($Quick) { $runs = @(@{ name = "ann-quick"; args = @("--sizes", "20000", "--scalars", "f32,f16", "--efs", "32,64", "--queries", "100") }) }

    foreach ($run in $runs) {
        Write-Step $run.name
        $json = Join-Path $OutDir "$($run.name).json"
        $log = Join-Path $OutDir "$($run.name).log"
        $benchArgs = @("ann") + $run.args + @("--work-dir", $work, "--label", "$($machine.cpu) | $($run.name)", "--json", $json)
        $ErrorActionPreference = "Continue"
        & $exe @benchArgs 2>&1 |
            ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
            Where-Object { $_ -and $_.Trim() } | Tee-Object -FilePath $log
        $code = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($code -ne 0) { Write-Warning "$($run.name) failed (exit $code), see $log" }
    }
    Write-Step "done: $OutDir"
}
finally {
    Pop-Location
}
