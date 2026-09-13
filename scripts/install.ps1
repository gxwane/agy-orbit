# agy-orbit Local Installer (PowerShell)
$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "Installing agy-orbit (agyo) from source..." -ForegroundColor Cyan

cargo install --path . --force

if ($LASTEXITCODE -eq 0) {
    Write-Host "`n✓ agyo has been successfully installed to Cargo bin directory!" -ForegroundColor Green
    Write-Host "Run 'agyo --help' to get started." -ForegroundColor Cyan
} else {
    Write-Host "`n❌ Installation failed." -ForegroundColor Red
    exit 1
}
