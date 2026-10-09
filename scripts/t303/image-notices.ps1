# Offline redistribution notices for the eight pinned image-codec packages added by T303.
$ErrorActionPreference = 'Stop'
$taskImagePackages = @('byteorder-lite@0.1.0', 'image@0.25.10', 'image-webp@0.2.4', 'moxcms@0.8.1', 'pxfm@0.1.30', 'quick-error@2.0.1', 'zune-core@0.5.3', 'zune-jpeg@0.5.15')
$taskImageMetadata = cargo metadata --format-version 1 --locked --offline | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Locked offline Cargo metadata failed' }
$taskImageNotices = [System.Text.StringBuilder]::new()
[void]$taskImageNotices.AppendLine('Image codec notices (T303, image 0.25.10)')
[void]$taskImageNotices.AppendLine('Generated from pinned registry packages by scripts/t303/image-notices.ps1.')
foreach ($taskImageKey in $taskImagePackages) {
    $taskImagePackage = $taskImageMetadata.packages | Where-Object { "$($_.name)@$($_.version)" -eq $taskImageKey }
    if (@($taskImagePackage).Count -ne 1) { throw "Missing or ambiguous locked package $taskImageKey" }
    $taskImageRoot = Split-Path -Parent $taskImagePackage.manifest_path
    $taskImageLicenses = @(Get-ChildItem -LiteralPath $taskImageRoot -File | Where-Object { $_.Name -match '^(licen[cs]e|copying|copyright)' })
    if ($taskImageLicenses.Count -eq 0) { throw "No license files found for $taskImageKey" }
    # Ordinal order is identical in Windows PowerShell 5 and PowerShell 7; culture-aware
    # Sort-Object orders dots/hyphens differently between their collation engines.
    $taskImageLicensePaths = [string[]]@($taskImageLicenses | ForEach-Object { $_.FullName })
    [Array]::Sort($taskImageLicensePaths, [StringComparer]::Ordinal)
    [void]$taskImageNotices.AppendLine("`n=== $taskImageKey ($($taskImagePackage.license)) ===")
    foreach ($taskImageLicensePath in $taskImageLicensePaths) {
        [void]$taskImageNotices.AppendLine("--- $([IO.Path]::GetFileName($taskImageLicensePath)) ---")
        [void]$taskImageNotices.AppendLine([System.IO.File]::ReadAllText($taskImageLicensePath))
    }
}
$taskImageDestination = Join-Path (Get-Location) 'docs/licenses/image-codec-notices.txt'
$taskImageFinalText = $taskImageNotices.ToString().Replace("`r`n", "`n").TrimEnd() + "`n"
[System.IO.File]::WriteAllText($taskImageDestination, $taskImageFinalText, [System.Text.UTF8Encoding]::new($false))
Write-Output "Wrote pinned notices for $($taskImagePackages.Count) packages."
