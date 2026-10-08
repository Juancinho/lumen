<#
.SYNOPSIS
  T111 scale check: catalog a whole drive the way Lumen would (default exclusions on) and
  measure sync time, database size and keystroke latency.

.DESCRIPTION
  Builds lumen-bench (release) and runs `lumen-bench catalog --app-defaults` over -Drive
  (default D:\). When the drive is the system drive, the system folders Lumen pre-excludes
  (Windows, Program Files, Program Files (x86), ProgramData, %LOCALAPPDATA%\Temp) are
  excluded too. The JSON report holds counts and timings only (exclusions by rule kind,
  never paths); it goes to docs\benchmarks\t111\<date>-<pc>\.

  A whole drive can hold millions of entries: expect several minutes for the first sync.
  Nothing in your Lumen database is touched (the bench uses a temporary database).

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t111\run-windows-locations.ps1 -Drive D:\
#>
[CmdletBinding()]
param(
    [string]$Drive = "D:\",
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [int]$Sample = 300
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t111\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$bench = Join-Path $Repo "target\release\lumen-bench.exe"

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench"
        cargo build --release -p lumen-bench
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    if (-not (Test-Path $Drive)) { throw "$Drive not found" }
    $args = @("catalog", "--root", $Drive, "--app-defaults", "--apps", "--sample", "$Sample")
    $system = "$($env:SystemDrive)\"
    if ($Drive.TrimEnd('\').ToLower() -eq $system.TrimEnd('\').ToLower()) {
        foreach ($d in @("Windows", "Program Files", "Program Files (x86)", "ProgramData")) {
            $args += @("--exclude", (Join-Path $system $d))
        }
        $args += @("--exclude", (Join-Path $env:LOCALAPPDATA "Temp"))
    }
    $label = "whole drive $($Drive.TrimEnd('\')) with app defaults"
    $name = "catalog-" + ($Drive.TrimEnd(':\').ToLower()) + ".json"
    $args += @("--label", $label, "--json", (Join-Path $OutDir $name))
    Write-Step "lumen-bench $($args -join ' ')"
    & $bench @args
    if ($LASTEXITCODE -ne 0) { throw "bench failed" }
    Write-Step "done: $OutDir"
    Write-Host "  Compare keystroke p95 with T102 (7.9 ms at 247k, sandbox) and T101 (5.3 ms on joao-pc)."
}
finally {
    Pop-Location
}
