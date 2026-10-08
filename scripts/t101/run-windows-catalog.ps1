<#
.SYNOPSIS
  T101 app/file catalog on this Windows PC: Start-menu apps, inventory -> SQLite speed and
  keystroke name lookups.

.DESCRIPTION
  1. Runs the lumen-windows / lumen-catalog tests natively (AppsFolder enumeration, paths).
  2. `lumen-bench catalog` over Documents, Desktop, Downloads, Pictures, OneDrive (if present)
     plus the Start-menu application list; prints the top results for a few queries.
  The JSON report holds counts and timings only (no file or app names). Read-only on your
  folders; the temporary database is deleted. Takes ~1-3 minutes.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t101\run-windows-catalog.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [string[]]$Roots = @(),
    [string[]]$Show = @("calc", "spotify", "visual", "config", "notas"),
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t101\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "cargo test -p lumen-windows -p lumen-catalog (native Windows)"
        $ErrorActionPreference = "Continue"
        $out = cargo test -p lumen-windows -p lumen-catalog 2>&1 | ForEach-Object { "$_" }
        $code = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        $out | Where-Object { $_ -match "^test |test result|panicked|error" } | ForEach-Object { Write-Host $_ }
        if ($code -ne 0) { Write-Warning "tests FAILED" }
        Write-Step "cargo build --release -p lumen-bench"
        cargo build --release -p lumen-bench
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    if (-not $Roots) {
        $Roots = @("Documents", "Desktop", "Downloads", "Pictures", "OneDrive") |
            ForEach-Object { Join-Path $env:USERPROFILE $_ } | Where-Object { Test-Path $_ }
    }
    $benchArgs = @("catalog", "--apps", "--sample", "300", "--label", "user folders + apps", "--json", (Join-Path $OutDir "catalog.json"))
    foreach ($r in $Roots) { $benchArgs += @("--root", $r) }
    foreach ($q in $Show) { $benchArgs += @("--show", $q) }
    Write-Step "lumen-bench catalog ($($Roots.Count) folders + apps)"
    $ErrorActionPreference = "Continue"
    & (Join-Path $Repo "target\release\lumen-bench.exe") @benchArgs 2>&1 |
        ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
        Where-Object { $_ -and $_.Trim() } | ForEach-Object { Write-Host $_ }
    $ErrorActionPreference = "Stop"
    Write-Step "done: $OutDir"
}
finally {
    Pop-Location
}
