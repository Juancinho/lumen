<#
.SYNOPSIS
  T014 indexing-throughput spike: chunks/s and the CPU share they cost, for EmbeddingGemma 2
  on ONNX Runtime (thread sweep, DirectML) and llama.cpp (CPU, Vulkan, CUDA builds).

.DESCRIPTION
  Every run embeds ~128-token documents (the T201 chunk target, --doc-words 100) in batches
  of 1, 8 and 16 and records items/s plus the CPU time of the process doing the work
  (lumen-bench itself for ONNX Runtime, llama-server for llama.cpp), so each configuration
  becomes a point on a "chunks/s at CPU %" curve. Fidelity against the fp32 reference is
  checked on every run (different weights must stay close; ADR-014 spaces).

  1. (-Download) fetches into -CacheDir:
       - ONNX Runtime CPU/DirectML DLLs and the ONNX model (same files as T006; reused
         from .cache\t006 when present),
       - the latest llama.cpp Windows release zips (cpu, vulkan, cuda 12.x + cudart) from
         GitHub, SHA-256 checked when GitHub publishes a digest,
       - GGUF weights from Hugging Face (Q8_0 from ggml-org, UD-Q4_K_XL from unsloth),
         SHA-256 checked against the LFS oid.
  2. Builds lumen-bench (release, --features directml).
  3. Runs the matrix; one JSON + log per run, plus machine.json, runs.json and summary.md in
     docs\benchmarks\t014\<date>-<pc>\. Reports hold counts and timings only.

  Failed configurations (no GPU, driver too old, ...) are recorded, not fatal. Keep the PC
  plugged in and idle; ~20-40 min. Needs ~3 GB of downloads the first time.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t014\run-windows-throughput.ps1 -Download
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t014\run-windows-throughput.ps1 -Only "ort-q4-t2,llama-vulkan-q8_0"
#>
[CmdletBinding()]
param(
    [string]$CacheDir = "",
    [string]$OutDir = "",
    [switch]$Download,
    [switch]$SkipBuild,
    [switch]$Quick,
    [string]$Only = "",          # comma-separated run names
    [int]$Port = 8089
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $CacheDir) { $CacheDir = Join-Path $Repo ".cache\t014" }
$T006 = Join-Path $Repo ".cache\t006"
$ModelDir = Join-Path $T006 "embeddinggemma-2-ONNX"
$OrtCpuDir = Join-Path $T006 "ort-cpu"
$OrtDmlDir = Join-Path $T006 "ort-dml"
$LlamaDir = Join-Path $CacheDir "llama.cpp"
$GgufDir = Join-Path $CacheDir "gguf"

$Ggufs = @(
    @{ key = "q8_0";    repo = "ggml-org/embeddinggemma-2-GGUF"; pattern = "*Q8_0.gguf" },
    @{ key = "q4_k_xl"; repo = "unsloth/embeddinggemma-2-GGUF";  pattern = "*UD-Q4_K_XL.gguf" }
)
$LlamaBuilds = @(
    @{ key = "cpu";    pattern = "llama-*-bin-win-cpu-x64.zip" },
    @{ key = "vulkan"; pattern = "llama-*-bin-win-vulkan-x64.zip" },
    @{ key = "cuda";   pattern = "llama-*-bin-win-cuda-12*-x64.zip"; extra = "cudart-llama-bin-win-cuda-12*-x64.zip" }
)

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

function Get-Checked($url, $dest, $sha256) {
    New-Item -ItemType Directory -Force -Path (Split-Path $dest) | Out-Null
    Invoke-WebRequest $url -OutFile $dest -UseBasicParsing
    if ($sha256) {
        $hash = (Get-FileHash $dest -Algorithm SHA256).Hash.ToLower()
        if ($hash -ne $sha256.ToLower()) { Remove-Item $dest; throw "sha256 mismatch for $dest" }
    }
}

function Get-Llama() {
    $release = Invoke-RestMethod "https://api.github.com/repos/ggml-org/llama.cpp/releases/latest" -Headers @{ "User-Agent" = "lumen-t014" }
    Set-Content (Join-Path $LlamaDir "release.txt") $release.tag_name -Encoding UTF8
    foreach ($b in $LlamaBuilds) {
        $dir = Join-Path $LlamaDir $b.key
        if (Test-Path (Join-Path $dir "llama-server.exe")) { continue }
        $patterns = @($b.pattern); if ($b.extra) { $patterns += $b.extra }
        foreach ($p in $patterns) {
            # Highest matching version first (cuda 12.4 over 12.2, ...).
            $asset = $release.assets | Where-Object { $_.name -like $p } | Sort-Object name -Descending | Select-Object -First 1
            if (-not $asset) { Write-Warning "llama.cpp $($release.tag_name): no asset like $p"; continue }
            $zip = Join-Path $LlamaDir $asset.name
            $digest = $null; if ($asset.digest -and $asset.digest -like "sha256:*") { $digest = $asset.digest.Substring(7) }
            Write-Step "download $($asset.name) ($([math]::Round($asset.size / 1MB, 1)) MB)"
            Get-Checked $asset.browser_download_url $zip $digest
            Expand-Archive $zip -DestinationPath $dir -Force
            Remove-Item $zip
        }
        # Some releases nest the binaries in a folder: flatten to $dir.
        $server = Get-ChildItem $dir -Recurse -Filter "llama-server.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($server -and $server.DirectoryName -ne $dir) {
            Get-ChildItem $server.DirectoryName | Move-Item -Destination $dir -Force
        }
    }
}

function Get-Gguf() {
    foreach ($g in $Ggufs) {
        $tree = Invoke-RestMethod "https://huggingface.co/api/models/$($g.repo)/tree/main"
        $entry = $tree | Where-Object { $_.path -like $g.pattern } | Select-Object -First 1
        if (-not $entry) { Write-Warning "$($g.repo): no file like $($g.pattern)"; continue }
        $dest = Join-Path $GgufDir "$($g.key).gguf"
        if ((Test-Path $dest) -and (Get-Item $dest).Length -eq $entry.lfs.size) { continue }
        Write-Step "download $($g.repo)/$($entry.path) ($([math]::Round($entry.size / 1MB, 1)) MB)"
        Get-Checked "https://huggingface.co/$($g.repo)/resolve/main/$($entry.path)" $dest $entry.lfs.oid
        Set-Content "$dest.source" "$($g.repo)/$($entry.path)" -Encoding UTF8
    }
}

function Get-MachineInfo() {
    $cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
    $os = Get-CimInstance Win32_OperatingSystem
    $gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object { @{ name = $_.Name; driver = $_.DriverVersion } })
    $battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
    $onAc = $true; if ($battery) { $onAc = ($battery.BatteryStatus -eq 2) }
    $llama = $null; $rel = Join-Path $LlamaDir "release.txt"; if (Test-Path $rel) { $llama = (Get-Content $rel -Raw).Trim() }
    $sources = @{}
    foreach ($g in $Ggufs) { $s = Join-Path $GgufDir "$($g.key).gguf.source"; if (Test-Path $s) { $sources[$g.key] = (Get-Content $s -Raw).Trim() } }
    return [ordered]@{
        cpu = $cpu.Name.Trim(); cores = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors
        ram_gb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
        os = "$($os.Caption) $($os.Version) (build $($os.BuildNumber))"
        gpus = $gpus; on_ac_power = $onAc; power_scheme = ((powercfg /getactivescheme) -join " ")
        llama_cpp = $llama; gguf = $sources; onnx_model = "onnx-community/embeddinggemma-2-ONNX"
        date = (Get-Date).ToString("s")
    }
}

function Stop-Server($proc) {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue; $proc.WaitForExit(5000) | Out-Null }
}

function Start-LlamaServer($run, $log) {
    $exe = Join-Path (Join-Path $LlamaDir $run.build) "llama-server.exe"
    if (-not (Test-Path $exe)) { throw "missing $exe - run with -Download" }
    $model = Join-Path $GgufDir "$($run.gguf).gguf"
    if (-not (Test-Path $model)) { throw "missing $model - run with -Download" }
    # Non-causal embedding: a whole input must fit one micro-batch (-ub); 16 inputs of
    # <=192 tokens fit -b 4096. One slot: the bench sends one request at a time.
    $srvArgs = @("-m", "`"$model`"", "--embedding", "--pooling", "mean", "--host", "127.0.0.1", "--port", $Port,
        "-c", "4096", "-b", "4096", "-ub", "4096", "-np", "1", "--no-webui")
    if ($run.threads) { $srvArgs += @("-t", $run.threads, "-tb", $run.threads) }
    if ($run.build -eq "cpu") { $srvArgs += @("-ngl", "0") } else { $srvArgs += @("-ngl", "999") }
    $proc = Start-Process -FilePath $exe -ArgumentList $srvArgs -PassThru -WindowStyle Hidden `
        -RedirectStandardError $log -RedirectStandardOutput "$log.out"
    $deadline = (Get-Date).AddSeconds(120)
    while ((Get-Date) -lt $deadline) {
        if ($proc.HasExited) { throw "llama-server exited ($($proc.ExitCode)); see $(Split-Path $log -Leaf)" }
        try {
            $r = Invoke-WebRequest "http://127.0.0.1:$Port/health" -UseBasicParsing -TimeoutSec 2
            if ($r.StatusCode -eq 200) { return $proc }
        } catch { }
        Start-Sleep -Milliseconds 500
    }
    Stop-Server $proc
    throw "llama-server did not become ready in 120 s"
}

# ---------------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $LlamaDir, $GgufDir | Out-Null
if ($Download) {
    if (-not (Test-Path (Join-Path $OrtCpuDir "onnxruntime.dll")) -or -not (Test-Path (Join-Path $ModelDir "onnx\model_q4.onnx"))) {
        Write-Step "ONNX Runtime + ONNX model (T006 cache)"
        & (Join-Path $Repo "scripts\t006\run-windows-bench.ps1") -Download -SkipBuild -Only "none" | Out-Null
    }
    Get-Llama
    Get-Gguf
}

$machine = Get-MachineInfo
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t014\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$machine | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $OutDir "machine.json") -Encoding UTF8
if (-not $machine.on_ac_power) { Write-Warning "running on battery: results are not comparable" }

# Thread sweep: 1, 2, 4, physical cores, logical CPUs (deduplicated).
$sweep = @(1, 2, 4, [int]$machine.cores, [int]$machine.threads) | Where-Object { $_ -le $machine.threads } | Sort-Object -Unique
$half = [math]::Max(1, [int]($machine.cores / 2))
$matrix = @()
foreach ($t in $sweep) { $matrix += @{ name = "ort-q4-t$t"; kind = "ort"; variant = "q4"; device = "cpu"; threads = $t } }
$matrix += @{ name = "ort-q8-t$half"; kind = "ort"; variant = "q8"; device = "cpu"; threads = $half }
$matrix += @{ name = "ort-dml-fp16"; kind = "ort-dml"; variant = "fp16"; device = "dml:high" }
foreach ($g in @("q8_0", "q4_k_xl")) {
    foreach ($t in @($half, [int]$machine.cores) | Sort-Object -Unique) {
        $matrix += @{ name = "llama-cpu-$g-t$t"; kind = "llama"; build = "cpu"; gguf = $g; threads = $t }
    }
    $matrix += @{ name = "llama-vulkan-$g"; kind = "llama"; build = "vulkan"; gguf = $g; threads = 2 }
    $matrix += @{ name = "llama-cuda-$g"; kind = "llama"; build = "cuda"; gguf = $g; threads = 2 }
}
if ($Only) { $keep = $Only.Split(","); $matrix = @($matrix | Where-Object { $keep -contains $_.name }) }

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
        if (Test-Path $src) { Copy-Item (Join-Path $src "*.dll") $dir -Force }
    }

    $iters = 30; $docs = 96; if ($Quick) { $iters = 10; $docs = 32 }
    $reference = Join-Path $Repo "fixtures\embedding\reference-eg2-onnx-fp32-d256.json"
    $corpus = Join-Path $Repo "fixtures\embedding\corpus.json"
    $common = @("embed", "--iterations", $iters, "--warmup", "5", "--docs", $docs, "--doc-words", "100",
        "--batch-sizes", "1,8,16", "--long-words", "0", "--reference", $reference, "--corpus", $corpus)
    $results = @()
    foreach ($run in $matrix) {
        $json = Join-Path $OutDir "$($run.name).json"
        $log = Join-Path $OutDir "$($run.name).log"
        Write-Step $run.name
        $started = Get-Date
        $server = $null; $code = 1; $err = $null
        try {
            $benchArgs = $common + @("--label", "$($machine.cpu) | $($run.name)", "--json", $json)
            $dir = Join-Path $runRoot "cpu"
            if ($run.kind -like "ort*") {
                if ($run.kind -eq "ort-dml") { $dir = Join-Path $runRoot "dml" }
                $benchArgs += @("--backend", "ort", "--ort-dylib", (Join-Path $dir "onnxruntime.dll"),
                    "--model-dir", $ModelDir, "--variant", $run.variant, "--device", $run.device)
                if ($run.threads) { $benchArgs += @("--threads", $run.threads) }
            } else {
                $server = Start-LlamaServer $run "$log.server"
                $target = "gpu"; if ($run.build -eq "cpu") { $target = "cpu" }
                $benchArgs += @("--backend", "llama-server", "--server", "127.0.0.1:$Port", "--server-target", $target,
                    "--variant", "gguf-$($run.gguf)", "--cpu-pid", $server.Id)
            }
            $ErrorActionPreference = "Continue"
            & (Join-Path $dir "lumen-bench.exe") @benchArgs 2>&1 |
                ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
                Where-Object { $_ -and $_.Trim() } | Set-Content $log -Encoding UTF8
            $code = $LASTEXITCODE
            $ErrorActionPreference = "Stop"
        } catch {
            $ErrorActionPreference = "Stop"
            $err = $_.Exception.Message
        } finally {
            Stop-Server $server
        }
        $entry = [ordered]@{ name = $run.name; ok = ($code -eq 0 -and -not $err); seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1) }
        if (-not $entry.ok) {
            if (-not $err -and (Test-Path $log)) { $err = (Get-Content $log | Where-Object { $_ -match "^error:" } | Select-Object -First 1) }
            if (-not $err) { $err = "exit code $code (see $($run.name).log)" }
            $entry.error = $err -replace "\|", "/"
            Write-Warning "$($run.name) failed: $($entry.error)"
        }
        $results += $entry
    }
    $runsFile = Join-Path $OutDir "runs.json"
    if ($Only -and (Test-Path $runsFile)) {
        $parsed = Get-Content $runsFile -Raw | ConvertFrom-Json
        $previous = @($parsed | ForEach-Object { $_ } | Where-Object { $keep -notcontains $_.name })
        $results = @($previous) + @($results)
    }
    $results | ConvertTo-Json -Depth 4 | Set-Content $runsFile -Encoding UTF8

    # Budget (docs/PERFORMANCE.md §9, proposed): >= 8 chunks/s at <= 50 % of the machine.
    $lines = @("# T014 indexing throughput - $($machine.cpu)", "",
        "$($machine.os) | $($machine.cores) cores / $($machine.threads) threads | $($machine.ram_gb) GB | GPUs: $(($machine.gpus | ForEach-Object { $_.name }) -join ', ') | llama.cpp $($machine.llama_cpp) | AC: $($machine.on_ac_power)", "",
        "~128-token chunks (100 words). CPU = process doing the work (lumen-bench or llama-server).", "",
        "| run | chunks/s b1 | b8 | b16 | cores busy (b8) | machine CPU % (b8) | chunks/s per core | query p50 ms | min cos | budget | status |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|")
    foreach ($r in $results) {
        $f = Join-Path $OutDir "$($r.name).json"
        if ($r.ok -and (Test-Path $f)) {
            $j = Get-Content $f -Raw | ConvertFrom-Json
            $tp = @{}; foreach ($t in $j.throughput) { $tp[[int]$t.batch_size] = $t }
            $b8 = $tp[8]
            $cores = ""; $pct = ""; $perCore = ""; $budget = ""
            if ($b8.cpu) {
                $cores = "{0:N2}" -f $b8.cpu.cores; $pct = "{0:N0}" -f $b8.cpu.machine_percent
                if ($b8.cpu.cores -gt 0) { $perCore = "{0:N1}" -f ($b8.items_per_s / $b8.cpu.cores) }
                if ($b8.items_per_s -ge 8 -and $b8.cpu.machine_percent -le 50) { $budget = "meets" } else { $budget = "misses" }
            }
            $cos = ""; if ($j.fidelity) { $cos = "{0:N4}" -f $j.fidelity.min_cosine }
            $lines += ("| {0} | {1:N1} | {2:N1} | {3:N1} | {4} | {5} | {6} | {7:N1} | {8} | {9} | ok |" -f $r.name,
                $tp[1].items_per_s, $b8.items_per_s, $tp[16].items_per_s, $cores, $pct, $perCore,
                $j.query.warm.p50_ms, $cos, $budget)
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
