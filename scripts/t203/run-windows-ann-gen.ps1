<#
.SYNOPSIS
  T203 persistent ANN generations on Windows: build from SQLite, memory-mapped open,
  search with the in-memory delta and stale rows, rebuild - at 100k (and 1M with -Large).

.DESCRIPTION
  Builds lumen-bench (release) and runs `lumen-bench ann-gen` on synthetic embedding-like
  vectors (no model, no personal data). Reports (counts and timings only) go to
  docs\benchmarks\t203\<date>-<pc>\. -Large adds the 1M run (~2 GB temporary disk in
  %TEMP%, several minutes). Keep the PC plugged in and otherwise idle.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t203\run-windows-ann-gen.ps1 -Large
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$Large
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t203\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench"
        cargo build --release -p lumen-bench
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $runs = @(@{ name = "100k"; vectors = 100000; delta = 5000 })
    if ($Large) { $runs += @{ name = "1m"; vectors = 1000000; delta = 20000 } }
    foreach ($r in $runs) {
        Write-Step "ann-gen $($r.name)"
        & $exe ann-gen --vectors $r.vectors --delta $r.delta --rewrite 0.05 `
            --label "$env:COMPUTERNAME | $($r.name)" --json (Join-Path $OutDir "ann-gen-$($r.name).json")
        if ($LASTEXITCODE -ne 0) { Write-Warning "$($r.name) failed" }
    }
    Write-Step "reports: $OutDir"
}
finally {
    Pop-Location
}
