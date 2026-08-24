# scripts/install.ps1 — installer for the RustJVM runtime on Windows.
#
#   powershell -File scripts\install.ps1 -FromSource   # from a repo checkout (works today)
#   irm https://rustjvm.dev/install.ps1 | iex          # once the first release is tagged

[CmdletBinding()]
param(
    # Build the runtime from this repository instead of downloading a release binary.
    [switch]$FromSource
)

$ErrorActionPreference = "Stop"

$RustjvmVersion = "0.1.0-alpha"
$Arch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq "Arm64") { "arm64" } else { "amd64" }

$InstallDir = if ($env:RUSTJVM_HOME) { $env:RUSTJVM_HOME } else { Join-Path $env:USERPROFILE ".rustjvm" }
$BinDir = Join-Path $InstallDir "bin"
$Binary = Join-Path $BinDir "rustjvm.exe"

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

if ($FromSource) {
    $RepoRoot = Split-Path -Parent $PSScriptRoot
    Write-Host "Building RustJVM runtime from $RepoRoot (release mode)..."
    cargo build --release -p rustjvm-cli --manifest-path (Join-Path $RepoRoot "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    Copy-Item (Join-Path $RepoRoot "target\release\rustjvm.exe") $Binary -Force
} else {
    $Url = "https://github.com/rustjvm/rustjvm/releases/download/v$RustjvmVersion/rustjvm-windows-$Arch.exe"
    Write-Host "Installing RustJVM runtime $RustjvmVersion (windows-$Arch)..."
    Write-Host "Downloading $Url ..."
    Invoke-WebRequest -Uri $Url -OutFile $Binary
}

# Add to the user PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$BinDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$BinDir;$UserPath", "User")
    Write-Host "Added $BinDir to the user PATH. Restart your terminal to pick it up."
}
if (-not $env:RUSTJVM_HOME) {
    [Environment]::SetEnvironmentVariable("RUSTJVM_HOME", $InstallDir, "User")
}

Write-Host "RustJVM installed: $Binary"
& $Binary --version
