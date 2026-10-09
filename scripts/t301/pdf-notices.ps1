# Generate redistribution notices for the packages introduced by T301. No network requests.
# Run from the repository root; the pinned registry packages must already be available.
$ErrorActionPreference = 'Stop'
$taskPdfPackages = @(
    'aes@0.9.3', 'alloc-no-stdlib@2.0.4', 'alloc-stdlib@0.2.4', 'block-buffer@0.12.1',
    'block-padding@0.4.2', 'brotli-decompressor@5.0.3', 'cbc@0.2.1', 'chacha20@0.10.2',
    'cipher@0.5.2', 'const-oid@0.10.2', 'cpubits@0.1.1', 'cpufeatures@0.3.1',
    'crypto-common@0.2.2', 'digest@0.11.3', 'ecb@0.2.1', 'hybrid-array@0.4.15',
    'inout@0.2.2', 'lopdf@0.45.0', 'md-5@0.11.0', 'nom@8.0.0', 'rand@0.10.3',
    'rand_core@0.10.1', 'rangemap@1.8.0', 'sha2@0.11.0', 'stringprep@0.1.5',
    'unicode-bidi@0.3.18', 'unicode-properties@0.1.4', 'weezl@0.2.1'
)
$taskPdfMetadata = cargo metadata --format-version 1 --locked --offline | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw 'Locked offline Cargo metadata failed' }
$taskPdfNotices = [System.Text.StringBuilder]::new()
[void]$taskPdfNotices.AppendLine("PDF text extractor notices (T301, lopdf 0.45.0)")
[void]$taskPdfNotices.AppendLine("Generated from pinned registry packages by scripts/t301/pdf-notices.ps1.")
[void]$taskPdfNotices.AppendLine("These notices supplement the model/runtime and existing application dependencies.")
foreach ($taskPdfKey in $taskPdfPackages) {
    $taskPdfPackage = $taskPdfMetadata.packages | Where-Object { "$($_.name)@$($_.version)" -eq $taskPdfKey }
    if (@($taskPdfPackage).Count -ne 1) { throw "Missing or ambiguous locked package $taskPdfKey" }
    $taskPdfRoot = Split-Path -Parent $taskPdfPackage.manifest_path
    $taskPdfLicenses = @(Get-ChildItem -LiteralPath $taskPdfRoot -File | Where-Object { $_.Name -match '^(licen[cs]e|copying|copyright)' } | Sort-Object Name)
    # The published subcrate omits its repository-root BSD license. The checked-in
    # copy was verified against that package's exact VCS commit (see ADR-039).
    if ($taskPdfKey -eq 'alloc-stdlib@0.2.4' -and $taskPdfLicenses.Count -eq 0) {
        $taskPdfLicenses = @(Get-Item -LiteralPath 'docs/licenses/alloc-stdlib-0.2.4-BSD-3-Clause.txt')
    }
    if ($taskPdfLicenses.Count -eq 0) { throw "No license files found for $taskPdfKey" }
    [void]$taskPdfNotices.AppendLine("`n=== $taskPdfKey ($($taskPdfPackage.license)) ===")
    foreach ($taskPdfLicense in $taskPdfLicenses) {
        [void]$taskPdfNotices.AppendLine("--- $($taskPdfLicense.Name) ---")
        [void]$taskPdfNotices.AppendLine([System.IO.File]::ReadAllText($taskPdfLicense.FullName))
    }
}
$taskPdfDestination = Join-Path (Get-Location) 'docs/licenses/pdf-extractor-notices.txt'
[void][System.IO.Directory]::CreateDirectory((Split-Path -Parent $taskPdfDestination))
$taskPdfFinalText = $taskPdfNotices.ToString().Replace("`r`n", "`n").TrimEnd() + "`n"
[System.IO.File]::WriteAllText($taskPdfDestination, $taskPdfFinalText, [System.Text.UTF8Encoding]::new($false))
Write-Output "Wrote pinned notices for $($taskPdfPackages.Count) packages."
