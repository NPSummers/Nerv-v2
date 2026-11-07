param(
    [Parameter(Mandatory = $true)][string]$Archive,
    [string]$Destination = (Join-Path $env:LOCALAPPDATA 'Nerv\bin')
)
$ErrorActionPreference = 'Stop'
$archivePath = (Resolve-Path -LiteralPath $Archive).Path
$checksumPath = "$archivePath.sha256"
if (Test-Path -LiteralPath $checksumPath) {
    $expected = ((Get-Content -LiteralPath $checksumPath -Raw).Trim() -split '\s+')[0]
} else {
    $name = [System.IO.Path]::GetFileName($archivePath)
    $sums = Join-Path (Split-Path -Parent $archivePath) 'SHA256SUMS'
    $checksumLines = @(Get-Content -LiteralPath $sums | Where-Object { $_ -match ('^[0-9a-fA-F]{64}\s+\*?' + [regex]::Escape($name) + '$') })
    if ($checksumLines.Count -ne 1) { throw 'No unique checksum for this archive in SHA256SUMS' }
    $expected = ($checksumLines[0] -split '\s+')[0]
}
if ($expected -notmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid SHA-256 checksum file' }
if ((Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash -ne $expected) { throw 'Compiler archive checksum mismatch' }
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $destinationPath) { throw 'Destination already exists; choose a new directory to install another version' }
$parent = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$staging = Join-Path $parent ('.nerv-install.' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging | Out-Null
try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        foreach ($entry in $zip.Entries) {
            $target = [System.IO.Path]::GetFullPath((Join-Path $staging $entry.FullName))
            if (-not $target.StartsWith("$staging\", [StringComparison]::OrdinalIgnoreCase)) { throw 'Archive entry escapes the installation directory' }
        }
    } finally { $zip.Dispose() }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $staging
    $directories = @(Get-ChildItem -LiteralPath $staging -Directory)
    if ($directories.Count -ne 1 -or -not (Test-Path -LiteralPath (Join-Path $directories[0].FullName 'nerv.exe'))) { throw 'Invalid Nerv compiler archive' }
    Move-Item -LiteralPath $directories[0].FullName -Destination $destinationPath
} finally {
    $resolvedStaging = [System.IO.Path]::GetFullPath($staging)
    if (-not $resolvedStaging.StartsWith((Join-Path $parent '.nerv-install.'), [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid staging cleanup path' }
    Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
}
Write-Host "Installed Nerv in $destinationPath"
Write-Host "Add this directory to your user PATH, then run nerv --version."
