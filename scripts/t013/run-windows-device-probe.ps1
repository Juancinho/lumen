<#
.SYNOPSIS
  T013 device policy on this Windows PC: probe CPU and the discrete GPU, then show what
  Lumen's policy would choose in each power/profile situation.

.DESCRIPTION
  Uses the runtime DLLs and model downloaded by T006 (.cache\t006; run
  scripts\t006\run-windows-bench.ps1 -Download once if missing). For each device it runs
  `lumen-bench probe` in its own process (q4 weights, the default index space):
    - cpu        : reference vectors saved for comparison
    - dml:high   : DirectML on the high-performance GPU, with node placement, compared with
                   the CPU vectors; its dedicated GPU memory is sampled while it runs
    - dml:low    : only with -IncludeIntegrated (the integrated GPU hung the device in T006)
  Then `lumen-bench device-policy` prints the decision for 7 scenarios.
  Takes ~2-4 minutes. Keep the PC plugged in.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t013\run-windows-device-probe.ps1
#>
[CmdletBinding()]
param(
    [string]$CacheDir = "",
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [switch]$IncludeIntegrated
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $CacheDir) { $CacheDir = Join-Path $Repo ".cache\t006" }
$ModelDir = Join-Path $CacheDir "embeddinggemma-2-ONNX"
$OrtCpuDir = Join-Path $CacheDir "ort-cpu"
$OrtDmlDir = Join-Path $CacheDir "ort-dml"
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

foreach ($p in @($ModelDir, (Join-Path $OrtCpuDir "onnxruntime.dll"), (Join-Path $OrtDmlDir "onnxruntime.dll"))) {
    if (-not (Test-Path $p)) { throw "missing $p - run scripts\t006\run-windows-bench.ps1 -Download first" }
}

$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t013\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# GPUs: name, driver, total dedicated memory (registry qwMemorySize is 64-bit; WMI caps at 4 GB).
$gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
    $name = $_.Name
    $integrated = ($name -match "Radeon\(TM\) Graphics|Radeon Graphics|Vega|UHD|Iris|Intel\(R\) HD|Intel\(R\) Graphics|Arc\(TM\) Graphics")
    $mem = $null
    Get-ChildItem "HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}" -ErrorAction SilentlyContinue |
        ForEach-Object {
            $props = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
            if ($props.DriverDesc -eq $name -and $props.'HardwareInformation.qwMemorySize') {
                $mem = [math]::Round([double]$props.'HardwareInformation.qwMemorySize' / 1MB)
            }
        }
    if (-not $mem -and $_.AdapterRAM) { $mem = [math]::Round([double]$_.AdapterRAM / 1MB) }
    [ordered]@{ name = $name; driver = $_.DriverVersion; integrated = $integrated; memory_mib = $mem }
})
$discrete = $gpus | Where-Object { -not $_.integrated } | Sort-Object { $_.memory_mib } -Descending | Select-Object -First 1
$integratedGpu = $gpus | Where-Object { $_.integrated } | Select-Object -First 1
$gpus | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutDir "gpus.json") -Encoding UTF8
$gpus | ForEach-Object { Write-Host ("  GPU: {0} (driver {1}, {2} MiB, integrated={3})" -f $_.name, $_.driver, $_.memory_mib, $_.integrated) }

# Peak dedicated GPU memory of one process (vendor-neutral perf counters via CIM; English
# class names work on localized Windows, unlike Get-Counter paths).
function Get-GpuDedicatedMb([int]$processId) {
    $rows = Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like "pid_$($processId)_*" }
    if (-not $rows) { return 0 }
    return [math]::Round((($rows | Measure-Object -Property DedicatedUsage -Sum).Sum) / 1MB)
}

function Write-JsonNoBom($path, $obj) {
    [System.IO.File]::WriteAllText($path, ($obj | ConvertTo-Json -Depth 8))
}

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench --features directml"
        cargo build --release -p lumen-bench --features directml
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $exe = Join-Path $Repo "target\release\lumen-bench.exe"
    $runRoot = Join-Path $CacheDir "run-t013"
    foreach ($kind in @("cpu", "dml")) {
        $dir = Join-Path $runRoot $kind
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
        Copy-Item $exe $dir -Force
        $src = $OrtCpuDir; if ($kind -eq "dml") { $src = $OrtDmlDir }
        Copy-Item (Join-Path $src "*.dll") $dir -Force
    }
    $cpuVectors = Join-Path $OutDir "cpu-vectors.json"

    $runs = @(@{ name = "cpu"; kind = "cpu"; device = "cpu"; extra = @("--save-vectors", $cpuVectors); gpu = $null })
    if ($discrete) {
        $runs += @{ name = "dml-high"; kind = "dml"; device = "dml:high"; gpu = $discrete
            extra = @("--placement", "--cpu-vectors", $cpuVectors, "--runtime-key", "onnxruntime-directml-1.24.4/$($discrete.driver)") }
    } else { Write-Host "  no discrete GPU found: CPU only" }
    if ($IncludeIntegrated -and $integratedGpu) {
        $runs += @{ name = "dml-low"; kind = "dml"; device = "dml:low"; gpu = $integratedGpu
            extra = @("--placement", "--integrated", "--cpu-vectors", $cpuVectors, "--runtime-key", "onnxruntime-directml-1.24.4/$($integratedGpu.driver)") }
    }

    $probeFiles = @()
    foreach ($run in $runs) {
        $dir = Join-Path $runRoot $run.kind
        $json = Join-Path $OutDir "probe-$($run.name).json"
        $benchArgs = @("probe", "--backend", "ort", "--ort-dylib", (Join-Path $dir "onnxruntime.dll"),
            "--model-dir", $ModelDir, "--variant", "q4", "--device", $run.device,
            "--label", "$($run.name)", "--json", $json) + $run.extra
        $quoted = ($benchArgs | ForEach-Object { if ("$_" -match '\s') { '"' + $_ + '"' } else { "$_" } }) -join " "
        Write-Step "probe $($run.name)"
        $errFile = Join-Path $OutDir "probe-$($run.name).stderr.log"
        $proc = Start-Process -FilePath (Join-Path $dir "lumen-bench.exe") -ArgumentList $quoted -NoNewWindow -PassThru `
            -RedirectStandardError $errFile -RedirectStandardOutput (Join-Path $OutDir "probe-$($run.name).stdout.log")
        $null = $proc.Handle   # PS 5.1: keep the handle so ExitCode is available later
        $peak = 0
        while (-not $proc.HasExited) {
            if ($run.gpu) { $m = Get-GpuDedicatedMb $proc.Id; if ($m -gt $peak) { $peak = $m } }
            Start-Sleep -Milliseconds 300
        }
        $proc.WaitForExit()
        Get-Content $errFile | Where-Object { $_ -match "^probe|^  space|^error" } | ForEach-Object { Write-Host $_ }
        if ($proc.ExitCode -ne 0 -or -not (Test-Path $json)) { Write-Warning "$($run.name) exited with $($proc.ExitCode)"; continue }
        if ($run.gpu) {
            $j = Get-Content $json -Raw | ConvertFrom-Json
            if ($j.metrics) {
                $j.metrics.device_memory_mib = [double]$peak
                $j.metrics.device_memory_total_mib = [double]$run.gpu.memory_mib
                Write-JsonNoBom $json $j
                Write-Host "  peak dedicated GPU memory: $peak MiB of $($run.gpu.memory_mib) MiB"
            }
        }
        $probeFiles += $json
    }
    if (Test-Path $cpuVectors) { Remove-Item $cpuVectors -ErrorAction SilentlyContinue }

    Write-Step "device-policy"
    $policyArgs = @("device-policy", "--label", "$($env:COMPUTERNAME)", "--json", (Join-Path $OutDir "policy.json"))
    foreach ($f in $probeFiles) { $policyArgs += @("--probe", $f) }
    $ErrorActionPreference = "Continue"
    & $exe @policyArgs 2>&1 | ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
        Where-Object { $_ -and $_.Trim() } | Tee-Object -FilePath (Join-Path $OutDir "policy.log")
    $ErrorActionPreference = "Stop"
    Write-Step "done: $OutDir"
}
finally {
    Pop-Location
}
