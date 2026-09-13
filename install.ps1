# Installs macula-desktop for Windows: downloads the release archive
# matching this machine's OS/arch from GitHub Releases, verifies it
# against the release's own checksums.txt, and installs the binary into
# a user-local directory (no admin rights needed).
#
# Usage (PowerShell):
#   irm https://raw.githubusercontent.com/macula-io/macula-desktop/main/install.ps1 | iex
#
# Uninstall with the matching script:
#   irm https://raw.githubusercontent.com/macula-io/macula-desktop/main/uninstall.ps1 | iex
param(
    [string]$Version = $env:MACULA_DESKTOP_VERSION,
    [string]$InstallDir = $env:MACULA_DESKTOP_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

$Repo = "macula-io/macula-desktop"
$Bin = "macula-desktop.exe"

if (-not $Version) { $Version = "latest" }
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "macula-desktop" }

$Arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "arm64" } else { "x64" }
$Archive = "macula-desktop-$Version-windows-$Arch.zip"

if ($Version -eq "latest") {
    $DownloadUrl = "https://github.com/$Repo/releases/latest/download/$Archive"
    $ChecksumUrl = "https://github.com/$Repo/releases/latest/download/checksums.txt"
} else {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$Archive"
    $ChecksumUrl = "https://github.com/$Repo/releases/download/$Version/checksums.txt"
}

Write-Host "installing macula-desktop $Version for windows/$Arch into $InstallDir"

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("macula-desktop-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    $ArchivePath = Join-Path $Tmp $Archive
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchivePath

    $ChecksumsPath = Join-Path $Tmp "checksums.txt"
    Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumsPath
    $Expected = (Get-Content $ChecksumsPath | Where-Object { $_ -match [regex]::Escape($Archive) + "\s*$" } | Select-Object -First 1) -split "\s+" | Select-Object -First 1
    if (-not $Expected) { throw "no checksum entry for $Archive in checksums.txt" }
    $Actual = (Get-FileHash -Algorithm SHA256 $ArchivePath).Hash.ToLower()
    if ($Actual -ne $Expected) { throw "checksum mismatch for $Archive: expected $Expected, got $Actual" }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Expand-Archive -Path $ArchivePath -DestinationPath $Tmp -Force
    Copy-Item -Force (Join-Path $Tmp $Bin) (Join-Path $InstallDir $Bin)

    Write-Host "installed $(Join-Path $InstallDir $Bin)"
    if (-not ($env:Path -split ";" | Where-Object { $_ -eq $InstallDir })) {
        Write-Host "note: $InstallDir is not on your PATH — add it or run the full path"
    }
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
