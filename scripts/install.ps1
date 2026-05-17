# Installs Terminal Studio by downloading the latest release binary.
# Usage: iwr https://raw.githubusercontent.com/dpkay-io/terminal-studio/master/scripts/install.ps1 | iex
#Requires -Version 5.1
$ErrorActionPreference = "Stop"

$repo = "dpkay-io/terminal-studio"
$asset = "terminal-studio-windows.exe"
$installDir = Join-Path $env:LOCALAPPDATA "terminal-studio"
$dest = Join-Path $installDir "terminal-studio.exe"
$url = "https://github.com/$repo/releases/latest/download/$asset"

Write-Host "Downloading Terminal Studio..."
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Invoke-WebRequest -Uri $url -OutFile $dest -UseBasicParsing
Write-Host "Installed to $dest"

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$installDir*") {
    Write-Host ""
    Write-Host "Add Terminal Studio to your PATH permanently:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$installDir', 'User')"
}

Write-Host ""
Write-Host "Terminal Studio will notify you of future updates automatically."
