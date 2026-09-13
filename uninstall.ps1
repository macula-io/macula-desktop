# Uninstalls macula-desktop for Windows: removes the binary install.ps1
# placed (or a custom directory).
#
# Usage (PowerShell):
#   irm https://raw.githubusercontent.com/macula-io/macula-desktop/main/uninstall.ps1 | iex
param(
    [string]$InstallDir = $env:MACULA_DESKTOP_INSTALL_DIR
)

$ErrorActionPreference = "Stop"

if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "macula-desktop" }
$Bin = Join-Path $InstallDir "macula-desktop.exe"

if (Test-Path $Bin) {
    Remove-Item -Force $Bin
    Write-Host "removed $Bin"
} else {
    Write-Host "macula-desktop is not installed at $Bin — nothing to remove."
}
