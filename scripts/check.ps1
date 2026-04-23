# Pre-flight checks that mirror the GitHub Actions CI workflow exactly.
# Run this before pushing to catch CI failures locally.
#
# Usage:
#   .\scripts\check.ps1            # run all checks
#   .\scripts\check.ps1 -NoTest    # skip the (slow) test step
param(
    [switch]$NoTest
)

$ErrorActionPreference = "Stop"

# Match CI: promote all rustc warnings to errors.
if (-not $env:RUSTFLAGS) {
    $env:RUSTFLAGS = "-D warnings"
}

Write-Host "==> cargo fmt --check"
cargo fmt --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (-not $NoTest) {
    Write-Host "==> cargo test"
    cargo test
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} else {
    Write-Host "==> cargo test (skipped via -NoTest)"
}

Write-Host "==> All pre-flight checks passed"
