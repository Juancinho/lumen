<#
.SYNOPSIS
  T006 benchmark matrix: EmbeddingGemma 2 on ONNX Runtime (CPU + DirectML) on this Windows PC.

.DESCRIPTION
  1. (optional, -Download) fetches the runtime DLLs (PyPI wheels) and the ONNX model files
     (Hugging Face) into -CacheDir, verifying SHA-256.
  2. Builds lumen-bench (release, --features directml).
  3. Runs every configuration in its own process, with the matching onnxruntime.dll next to
     the executable (the way Lumen ships), and writes one JSON report per run plus
     machine.json and summary.md into -OutDir.

  Safe to run on any Windows 10/11 x64 machine; failed configurations are recorded, not fatal.
  Keep the machine plugged in and idle while it runs (~20-40 min).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t006\run-windows-bench.ps1 -Download
#>
[CmdletBinding()]
param(
    [string]$CacheDir = "",
    [string]$OutDir = "",
    [switch]$Download,
    [switch]$SkipBuild,
    [switch]$Quick,
    [string]$Only = ""   # comma-separated run names to restrict the matrix
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # Invoke-WebRequest is 10x slower with progress in PS 5.1

$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $CacheDir) { $CacheDir = Join-Path $Repo ".cache\t006" }
$ModelDir = Join-Path $CacheDir "embeddinggemma-2-ONNX"
$OrtCpuDir = Join-Path $CacheDir "ort-cpu"
$OrtDmlDir = Join-Path $CacheDir "ort-dml"

$OrtCpu = @{ Package = "onnxruntime"; Version = "1.30.0"; Dlls = @("onnxruntime.dll", "onnxruntime_providers_shared.dll") }
$OrtDml = @{ Package = "onnxruntime-directml"; Version = "1.24.4"; Dlls = @("onnxruntime.dll", "onnxruntime_providers_shared.dll", "DirectML.dll") }
$HfRepo = "onnx-community/embeddinggemma-2-ONNX"
$ModelFiles = @(
    "tokenizer.json",
    "onnx/model.onnx", "onnx/model.onnx_data",
    "onnx/model_fp16.onnx", "onnx/model_fp16.onnx_data",
    "onnx/model_quantized.onnx", "onnx/model_quantized.onnx_data",
    "onnx/model_q4.onnx", "onnx/model_q4.onnx_data",
    "onnx/model_q4f16.onnx", "onnx/model_q4f16.onnx_data"
)

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

function Get-Wheel($spec, $destDir) {
    if (Test-Path (Join-Path $destDir "onnxruntime.dll")) { return }
    New-Item -ItemType Directory -Force -Path $destDir | Out-Null
    $meta = Invoke-RestMethod "https://pypi.org/pypi/$($spec.Package)/$($spec.Version)/json"
    $file = $meta.urls | Where-Object { $_.filename -like "*cp312-cp312-win_amd64.whl" } | Select-Object -First 1
    if (-not $file) { throw "no win_amd64 wheel for $($spec.Package) $($spec.Version)" }
    $zip = Join-Path $destDir "wheel.zip"
    Write-Step "download $($file.filename) ($([math]::Round($file.size / 1MB, 1)) MB)"
    Invoke-WebRequest $file.url -OutFile $zip
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($hash -ne $file.digests.sha256) { throw "sha256 mismatch for $($file.filename)" }
    $tmp = Join-Path $destDir "unzipped"
    Expand-Archive $zip -DestinationPath $tmp -Force
    foreach ($dll in $spec.Dlls) {
        Copy-Item (Join-Path $tmp "onnxruntime\capi\$dll") $destDir -Force
    }
    Remove-Item $zip, $tmp -Recurse -Force
}

function Get-Model() {
    $tree = Invoke-RestMethod "https://huggingface.co/api/models/$HfRepo/tree/main?recursive=true"
    foreach ($rel in $ModelFiles) {
        $dest = Join-Path $ModelDir ($rel -replace "/", "\")
        $entry = $tree | Where-Object { $_.path -eq $rel }
        $expected = $null
        if ($entry -and $entry.lfs) { $expected = $entry.lfs.oid }
        if ((Test-Path $dest) -and ($null -eq $expected -or (Get-Item $dest).Length -eq $entry.lfs.size)) { continue }
        New-Item -ItemType Directory -Force -Path (Split-Path $dest) | Out-Null
        Write-Step "download $rel ($([math]::Round($entry.size / 1MB, 1)) MB)"
        Invoke-WebRequest "https://huggingface.co/$HfRepo/resolve/main/$rel" -OutFile $dest
        if ($expected) {
            $hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower()
            if ($hash -ne $expected) { throw "sha256 mismatch for $rel" }
        }
    }
}

function Get-MachineInfo() {
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
        @{ name = $_.Name; driver = $_.DriverVersion; adapter_ram_mb = [math]::Round($_.AdapterRAM / 1MB) }
    })
    $battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
    $onAc = $true
    if ($battery) { $onAc = ($battery.BatteryStatus -eq 2) }
    $scheme = (powercfg /getactivescheme) -join " "
    $nvidia = $null
    if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
        $nvidia = (& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader) -join "; "
    }
    return [ordered]@{
        cpu = $cpu.Name.Trim(); cores = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors
        ram_gb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
        os = "$($os.Caption) $($os.Version) (build $($os.BuildNumber))"
        gpus = $gpus; nvidia_smi = $nvidia
        on_ac_power = $onAc; power_scheme = $scheme
        ort_cpu = "$($OrtCpu.Package) $($OrtCpu.Version)"; ort_dml = "$($OrtDml.Package) $($OrtDml.Version)"
        model = $HfRepo
        date = (Get-Date).ToString("s")
    }
}

function Get-NvidiaUsedMb() {
    if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) { return $null }
    $v = & nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits 2>$null | Select-Object -First 1
    if ($v) { return [int]$v } else { return $null }
}

# ---------------------------------------------------------------------------
if ($Download) {
    Write-Step "fetching runtimes and model into $CacheDir"
    Get-Wheel $OrtCpu $OrtCpuDir
    Get-Wheel $OrtDml $OrtDmlDir
    Get-Model
}
foreach ($p in @((Join-Path $OrtCpuDir "onnxruntime.dll"), (Join-Path $OrtDmlDir "DirectML.dll"), (Join-Path $ModelDir "tokenizer.json"))) {
    if (-not (Test-Path $p)) { throw "missing $p - run with -Download" }
}

$machine = Get-MachineInfo
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t006\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$machine | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $OutDir "machine.json") -Encoding UTF8
if (-not $machine.on_ac_power) { Write-Warning "running on battery: results are not comparable" }

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench --features directml"
        cargo build --release -p lumen-bench --features directml
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $runRoot = Join-Path $CacheDir "run"
    foreach ($kind in @("cpu", "dml")) {
        $dir = Join-Path $runRoot $kind
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item $exe $dir -Force
        $src = $OrtCpuDir; if ($kind -eq "dml") { $src = $OrtDmlDir }
        Copy-Item (Join-Path $src "*.dll") $dir -Force
    }

    $iters = 200; $docs = 128; if ($Quick) { $iters = 40; $docs = 24 }
    $matrix = @(
        @{ name = "cpu-fp32";      kind = "cpu"; variant = "fp32";  device = "cpu" },
        @{ name = "cpu-q8";        kind = "cpu"; variant = "q8";    device = "cpu" },
        @{ name = "cpu-q4";        kind = "cpu"; variant = "q4";    device = "cpu" },
        @{ name = "cpu-q4-t4";     kind = "cpu"; variant = "q4";    device = "cpu"; threads = 4 },
        @{ name = "cpu-fp32-t4";   kind = "cpu"; variant = "fp32";  device = "cpu"; threads = 4 },
        @{ name = "dml-high-fp16"; kind = "dml"; variant = "fp16";  device = "dml:high" },
        @{ name = "dml-high-fp32"; kind = "dml"; variant = "fp32";  device = "dml:high" },
        @{ name = "dml-high-q4f16"; kind = "dml"; variant = "q4f16"; device = "dml:high" },
        @{ name = "dml-high-q4";   kind = "dml"; variant = "q4";    device = "dml:high" },
        @{ name = "dml-low-fp16";  kind = "dml"; variant = "fp16";  device = "dml:low" },
        @{ name = "dml-low-q4f16"; kind = "dml"; variant = "q4f16"; device = "dml:low" },
        @{ name = "dml-0-fp16";    kind = "dml"; variant = "fp16";  device = "dml:0" },
        @{ name = "dml-1-fp16";    kind = "dml"; variant = "fp16";  device = "dml:1" }
    )
    if ($Only) { $keep = $Only.Split(","); $matrix = @($matrix | Where-Object { $keep -contains $_.name }) }

    $reference = Join-Path $Repo "fixtures\embedding\reference-eg2-onnx-fp32-d256.json"
    $corpus = Join-Path $Repo "fixtures\embedding\corpus.json"
    $results = @()
    foreach ($run in $matrix) {
        $dir = Join-Path $runRoot $run.kind
        $json = Join-Path $OutDir "$($run.name).json"
        $log = Join-Path $OutDir "$($run.name).log"
        $benchArgs = @("embed", "--backend", "ort", "--ort-dylib", (Join-Path $dir "onnxruntime.dll"),
            "--model-dir", $ModelDir, "--variant", $run.variant, "--device", $run.device,
            "--iterations", $iters, "--docs", $docs, "--reference", $reference, "--corpus", $corpus,
            "--label", "$($machine.cpu) | $($run.name)", "--json", $json)
        if ($run.threads) { $benchArgs += @("--threads", $run.threads) }
        if ($run.kind -eq "dml") { $benchArgs += @("--placement") }
        Write-Step "$($run.name)"
        $vramBefore = $null; $vramPeak = $null
        $sampler = $null
        if ($run.kind -eq "dml") {
            $vramBefore = Get-NvidiaUsedMb
            if ($null -ne $vramBefore) {
                $sampler = Start-Job -ArgumentList (Join-Path $OutDir ".vram") -ScriptBlock {
                    param($file)
                    $peak = 0
                    while ($true) {
                        $v = & nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits 2>$null | Select-Object -First 1
                        if ($v -and [int]$v -gt $peak) { $peak = [int]$v; Set-Content $file $peak }
                        Start-Sleep -Milliseconds 250
                    }
                }
            }
        }
        $started = Get-Date
        # PS 5.1 turns redirected native stderr into errors; do not let that abort the matrix.
        $ErrorActionPreference = "Continue"
        & (Join-Path $dir "lumen-bench.exe") @benchArgs 2>&1 |
            ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
            Where-Object { $_ -and $_.Trim() } | Set-Content $log -Encoding UTF8
        $code = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        if ($sampler) {
            Stop-Job $sampler; Remove-Job $sampler -Force
            $vf = Join-Path $OutDir ".vram"
            if (Test-Path $vf) { $vramPeak = [int](Get-Content $vf); Remove-Item $vf }
        }
        $entry = [ordered]@{ name = $run.name; ok = ($code -eq 0); seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1)
            nvidia_vram_before_mb = $vramBefore; nvidia_vram_peak_mb = $vramPeak }
        if ($code -ne 0) {
            $entry.error = (Get-Content $log | Where-Object { $_ -match "^error:" } | Select-Object -First 1) -replace "\|", "/"
            if (-not $entry.error) { $entry.error = "exit code $code (see $($run.name).log)" }
            Write-Warning "$($run.name) failed: $($entry.error)"
        }
        $results += $entry
    }
    # With -Only, keep earlier results of runs that were not repeated.
    $runsFile = Join-Path $OutDir "runs.json"
    if ($Only -and (Test-Path $runsFile)) {
        # PS 5.1 emits a JSON array as ONE pipeline object: assign first, then enumerate.
        $parsed = Get-Content $runsFile -Raw | ConvertFrom-Json
        $previous = @($parsed | ForEach-Object { $_ } | Where-Object { $keep -notcontains $_.name })
        $results = @($previous) + @($results)
    }
    $results | ConvertTo-Json -Depth 4 | Set-Content $runsFile -Encoding UTF8

    # Summary table.
    $lines = @("# T006 benchmark - $($machine.cpu)", "",
        "$($machine.os) | $($machine.ram_gb) GB RAM | GPUs: $(($machine.gpus | ForEach-Object { $_.name }) -join ', ') | AC power: $($machine.on_ac_power)", "",
        "| run | query p50 ms | p95 | ~128-tok p50 | docs/s (b8) | cold load ms | RSS warm MiB | NVIDIA VRAM +MiB | GPU nodes % | min cos | status |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|")
    foreach ($r in $results) {
        $f = Join-Path $OutDir "$($r.name).json"
        if ($r.ok -and (Test-Path $f)) {
            $j = Get-Content $f -Raw | ConvertFrom-Json
            $b8 = $j.throughput | Where-Object { $_.batch_size -eq 8 } | Select-Object -First 1
            $vram = ""; if ($null -ne $r.nvidia_vram_peak_mb -and $null -ne $r.nvidia_vram_before_mb) { $vram = $r.nvidia_vram_peak_mb - $r.nvidia_vram_before_mb }
            $gpu = ""
            if ($j.diagnostics -and $j.diagnostics.placement -and $null -ne $j.diagnostics.placement.offloaded_fraction) {
                $gpu = "{0:N1}" -f (100 * $j.diagnostics.placement.offloaded_fraction)
            }
            $lines += ("| {0} | {1:N1} | {2:N1} | {3:N1} | {4:N1} | {5:N0} | {6:N0} | {7} | {8} | {9:N4} | ok |" -f $r.name,
                $j.query.warm.p50_ms, $j.query.warm.p95_ms, $j.query.long_input.p50_ms, $b8.items_per_s,
                $j.cold_load_ms, $j.memory.after_warm.resident_mib, $vram, $gpu, $j.fidelity.min_cosine)
        } else {
            $lines += "| $($r.name) | | | | | | | | | | FAILED: $($r.error) |"
        }
    }
    $lines | Set-Content (Join-Path $OutDir "summary.md") -Encoding UTF8
    Write-Step "done: $OutDir\summary.md"
    Get-Content (Join-Path $OutDir "summary.md")
}
finally {
    Pop-Location
}
