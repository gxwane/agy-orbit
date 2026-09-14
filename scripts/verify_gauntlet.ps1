# agy-orbit 质量检查自动化脚本 (verify_gauntlet.ps1)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
$env:RUSTFLAGS = "-D warnings"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  agy-orbit Quality Verification Suite  " -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 1. 静态代码分析 (Clippy 零容忍)
Write-Host "`n[Gate 1/3] Running Cargo Clippy (-D warnings)..." -ForegroundColor Yellow
cargo clippy --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Clippy failed with warnings/errors!" -ForegroundColor Red
    exit 1
}

# 2. 代码格式化校验 (Cargo fmt)
Write-Host "`n[Gate 2/3] Checking Code Formatting (cargo fmt)..." -ForegroundColor Yellow
cargo fmt --all --check
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Formatting check failed! Run 'cargo fmt' to fix." -ForegroundColor Red
    exit 1
}

# 3. 完整测试套件 (Cargo test --all-targets & --doc)
Write-Host "`n[Gate 3/3] Running Cargo Test Suite (all targets & doc tests)..." -ForegroundColor Yellow
cargo test --all-targets
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Unit/Integration tests failed!" -ForegroundColor Red
    exit 1
}
cargo test --doc
if ($LASTEXITCODE -ne 0) {
    Write-Host "❌ Doc-tests failed!" -ForegroundColor Red
    exit 1
}

Write-Host "`n✅ [ALL PASSED] All 3 quality checks are 100% GREEN!" -ForegroundColor Green
exit 0
