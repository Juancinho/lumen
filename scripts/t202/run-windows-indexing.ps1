<#
.SYNOPSIS
  T202 content indexing on Windows: pipeline benchmark over a real folder, and (optionally)
  Lumen started with semantic indexing enabled (dev-only model until T210).

.DESCRIPTION
  Reuses the ONNX Runtime DLL and the EmbeddingGemma 2 ONNX model downloaded by T006
  (.cache\t006; run scripts\t006\run-windows-bench.ps1 -Download once if missing).

  1. Builds lumen-bench (release, --features ort) and runs `lumen-bench pipeline` over -Root
     with the Balanced policy's thread counts (a quarter and half of the logical CPUs) for
     -Seconds each: catalog -> content pass -> embedding queue on a temporary database.
     Reports (counts and timings only, never paths) go to docs\benchmarks\t202\<date>-<pc>\.
  2. With -Launch, builds Lumen (release) and starts it with LUMEN_EMBED_MODEL_DIR and
     LUMEN_ORT_DYLIB set, so the real app indexes your locations' contents in the
     background. Watch tray -> Content indexing.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Root D:\Proyectos\lumen\docs
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t202\run-windows-indexing.ps1 -Launch -SkipBench
#>
[CmdletBinding()]
param(
    [string]$Root = "",
    [int]$Seconds = 90,
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$SkipBench,
    [switch]$Launch
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
if (-not $Root) { $Root = [Environment]::GetFolderPath("MyDocuments") }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t202\$tag" }
$logical = [Environment]::ProcessorCount

Push-Location $Repo
try {
    if (-not $SkipBench) {
        if (-not $SkipBuild) {
            Write-Step "cargo build --release -p lumen-bench --features ort"
            cargo build --release -p lumen-bench --features ort
            if ($LASTEXITCODE -ne 0) { throw "build failed" }
        }
        New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
        $exe = Join-Path $Repo "target\release\lumen-bench.exe"
        $threads = @([math]::Max(1, [int]($logical / 4)), [math]::Max(1, [int]($logical / 2))) | Sort-Object -Unique
        foreach ($t in $threads) {
            $json = Join-Path $OutDir "pipeline-q4-t$t.json"
            Write-Step "pipeline: q4, $t threads, $Seconds s"
            & $exe pipeline --root $Root --backend ort --ort-dylib $OrtDll --model-dir $ModelDir `
                --variant q4 --threads $t --max-seconds $Seconds `
                --label "$env:COMPUTERNAME | q4 t$t" --json $json
            if ($LASTEXITCODE -ne 0) { Write-Warning "pipeline t$t failed" }
        }
        Write-Step "reports: $OutDir"
    }
    if ($Launch) {
        if (-not $SkipBuild) {
            Write-Step "building Lumen (release)"
            Push-Location (Join-Path $Repo "apps\desktop"); npm run build; Pop-Location
            cargo build --release -p lumen-desktop --features tauri/custom-protocol
            if ($LASTEXITCODE -ne 0) { throw "build failed" }
        }
        $env:LUMEN_EMBED_MODEL_DIR = $ModelDir
        $env:LUMEN_ORT_DYLIB = $OrtDll
        Write-Step "starting Lumen with semantic indexing (tray -> Content indexing)"
        Start-Process (Join-Path $Repo "target\release\lumen.exe")
    }
}
finally {
    Pop-Location
}
