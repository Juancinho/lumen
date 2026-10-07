<#
.SYNOPSIS
  T009 file inventory check on this Windows PC: coverage, edge cases, speed, stable identity.

.DESCRIPTION
  1. Builds lumen-bench (release) and runs the lumen-indexer unit tests natively on Windows.
  2. Creates an edge-case folder in %TEMP% (path > 260 chars, junction loop, hidden+system
     file, unreadable folder, non-ASCII and unpaired-surrogate names, trailing dot, reserved
     name, empty file) and checks that every entry is found or reported.
  3. Inventories your real folders (Documents, Desktop, Downloads, Pictures, OneDrive if
     present) twice, with and without file identity, and compares the counts with an
     independent .NET walk.
  4. Runs identity-check (rename/move/copy/save-by-replace/hard link) on every fixed drive.

  Reports store COUNTS ONLY: no file or folder name of yours is written to the JSON files.
  The console shows up to 10 problem paths so you can see what failed and why.
  Read-only on your folders. Takes ~2-10 min depending on how many files you have.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File scripts\t009\run-windows-scan.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = "",
    [string[]]$Roots = @(),
    [switch]$SkipBuild,
    [switch]$SkipOracle
)

$ErrorActionPreference = "Stop"
$Repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }

$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem
$tag = "$((Get-Date).ToString('yyyy-MM-dd'))-$($env:COMPUTERNAME.ToLower())"
if (-not $OutDir) { $OutDir = Join-Path $Repo "docs\benchmarks\t009\$tag" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$volumes = @(Get-Volume -ErrorAction SilentlyContinue |
    Where-Object { $_.DriveLetter -and $_.DriveType -eq "Fixed" } |
    ForEach-Object { [ordered]@{ drive = "$($_.DriveLetter):"; fs = $_.FileSystem; size_gb = [math]::Round($_.Size / 1GB, 1) } })
if (-not $volumes) { $volumes = @([ordered]@{ drive = $env:SystemDrive; fs = "unknown"; size_gb = 0 }) }
$longPaths = (Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" -ErrorAction SilentlyContinue).LongPathsEnabled
$machine = [ordered]@{
    cpu = $cpu.Name.Trim(); threads = $cpu.NumberOfLogicalProcessors
    ram_gb = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
    os = "$($os.Caption) $($os.Version) (build $($os.BuildNumber))"
    long_paths_enabled = $longPaths
    volumes = $volumes
    date = (Get-Date).ToString("s")
}
$machine | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $OutDir "machine.json") -Encoding UTF8

$exe = Join-Path $Repo "target\release\lumen-bench.exe"

# Runs lumen-bench, shows its stderr summary, saves it (without paths) next to the JSON.
function Invoke-Bench([string]$name, [string[]]$benchArgs) {
    $json = Join-Path $OutDir "$name.json"
    $ErrorActionPreference = "Continue"
    $lines = & $exe @benchArgs "--json" $json 2>&1 |
        ForEach-Object { if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" } } |
        Where-Object { $_ -and $_.Trim() }
    $code = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    $lines | ForEach-Object { Write-Host $_ }
    # Issue lines contain paths: keep them on screen only.
    $lines | Where-Object { $_ -notmatch "^\s*issue " } | Set-Content (Join-Path $OutDir "$name.log") -Encoding UTF8
    if ($code -ne 0) { Write-Warning "$name failed (exit $code)" }
    if (Test-Path $json) { return (Get-Content $json -Raw | ConvertFrom-Json) }
    return $null
}

# Independent count: .NET enumeration, links/junctions counted but not followed.
function Get-OracleCount([string[]]$paths) {
    $n = [int64]0; $errors = [int64]0
    $stack = New-Object System.Collections.Generic.Stack[string]
    foreach ($p in $paths) { $n++; $stack.Push($p) }
    $excluded = @('$recycle.bin', 'system volume information', '$winreagent', 'config.msi')
    while ($stack.Count -gt 0) {
        $dir = $stack.Pop()
        try { $items = [System.IO.Directory]::EnumerateFileSystemEntries($dir) | ForEach-Object { $_ } }
        catch { $errors++; continue }
        foreach ($item in $items) {
            $leaf = [System.IO.Path]::GetFileName($item)
            if ($excluded -contains $leaf.ToLowerInvariant()) { continue }
            $n++
            try { $attr = [System.IO.File]::GetAttributes($item) } catch { $errors++; continue }
            $isDir = ($attr -band [System.IO.FileAttributes]::Directory) -ne 0
            $isReparse = ($attr -band [System.IO.FileAttributes]::ReparsePoint) -ne 0
            if ($isDir -and $isReparse) {
                # Junction/symlink (not followed) vs cloud folder (followed): ask the shell.
                $lt = (Get-Item -LiteralPath $item -Force -ErrorAction SilentlyContinue).LinkType
                if ($lt) { continue }
            }
            if ($isDir) { $stack.Push($item) }
        }
    }
    return [ordered]@{ entries = $n; errors = $errors }
}

Push-Location $Repo
$edge = Join-Path $env:TEMP "lumen-t009-edge"
$locked = Join-Path $edge "locked"
try {
    if (-not $SkipBuild) {
        Write-Step "cargo build --release -p lumen-bench"
        cargo build --release -p lumen-bench
        if ($LASTEXITCODE -ne 0) { throw "build failed" }
        Write-Step "cargo test -p lumen-indexer (native Windows)"
        $ErrorActionPreference = "Continue"
        $testOut = cargo test -p lumen-indexer 2>&1 | ForEach-Object { "$_" }
        $testCode = $LASTEXITCODE
        $ErrorActionPreference = "Stop"
        $testOut | Where-Object { $_ -match "^test |test result|panicked|assert" } | ForEach-Object { Write-Host $_ }
        $testOut | Set-Content (Join-Path $OutDir "unit-tests.log") -Encoding UTF8
        if ($testCode -ne 0) { Write-Warning "lumen-indexer tests FAILED (see unit-tests.log)" }
    }

    # ---- 2. edge cases -------------------------------------------------------------------
    Write-Step "edge-case folder"
    if (Test-Path $edge) {
        cmd /c "rmdir `"$edge\real\loop`"" 2>$null | Out-Null
        icacls $locked /remove:d "$env:USERNAME" 2>$null | Out-Null
        cmd /c "rmdir /s /q `"\\?\$edge`"" | Out-Null
    }
    $v = "\\?\$edge"
    $script:created = 0
    # Creates one readable edge-case file through the verbatim path; returns whether it worked.
    function New-EdgeFile([string]$rel, [string]$text = "x") {
        try { [System.IO.File]::WriteAllText("$v\$rel", $text); $script:created++; return $true }
        catch { Write-Host "  could not create edge case: $rel ($($_.Exception.Message))" -ForegroundColor Yellow; return $false }
    }
    [System.IO.Directory]::CreateDirectory("$v\real") | Out-Null
    New-EdgeFile "real\normal.txt" | Out-Null
    New-EdgeFile "empty.txt" "" | Out-Null
    $deepRel = ""
    for ($i = 0; $i -lt 30; $i++) { $deepRel = "$deepRel\folder-$i-abcdefghij" }
    [System.IO.Directory]::CreateDirectory("$v$deepRel") | Out-Null
    New-EdgeFile "$($deepRel.TrimStart('\'))\deep-leaf.txt" | Out-Null
    $deepLen = ("$edge$deepRel\deep-leaf.txt").Length
    cmd /c "mklink /J `"$edge\real\loop`" `"$edge`"" | Out-Null
    if (New-EdgeFile "hidden-system.txt") { attrib +h +s "$edge\hidden-system.txt" }
    $uni = "reuni" + [char]0x00F3 + "n " + [char]0x65E5 + [char]0x672C + " " + [char]::ConvertFromUtf32(0x1F600) + ".txt"
    New-EdgeFile $uni | Out-Null
    $surrogateOk = New-EdgeFile ("lone-" + [char]0xD800 + ".txt")
    New-EdgeFile "trailing-dot." | Out-Null
    New-EdgeFile "aux.txt" | Out-Null
    [System.IO.Directory]::CreateDirectory("$v\locked") | Out-Null
    [System.IO.File]::WriteAllText("$v\locked\inside.txt", "x")
    icacls $locked /deny "${env:USERNAME}:(RX)" | Out-Null
    [System.IO.Directory]::CreateDirectory("$v\`$Recycle.Bin") | Out-Null
    [System.IO.File]::WriteAllText("$v\`$Recycle.Bin\old.txt", "x")
    Write-Host "  $($script:created) readable files; deepest path $deepLen chars; unpaired-surrogate name: $surrogateOk"

    $r = Invoke-Bench "edge-cases" @("scan", "--root", $edge, "--identity", "--show-issues", "10", "--label", "edge-cases")
    if ($r) {
        $identityFailures = 0
        foreach ($prop in $r.issues.PSObject.Properties) { if ($prop.Name -like "ReadIdentity/*") { $identityFailures += $prop.Value } }
        $checks = [ordered]@{
            "all $($script:created) readable files found" = ($r.files -eq $script:created)
            "junction emitted, not followed"              = ($r.links -eq 1)
            "hidden+system file counted"                  = ($r.hidden -ge 1 -and $r.system -ge 1)
            "unreadable folder reported, not hidden"      = ($r.blocking_issues -eq 1)
            "recycle bin excluded by a visible rule"      = ($r.excluded_by_rule.'system:$Recycle.Bin' -eq 1)
            "non-Unicode name emitted"                    = ((-not $surrogateOk) -or $r.non_unicode_paths -ge 1)
            "identity read (locked folder may refuse)"    = ($identityFailures -le 1)
        }
        $checks.GetEnumerator() | ForEach-Object {
            $color = if ($_.Value) { "Green" } else { "Red" }
            Write-Host ("  [{0}] {1}" -f $(if ($_.Value) { "ok" } else { "FAIL" }), $_.Key) -ForegroundColor $color
        }
        $checks | ConvertTo-Json | Set-Content (Join-Path $OutDir "edge-cases-checks.json") -Encoding UTF8
    }

    # ---- 3. real folders -----------------------------------------------------------------
    if (-not $Roots) {
        $Roots = @("Documents", "Desktop", "Downloads", "Pictures", "OneDrive") |
            ForEach-Object { Join-Path $env:USERPROFILE $_ } | Where-Object { Test-Path $_ }
    }
    $rootArgs = @(); foreach ($p in $Roots) { $rootArgs += @("--root", $p) }
    Write-Step "inventory of $($Roots.Count) folders (names not stored)"
    $plain = Invoke-Bench "scan-user" (@("scan") + $rootArgs + @("--repeat", "2", "--label", "user folders"))
    Write-Step "same, with stable identity"
    $ident = Invoke-Bench "scan-user-identity" (@("scan") + $rootArgs + @("--identity", "--repeat", "2", "--label", "user folders + identity"))

    if (-not $SkipOracle -and $plain) {
        Write-Step "independent .NET count (slow, PowerShell)"
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $oracle = Get-OracleCount $Roots
        $sw.Stop()
        $lumen = [int64]$plain.files + $plain.dirs + $plain.links + $plain.other + $plain.unknown
        $match = ($lumen -eq $oracle.entries)
        $cmp = [ordered]@{ lumen_entries = $lumen; dotnet_entries = $oracle.entries; dotnet_errors = $oracle.errors; match = $match; dotnet_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) }
        $cmp | ConvertTo-Json | Set-Content (Join-Path $OutDir "oracle.json") -Encoding UTF8
        $color = if ($match) { "Green" } else { "Yellow" }
        Write-Host ("  lumen {0} entries vs .NET {1} ({2} .NET errors) -> {3}" -f $lumen, $oracle.entries, $oracle.errors, $(if ($match) { "MATCH" } else { "DIFFERENT (files may have changed during the scan; rerun to confirm)" })) -ForegroundColor $color
    }

    # ---- 4. identity on every fixed drive ------------------------------------------------
    foreach ($vol in $volumes) {
        $dir = if ($vol.drive -eq $env:SystemDrive) { $env:TEMP } else { "$($vol.drive)\" }
        Write-Step "identity-check on $($vol.drive) ($($vol.fs))"
        $probe = Join-Path $dir "lumen-write-probe-$PID"
        try { New-Item -ItemType Directory -Path $probe -ErrorAction Stop | Out-Null; Remove-Item $probe }
        catch { Write-Host "  not writable, skipped"; continue }
        Invoke-Bench "identity-$($vol.drive.TrimEnd(':').ToLower())" @("identity-check", "--dir", $dir, "--label", "$($vol.drive) $($vol.fs)") | Out-Null
    }
    Write-Step "done: $OutDir"
}
finally {
    if (Test-Path $edge) {
        cmd /c "rmdir `"$edge\real\loop`"" 2>$null | Out-Null
        icacls $locked /remove:d "$env:USERNAME" 2>$null | Out-Null
        cmd /c "rmdir /s /q `"\\?\$edge`"" 2>$null | Out-Null
    }
    Pop-Location
}
