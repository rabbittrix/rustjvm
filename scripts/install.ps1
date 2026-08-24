# scripts/install.ps1 — one-line installer for the RustJVM runtime on Windows.
#
#   irm https://rustjvm.dev/install.ps1 | iex

$ErrorActionPreference = "Stop"

$RustjvmVersion = "0.1.0-alpha"
$Arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") { "arm64" } else { "amd64" }
$Url = "https://github.com/rustjvm/rustjvm/releases/download/v$RustjvmVersion/rustjvm-windows-$Arch.exe"

$InstallDir = if ($env:RUSTJVM_HOME) { $env:RUSTJVM_HOME } else { Join-Path $env:USERPROFILE ".rustjvm" }
$BinDir = Join-Path $InstallDir "bin"
$Binary = Join-Path $BinDir "rustjvm.exe"

Write-Host "Installing RustJVM runtime $RustjvmVersion (windows-$Arch)..."

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
Write-Host "Downloading $Url ..."
Invoke-WebRequest -Uri $Url -OutFile $Binary

# Add to the user PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$BinDir;$UserPath", "User")
    [Environment]::SetEnvironmentVariable("RUSTJVM_HOME", $InstallDir, "User")
    Write-Host "Added $BinDir to the user PATH. Restart your terminal to pick it up."
}

Write-Host "RustJVM installed successfully!"
Write-Host "Run 'rustjvm --version' to verify."
