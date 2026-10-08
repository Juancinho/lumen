<#
.SYNOPSIS
  T012 WebView lifecycle/RAM spike on this Windows PC: start-up, show latency and memory of
  the resident overlay for each hidden-state WebView mode.

.DESCRIPTION
  Builds the UI and lumen.exe (release), then for each mode in LUMEN_WEBVIEW_HIDDEN
  (keep, invisible, low-memory, suspend):
    1. starts `lumen.exe --background` with timing diagnostics (LUMEN_DIAG_LOG);
    2. waits until the UI reports ready, then 8 s, and samples memory (never shown);
    3. 20 quick show/hide cycles via `lumen.exe --show` / `--hide` (single instance);
    4. 3 shows after 8 s hidden (long enough for `suspend` to suspend the WebView);
    5. samples memory while visible and after 8 s hidden; quits with `lumen.exe --quit`.
  Memory = private working set (and commit) of lumen.exe plus its WebView2 process tree.
  Show latency = shell receives the request -> UI reports the next frame painted (double
  requestAnimationFrame); it excludes hotkey delivery.

  WARNING: stops any running Lumen first. The overlay will flash on screen ~25 times per
  mode; do not type meanwhile. Takes ~4 minutes.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t012\run-windows-webview.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [string[]]$Modes = @("keep", "invisible", "low-memory", "suspend"),
    [int]$Cycles = 20
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t012\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$exe = Join-Path $Repo "target\release\lumen.exe"

function Get-UnixMs { [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }

# Reads a file another process keeps open for writing.
function Read-Shared([string]$path) {
    if (-not (Test-Path $path)) { return @() }
    $fs = New-Object System.IO.FileStream($path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    try {
        $reader = New-Object System.IO.StreamReader($fs)
        $lines = @()
        while ($null -ne ($line = $reader.ReadLine())) { $lines += $line }
        return $lines
    } finally { $fs.Dispose() }
}

# Diag lines: "<event> <value_ms> <unix_ms>".
function Get-Events([string]$path, [string]$event) {
    @(Read-Shared $path | Where-Object { $_ -like "$event *" } | ForEach-Object {
        $p = $_.Split(" "); [pscustomobject]@{ value = [double]::Parse($p[1], [Globalization.CultureInfo]::InvariantCulture); unix = [int64]$p[2] }
    })
}

function Get-Percentile([double[]]$values, [double]$p) {
    if (-not $values -or $values.Count -eq 0) { return $null }
    $sorted = $values | Sort-Object
    $rank = [math]::Ceiling($p / 100 * $sorted.Count)
    if ($rank -lt 1) { $rank = 1 }
    return [math]::Round($sorted[$rank - 1], 2)
}

# Private working set / commit (MiB) of lumen.exe and its WebView2 process tree.
function Get-TreeMemory([int]$rootId) {
    $all = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name, PrivatePageCount)
    $tree = New-Object System.Collections.Generic.List[object]
    $queue = New-Object System.Collections.Generic.Queue[int]
    $queue.Enqueue($rootId)
    while ($queue.Count -gt 0) {
        $id = $queue.Dequeue()
        foreach ($p in $all) {
            if ($p.ProcessId -eq $id -and -not ($tree | Where-Object { $_.ProcessId -eq $id })) { $tree.Add($p) }
            if ($p.ParentProcessId -eq $id -and $p.ProcessId -ne $id) { $queue.Enqueue([int]$p.ProcessId) }
        }
    }
    $perf = @{}
    Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Property IDProcess, WorkingSetPrivate |
        ForEach-Object { $perf[[int]$_.IDProcess] = [double]$_.WorkingSetPrivate }
    $shell = 0.0; $web = 0.0; $commit = 0.0; $count = 0
    foreach ($p in $tree) {
        $ws = 0.0; if ($perf.ContainsKey([int]$p.ProcessId)) { $ws = $perf[[int]$p.ProcessId] }
        if ($p.ProcessId -eq $rootId) { $shell += $ws } else { $web += $ws; $count++ }
        $commit += [double]$p.PrivatePageCount
    }
    [ordered]@{
        shell_private_ws_mib = [math]::Round($shell / 1MB, 1)
        webview_private_ws_mib = [math]::Round($web / 1MB, 1)
        total_private_ws_mib = [math]::Round(($shell + $web) / 1MB, 1)
        total_commit_mib = [math]::Round($commit / 1MB, 1)
        webview_processes = $count
    }
}

function Send-Lumen([string]$flag) {
    Start-Process -FilePath $exe -ArgumentList $flag -Wait -WindowStyle Hidden
}

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "npm run build (apps/desktop)"
        Push-Location (Join-Path $Repo "apps\desktop")
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "frontend build failed" }
        Pop-Location
        # tauri/custom-protocol embeds apps/desktop/dist; without it the exe loads the dev
        # server URL and the UI never starts (docs/DEVELOPMENT.md, profiles).
        Write-Step "cargo build --release -p lumen-desktop --features tauri/custom-protocol"
        cargo build --release -p lumen-desktop --features tauri/custom-protocol
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $running = Get-Process -Name lumen -ErrorAction SilentlyContinue
    if ($running) { Write-Host "  stopping running Lumen"; $running | Stop-Process -Force; Start-Sleep -Seconds 1 }

    $results = @()
    foreach ($mode in $Modes) {
        Write-Step "mode $mode"
        $log = Join-Path $OutDir "diag-$mode.log"
        if (Test-Path $log) { Remove-Item $log }
        $env:LUMEN_DIAG_LOG = $log
        $env:LUMEN_WEBVIEW_HIDDEN = $mode
        $launchedAt = Get-UnixMs
        $proc = Start-Process -FilePath $exe -ArgumentList "--background" -PassThru
        $deadline = (Get-Date).AddSeconds(30)
        while (-not (Get-Events $log "ready_ms") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
        $ready = Get-Events $log "ready_ms" | Select-Object -First 1
        if (-not $ready) {
            Write-Warning "  no ready event in 30 s (UI did not start: was lumen.exe built with tauri/custom-protocol?)"
            $proc | Stop-Process -Force; continue
        }
        $startupWall = $ready.unix - $launchedAt
        Write-Host ("  ready: {0:N0} ms after launch ({1:N0} ms inside the process)" -f $startupWall, $ready.value)

        Start-Sleep -Seconds 8
        $memStart = Get-TreeMemory $proc.Id

        for ($i = 0; $i -lt $Cycles; $i++) {
            Send-Lumen "--show"; Start-Sleep -Milliseconds 500
            Send-Lumen "--hide"; Start-Sleep -Milliseconds 300
        }
        $quick = @(Get-Events $log "show_to_paint_ms" | ForEach-Object { $_.value })

        $long = @()
        for ($i = 0; $i -lt 3; $i++) {
            Start-Sleep -Seconds 8
            $before = (Get-Events $log "show_to_paint_ms").Count
            Send-Lumen "--show"; Start-Sleep -Milliseconds 700
            $after = @(Get-Events $log "show_to_paint_ms")
            if ($after.Count -gt $before) { $long += $after[$after.Count - 1].value }
            if ($i -eq 2) { $memVisible = Get-TreeMemory $proc.Id }
            Send-Lumen "--hide"
        }
        Start-Sleep -Seconds 8
        $memHidden = Get-TreeMemory $proc.Id
        $suspended = (Get-Events $log "webview_suspended_ms").Count
        $refused = (Get-Events $log "webview_suspend_refused_ms").Count

        Send-Lumen "--quit"
        if (-not $proc.WaitForExit(10000)) { $proc | Stop-Process -Force }

        $r = [ordered]@{
            mode = $mode
            startup_to_ready_wall_ms = $startupWall
            startup_to_ready_in_process_ms = [math]::Round($ready.value, 1)
            show_to_paint_quick = [ordered]@{ n = $quick.Count; p50_ms = (Get-Percentile $quick 50); p95_ms = (Get-Percentile $quick 95); max_ms = (Get-Percentile $quick 100) }
            show_to_paint_after_8s_hidden_ms = $long
            suspended = $suspended; suspend_refused = $refused
            memory_hidden_never_shown = $memStart
            memory_visible = $memVisible
            memory_hidden_after_use = $memHidden
        }
        $results += $r
        Write-Host ("  show->paint p50/p95 {0}/{1} ms (n={2}); after 8 s hidden: {3} ms" -f $r.show_to_paint_quick.p50_ms, $r.show_to_paint_quick.p95_ms, $quick.Count, ($long -join ", "))
        Write-Host ("  private WS MiB: never shown {0} | visible {1} | hidden after use {2} (webview procs {3})" -f $memStart.total_private_ws_mib, $memVisible.total_private_ws_mib, $memHidden.total_private_ws_mib, $memHidden.webview_processes)
        if ($mode -eq "suspend") { Write-Host "  suspended $suspended times, refused $refused" }
    }
    Remove-Item Env:\LUMEN_DIAG_LOG -ErrorAction SilentlyContinue
    Remove-Item Env:\LUMEN_WEBVIEW_HIDDEN -ErrorAction SilentlyContinue

    $cpu = (Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()
    $os = Get-CimInstance Win32_OperatingSystem
    $wv2 = (Get-ItemProperty "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue).pv
    $report = [ordered]@{
        schema_version = 1; kind = "webview-lifecycle"
        machine = [ordered]@{ cpu = $cpu; os = "$($os.Caption) $($os.Version)"; ram_gb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1); webview2_runtime = $wv2 }
        cycles = $Cycles; results = $results
    }
    [System.IO.File]::WriteAllText((Join-Path $OutDir "webview-lifecycle.json"), ($report | ConvertTo-Json -Depth 6))

    Write-Step "summary"
    $fmt = "{0,-11} {1,8} {2,14} {3,12} {4,12} {5,10} {6,12}"
    Write-Host ($fmt -f "mode", "ready ms", "show p50/p95", "after 8s ms", "never shown", "visible", "hidden used")
    foreach ($r in $results) {
        $longMed = Get-Percentile ([double[]]$r.show_to_paint_after_8s_hidden_ms) 50
        Write-Host ($fmt -f $r.mode, $r.startup_to_ready_wall_ms, "$($r.show_to_paint_quick.p50_ms)/$($r.show_to_paint_quick.p95_ms)", $longMed, $r.memory_hidden_never_shown.total_private_ws_mib, $r.memory_visible.total_private_ws_mib, $r.memory_hidden_after_use.total_private_ws_mib)
    }
    Write-Host "  (memory = private working set MiB, lumen.exe + WebView2 processes)"
    Write-Step "done: $OutDir"
}
finally {
    Pop-Location
}
