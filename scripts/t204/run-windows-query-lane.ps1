<#
.SYNOPSIS
  T204 query lane on Windows: warm query-embedding latency alone, next to a busy indexing
  session, and with the query lane preempting indexing - for a few thread/batch settings.

.DESCRIPTION
  Reuses the T006 downloads (.cache\t006: onnxruntime.dll + EmbeddingGemma 2 ONNX). Builds
  lumen-bench (release, --features ort) and runs `lumen-bench query-lane` for query threads
  2 and 4 against indexing at a quarter / half of the logical CPUs, with 8-chunk and 1-chunk
  indexing calls. Reports (timings only) go to docs\benchmarks\t204\<date>-<pc>\.
  Keep the PC plugged in and otherwise idle (~10 min).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t204\run-windows-query-lane.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [int]$Queries = 60
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$T006 = Join-Path $Repo ".cache\t006"
$ModelDir = Join-Path $T006 "embeddinggemma-2-ONNX"
$OrtDll = Join-Path $T006 "ort-cpu\onnxruntime.dll"
foreach ($p in @($OrtDll, (Join-Path $ModelDir "onnx\model_q4.onnx"))) {
    if (-not (Test-Path $p)) { throw "missing $p - run scripts\t006\run-windows-bench.ps1 -Download first" }
}
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t204\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$logical = [Environment]::ProcessorCount

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench --features ort"
        cargo build --release -p lumen-bench --features ort
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $indexThreads = @([math]::Max(1, [int]($logical / 4)), [math]::Max(1, [int]($logical / 2))) | Sort-Object -Unique
    foreach ($q in @(2, 4)) {
        foreach ($i in $indexThreads) {
            foreach ($b in @(8, 1)) {
                $name = "q4-query-t$q-index-t$i-b$b"
                Write-Step $name
                & $exe query-lane --backend ort --ort-dylib $OrtDll --model-dir $ModelDir --variant q4 `
                    --query-threads $q --index-threads $i --index-batch $b --queries $Queries `
                    --label "$env:COMPUTERNAME | $name" --json (Join-Path $OutDir "$name.json")
                if ($LASTEXITCODE -ne 0) { Write-Warning "$name failed" }
            }
        }
    }
    Write-Step "reports: $OutDir"
}
finally {
    Pop-Location
}
