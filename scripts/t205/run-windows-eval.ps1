<#
.SYNOPSIS
  T205 search relevance on Windows: names / contents / meaning lanes alone and fused, over
  the committed synthetic set fixtures\eval (no personal data), with the real model.

.DESCRIPTION
  Reuses the T006 downloads (.cache\t006: onnxruntime.dll + EmbeddingGemma 2 ONNX). Builds
  lumen-bench (release, --features ort) and runs `lumen-bench eval --sweep` for q4 (and q8
  with -AllVariants). Reports (aggregates and per-query ranks, no text) go to
  docs\benchmarks\t205\<date>-<pc>\. ~2 minutes per variant.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t205\run-windows-eval.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$AllVariants
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
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t205\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$threads = [math]::Max(1, [int]([Environment]::ProcessorCount / 2))

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench --features ort"
        cargo build --release -p lumen-bench --features ort
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $variants = @("q4"); if ($AllVariants) { $variants += "q8" }
    foreach ($v in $variants) {
        Write-Step "eval $v"
        & $exe eval --backend ort --ort-dylib $OrtDll --model-dir $ModelDir --variant $v `
            --threads $threads --sweep --fixture (Join-Path $Repo "fixtures\eval") `
            --label "$env:COMPUTERNAME | $v" --json (Join-Path $OutDir "eval-$v.json")
        if ($LASTEXITCODE -ne 0) { Write-Warning "eval $v failed" }
    }
    Write-Step "reports: $OutDir"
}
finally {
    Pop-Location
}
