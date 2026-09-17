# agy-orbit Quality Verification Suite (verify_gauntlet.ps1)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$env:RUSTFLAGS = "-D warnings"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  agy-orbit Quality Verification Suite  " -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 1. Static code analysis (Clippy zero tolerance)
Write-Host "`n[Gate 1/3] Running Cargo Clippy (-D warnings)..." -ForegroundColor Yellow
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Clippy failed with warnings/errors!" -ForegroundColor Red
    exit 1
}

# 2. Code formatting check (Cargo fmt)
Write-Host "`n[Gate 2/3] Checking Code Formatting (cargo fmt)..." -ForegroundColor Yellow
cargo fmt --all --check
if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Formatting check failed! Run 'cargo fmt' to fix." -ForegroundColor Red
    exit 1
}

# 3. Test suite (Cargo test --all-targets & --doc)
Write-Host "`n[Gate 3/3] Running Cargo Test Suite (all targets & doc tests)..." -ForegroundColor Yellow
cargo test --all-targets
if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Unit/Integration tests failed!" -ForegroundColor Red
    exit 1
}
cargo test --doc
if ($LASTEXITCODE -ne 0) {
    Write-Host "[FAIL] Doc-tests failed!" -ForegroundColor Red
    exit 1
}

Write-Host "`n[OK] [ALL PASSED] All 3 quality checks are 100% GREEN!" -ForegroundColor Green
exit 0
