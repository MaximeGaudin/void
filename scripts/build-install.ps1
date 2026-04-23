param(
    [string]$InstallDir = "$HOME\bin",
    [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BinName = "void.exe"
$Src = Join-Path "target\release" $BinName
$Dest = Join-Path $InstallDir $BinName

if (-not $SkipChecks) {
    Write-Host "==> Running pre-flight checks (fmt/clippy/test)..."
    & (Join-Path $ScriptDir "check.ps1")
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
    Write-Host "==> Skipping pre-flight checks (-SkipChecks)"
}

Write-Host "==> Building release binary..."
cargo build --release

# Stop any running sync daemon before replacing the binary.
if (Test-Path $Dest) {
    try {
        & $Dest sync --stop *> $null
        Write-Host "==> Stopped running sync daemon"
    } catch {
        # Best effort only.
    }
}

if (-not (Test-Path $Src)) {
    throw "Error: release binary not found at $Src"
}

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

# Use temp file + atomic rename to avoid in-use file replacement issues.
$TmpDest = Join-Path $InstallDir ".$($BinName).tmp.$PID"
Copy-Item $Src $TmpDest -Force
Move-Item $TmpDest $Dest -Force

Write-Host "==> Installed $BinName -> $Dest"

# Check PATH.
$pathEntries = ($env:PATH -split ';') | ForEach-Object { $_.TrimEnd('\') }
$normalizedInstall = $InstallDir.TrimEnd('\')
if (-not ($pathEntries -contains $normalizedInstall)) {
    Write-Host ""
    Write-Host "Warning: $InstallDir is not on your PATH."
    Write-Host "Add it in System Properties -> Environment Variables -> Path"
}
