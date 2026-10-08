<#
.SYNOPSIS
  T004 window material spike on this Windows PC: which material Lumen applies, show
  latency and DWM GPU load per material, real composited contrast, and screenshots.

.DESCRIPTION
  Builds the UI and lumen.exe (release, custom protocol), then for each material in
  LUMEN_MATERIAL (auto, acrylic, mica, solid):
    1. starts `lumen.exe --background` with timing diagnostics (LUMEN_DIAG_LOG);
    2. 15 quick show/hide cycles via `lumen.exe --show` / `--hide`: show -> painted frame
       latency, and the per-show material check cost;
    3. shows the overlay, samples DWM 3D-engine GPU use for 3 s (visible) and 3 s after
       hiding (baseline);
    4. while visible, takes a screenshot of the overlay (plus its shadow) and samples the
       composited background colour at three empty points of the search bar, then computes
       the WCAG contrast of Lumen's text tokens against what is really on screen.

  The JSON report (docs\benchmarks\t004\<date>-<pc>\material.json) holds numbers only.
  Screenshots go to target\t004\ (git-ignored) because they show whatever is behind the
  overlay: look at them yourself, share only if you want to.

  For a meaningful contrast/legibility check, put a busy, bright window (e.g. a white web
  page) behind the centre-top of the screen before running, then run it again with a dark
  one. Run once in light and once in dark mode if you can.

  WARNING: stops any running Lumen first. The overlay flashes on screen ~20 times per
  material; do not type meanwhile. Takes ~2 minutes.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t004\run-windows-material.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [switch]$SkipBuild,
    [string[]]$Materials = @("auto", "acrylic", "mica", "solid"),
    [int]$Cycles = 15
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t004\$tag" }
$ShotDir = Join-Path $Repo "target\t004"
New-Item -ItemType Directory -Force -Path $OutDir, $ShotDir | Out-Null
$exe = Join-Path $Repo "target\release\lumen.exe"

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class LumenWin {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT rect, int size);
    public static void DpiAware() { SetProcessDPIAware(); }
    // Visible top-level window of `pid`, in physical pixels (DWMWA_EXTENDED_FRAME_BOUNDS).
    public static int[] VisibleRect(uint pid) {
        int[] found = null;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p != pid || !IsWindowVisible(h)) return true;
            RECT r;
            if (DwmGetWindowAttribute(h, 9, out r, Marshal.SizeOf(typeof(RECT))) != 0) return true;
            if (r.Right - r.Left < 200) return true;
            found = new int[] { r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top };
            return false;
        }, IntPtr.Zero);
        return found;
    }
}
"@
[LumenWin]::DpiAware()

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
function Get-Events([string]$path, [string]$like) {
    @(Read-Shared $path | Where-Object { $_ -like "$like *" } | ForEach-Object {
        $p = $_.Split(" "); [pscustomobject]@{ name = $p[0]; value = [double]::Parse($p[1], [Globalization.CultureInfo]::InvariantCulture) }
    })
}

function Get-Percentile([double[]]$values, [double]$p) {
    if (-not $values -or $values.Count -eq 0) { return $null }
    $sorted = $values | Sort-Object
    $rank = [math]::Ceiling($p / 100 * $sorted.Count)
    if ($rank -lt 1) { $rank = 1 }
    return [math]::Round($sorted[$rank - 1], 3)
}

function Send-Lumen([string]$flag) { Start-Process -FilePath $exe -ArgumentList $flag -Wait -WindowStyle Hidden }

# Mean DWM 3D-engine utilisation (%) over $seconds; $null when the counter is unavailable.
function Get-DwmGpu([int]$seconds) {
    try {
        $dwm = (Get-Process -Name dwm -ErrorAction Stop | Select-Object -First 1).Id
        $samples = Get-Counter -Counter "\GPU Engine(pid_$($dwm)_*engtype_3D)\Utilization Percentage" -SampleInterval 1 -MaxSamples $seconds -ErrorAction Stop
        $means = foreach ($s in $samples) { ($s.CounterSamples | Measure-Object -Property CookedValue -Sum).Sum }
        return [math]::Round(($means | Measure-Object -Average).Average, 2)
    } catch { return $null }
}

function Get-Luminance([int[]]$rgb) {
    $l = foreach ($c in $rgb) { $s = $c / 255.0; if ($s -le 0.04045) { $s / 12.92 } else { [math]::Pow(($s + 0.055) / 1.055, 2.4) } }
    0.2126 * $l[0] + 0.7152 * $l[1] + 0.0722 * $l[2]
}
function Get-Contrast([int[]]$a, [int[]]$b) {
    $la = Get-Luminance $a; $lb = Get-Luminance $b
    [math]::Round(([math]::Max($la, $lb) + 0.05) / ([math]::Min($la, $lb) + 0.05), 2)
}

$light = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -ErrorAction SilentlyContinue).AppsUseLightTheme -ne 0
$transparency = (Get-ItemProperty "HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" -ErrorAction SilentlyContinue).EnableTransparency -ne 0
$theme = if ($light) { "light" } else { "dark" }
# Must match src/design/material.css.
$textPrimary = if ($light) { @(0x1a, 0x1a, 0x1a) } else { @(255, 255, 255) }
$textSecondary = if ($light) { @(0x5c, 0x5c, 0x5c) } else { @(0xb0, 0xb0, 0xb0) }

Push-Location $Repo
try {
    if (-not $SkipBuild) {
        Write-Step "npm run build (apps/desktop)"
        Push-Location (Join-Path $Repo "apps\desktop")
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "frontend build failed" }
        Pop-Location
        Write-Step "cargo build --release -p lumen-desktop --features tauri/custom-protocol"
        cargo build --release -p lumen-desktop --features tauri/custom-protocol
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
    }
    $running = Get-Process -Name lumen -ErrorAction SilentlyContinue
    if ($running) { Write-Host "  stopping running Lumen"; $running | Stop-Process -Force; Start-Sleep -Seconds 1 }

    $results = @()
    foreach ($material in $Materials) {
        Write-Step "material $material ($theme mode)"
        $log = Join-Path $ShotDir "diag-$material.log"
        if (Test-Path $log) { Remove-Item $log }
        $env:LUMEN_DIAG_LOG = $log
        $env:LUMEN_MATERIAL = $material
        $proc = Start-Process -FilePath $exe -ArgumentList "--background" -PassThru
        $deadline = (Get-Date).AddSeconds(30)
        while (-not (Get-Events $log "ready_ms") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 50 }
        if (-not (Get-Events $log "ready_ms")) {
            Write-Warning "  no ready event in 30 s (was lumen.exe built with --features tauri/custom-protocol?)"
            $proc | Stop-Process -Force; continue
        }
        $applied = @(Get-Events $log "material_applied_*" | ForEach-Object { $_.name -replace "^material_applied_", "" }) | Select-Object -Last 1

        for ($i = 0; $i -lt $Cycles; $i++) {
            Send-Lumen "--show"; Start-Sleep -Milliseconds 450
            Send-Lumen "--hide"; Start-Sleep -Milliseconds 250
        }
        $paint = @(Get-Events $log "show_to_paint_ms" | ForEach-Object { $_.value })
        $check = @(Get-Events $log "material_check_ms" | ForEach-Object { $_.value })

        Send-Lumen "--show"; Start-Sleep -Milliseconds 800
        $rect = [LumenWin]::VisibleRect([uint32]$proc.Id)
        $samples = @(); $shot = $null
        if ($rect) {
            $m = 48
            $bmp = New-Object System.Drawing.Bitmap ($rect[2] + 2 * $m), ($rect[3] + 2 * $m)
            $g = [System.Drawing.Graphics]::FromImage($bmp)
            $g.CopyFromScreen($rect[0] - $m, $rect[1] - $m, 0, 0, $bmp.Size)
            $g.Dispose()
            $shot = Join-Path $ShotDir "$material-$theme.png"
            $bmp.Save($shot, [System.Drawing.Imaging.ImageFormat]::Png)
            # Empty points of the search bar (right half; the placeholder sits on the left).
            foreach ($fx in 0.6, 0.75, 0.9) {
                $c = $bmp.GetPixel($m + [int]($rect[2] * $fx), $m + [int]($rect[3] / 2))
                $px = @([int]$c.R, [int]$c.G, [int]$c.B)
                $samples += [ordered]@{
                    rgb = $px
                    primary_contrast = (Get-Contrast $textPrimary $px)
                    secondary_contrast = (Get-Contrast $textSecondary $px)
                }
            }
            $bmp.Dispose()
        } else { Write-Warning "  overlay window not found for the screenshot" }
        $gpuVisible = Get-DwmGpu 3
        Send-Lumen "--hide"; Start-Sleep -Milliseconds 300
        $gpuHidden = Get-DwmGpu 3

        Send-Lumen "--quit"
        if (-not $proc.WaitForExit(10000)) { $proc | Stop-Process -Force }

        $minPrimary = ($samples | ForEach-Object { $_.primary_contrast } | Measure-Object -Minimum).Minimum
        $minSecondary = ($samples | ForEach-Object { $_.secondary_contrast } | Measure-Object -Minimum).Minimum
        $r = [ordered]@{
            requested = $material
            applied = $applied
            show_to_paint = [ordered]@{ n = $paint.Count; p50_ms = (Get-Percentile $paint 50); p95_ms = (Get-Percentile $paint 95); max_ms = (Get-Percentile $paint 100) }
            material_check = [ordered]@{ n = $check.Count; p50_ms = (Get-Percentile $check 50); p95_ms = (Get-Percentile $check 95) }
            dwm_gpu_3d_percent_visible = $gpuVisible
            dwm_gpu_3d_percent_hidden = $gpuHidden
            window_px = if ($rect) { @($rect[2], $rect[3]) } else { $null }
            background_samples = $samples
            min_primary_contrast = $minPrimary
            min_secondary_contrast = $minSecondary
        }
        $results += $r
        Write-Host ("  applied {0} | show->paint p50/p95 {1}/{2} ms | check p95 {3} ms | DWM GPU visible/hidden {4}/{5} % | contrast primary {6} secondary {7}" -f `
            $applied, $r.show_to_paint.p50_ms, $r.show_to_paint.p95_ms, $r.material_check.p95_ms, $gpuVisible, $gpuHidden, $minPrimary, $minSecondary)
    }
    Remove-Item Env:\LUMEN_DIAG_LOG -ErrorAction SilentlyContinue
    Remove-Item Env:\LUMEN_MATERIAL -ErrorAction SilentlyContinue

    $os = Get-CimInstance Win32_OperatingSystem
    $gpu = (Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name) -join "; "
    $report = [ordered]@{
        schema_version = 1; kind = "window-material"
        machine = [ordered]@{ os = "$($os.Caption) $($os.Version)"; build = [int]$os.BuildNumber; gpu = $gpu }
        theme = $theme; transparency_effects = $transparency; cycles = $Cycles; results = $results
    }
    [System.IO.File]::WriteAllText((Join-Path $OutDir "material-$theme.json"), ($report | ConvertTo-Json -Depth 6))

    Write-Step "summary ($theme mode, transparency effects $transparency, build $($os.BuildNumber))"
    $fmt = "{0,-9} {1,-30} {2,14} {3,10} {4,12} {5,10}"
    Write-Host ($fmt -f "asked", "applied", "show p50/p95", "check p95", "GPU vis/hid", "contrast")
    foreach ($r in $results) {
        Write-Host ($fmt -f $r.requested, $r.applied, "$($r.show_to_paint.p50_ms)/$($r.show_to_paint.p95_ms)", $r.material_check.p95_ms, "$($r.dwm_gpu_3d_percent_visible)/$($r.dwm_gpu_3d_percent_hidden)", "$($r.min_primary_contrast)/$($r.min_secondary_contrast)")
    }
    Write-Host "  contrast = min over 3 background samples, primary/secondary text vs what is on screen"
    Write-Host "  screenshots (private, not committed): $ShotDir"
    Write-Step "done: $OutDir"
}
finally {
    Pop-Location
}
