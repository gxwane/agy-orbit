# agy-orbit Safe Uninstaller (PowerShell)
# Compliant with Windows MSVC environments & strict error handling
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$Force,
    [Alias("KeepVault")]
    [switch]$KeepData  # Allow keeping user's multi-account encrypted storage
)

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "`n===============================================" -ForegroundColor Cyan
Write-Host "       agy-orbit (agyo) Safe Uninstaller       " -ForegroundColor Cyan
Write-Host "===============================================`n" -ForegroundColor Cyan

# 1. Check for running agyo process(es)
$running = Get-Process -Name "agyo" -ErrorAction SilentlyContinue
if ($running) {
    Write-Warning "Detected active 'agyo' process(es) (PID: $($running.Id -join ', '))."
    if (-not $Force) {
        $confirmClose = Read-Host "Would you like to terminate running agyo processes before proceeding? [y/N]"
        if ($confirmClose -match '^[yY]') {
            Stop-Process -Name "agyo" -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 500
        } else {
            Write-Error "Uninstallation aborted: Active processes hold locks on binaries and runtime files."
            exit 1
        }
    } else {
        Stop-Process -Name "agyo" -Force -ErrorAction SilentlyContinue
    }
}

# 2. Guarded safe directory removal with validation against root and invalid paths
function Remove-SafeDirectory {
    param(
        [string]$Path,
        [string]$ExpectedSuffix,
        [string]$Description
    )

    if ([string]::IsNullOrWhiteSpace($Path) -or [string]::IsNullOrWhiteSpace($ExpectedSuffix)) {
        return
    }

    try {
        $fullPath = [System.IO.Path]::GetFullPath($Path)
    } catch {
        return
    }

    # Security assertion: Path length >= 8, must end with expected suffix, strictly not drive root
    if ($fullPath.Length -lt 8 -or -not $fullPath.TrimEnd('\').EndsWith($ExpectedSuffix, [System.StringComparison]::OrdinalIgnoreCase)) {
        Write-Warning "Security Guard: Aborting deletion of '$fullPath' (failed suffix validation for '$ExpectedSuffix')."
        return
    }

    $root = [System.IO.Path]::GetPathRoot($fullPath)
    $userProfile = [System.Environment]::GetFolderPath('UserProfile')
    if ($fullPath -eq $root -or $fullPath -eq $userProfile) {
        Write-Warning "Security Guard: Target '$fullPath' points to a protected system root. Skipped."
        return
    }

    if (Test-Path -LiteralPath $fullPath) {
        Write-Host "[-] Removing $Description : $fullPath" -ForegroundColor Yellow
        Remove-Item -LiteralPath $fullPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 3. Uninstall binary
Write-Host "[1/4] Uninstalling binary..." -ForegroundColor Cyan
$cargoInstalled = $false
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $uninstallResult = & cargo uninstall agy-orbit 2>&1
    if ($LASTEXITCODE -eq 0) {
        $cargoInstalled = $true
        Write-Host "  ✓ Successfully uninstalled agy-orbit via cargo." -ForegroundColor Green
    }
}

if (-not $cargoInstalled) {
    $agyoCmd = Get-Command agyo -ErrorAction SilentlyContinue
    if ($agyoCmd) {
        Write-Host "  Notice: Standalone agyo binary found at: $($agyoCmd.Source)" -ForegroundColor Gray
        Write-Host "  Please manually remove the binary if it was not installed via Cargo." -ForegroundColor Gray
    } else {
        Write-Host "  ✓ No cargo-managed agyo binary found in current PATH." -ForegroundColor Gray
    }
}

# 4. Clean ephemeral runtime locks (%LOCALAPPDATA%\agy-orbit)
Write-Host "`n[2/4] Cleaning ephemeral runtime locks..." -ForegroundColor Cyan
if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    Remove-SafeDirectory -Path (Join-Path $env:LOCALAPPDATA "agy-orbit") -ExpectedSuffix "agy-orbit" -Description "Runtime directory"
}
# Fallback temp runtime directory
$tempFallback = Join-Path ([System.IO.Path]::GetTempPath()) "agy-orbit-run"
if (Test-Path -LiteralPath $tempFallback) {
    Remove-SafeDirectory -Path $tempFallback -ExpectedSuffix "agy-orbit-run" -Description "Fallback temp runtime directory"
}

# 5. Clean Orbit storage directory (~/.agyo)
Write-Host "`n[3/4] Checking Orbit storage data (~/.agyo)..." -ForegroundColor Cyan
$homeDir = [System.Environment]::GetFolderPath('UserProfile')
if (-not [string]::IsNullOrWhiteSpace($homeDir)) {
    $agyoDir = Join-Path $homeDir ".agyo"
    if (Test-Path -LiteralPath $agyoDir) {
        if ($KeepData) {
            Write-Host "  [i] User requested to preserve Orbit storage at: $agyoDir" -ForegroundColor Cyan
        } else {
            $proceedDelete = $Force
            if (-not $Force) {
                Write-Host "`n  ⚠️  WARNING: Storage directory contains your encrypted multi-account profiles!" -ForegroundColor Yellow
                $ans = Read-Host "  Do you want to permanently delete all Orbit credentials in '$agyoDir'? [y/N]"
                if ($ans -match '^[yY]') { $proceedDelete = $true }
            }
            if ($proceedDelete) {
                Remove-SafeDirectory -Path $agyoDir -ExpectedSuffix ".agyo" -Description "Orbit storage directory"
                Write-Host "  ✓ Orbit storage removed." -ForegroundColor Green
            } else {
                Write-Host "  [i] Preserved storage at $agyoDir." -ForegroundColor Gray
            }
        }
    } else {
        Write-Host "  ✓ No storage directory found at ~/.agyo" -ForegroundColor Gray
    }
}

# 6. Shell completion inspection & warning (Non-destructive: never silently mutate user profile)
Write-Host "`n[4/4] Inspecting Shell Profile configuration..." -ForegroundColor Cyan
if ($PROFILE -and (Test-Path -LiteralPath $PROFILE)) {
    $profileContent = Get-Content -LiteralPath $PROFILE -Raw -ErrorAction SilentlyContinue
    if ($profileContent -and ($profileContent -match 'agyo completion' -or $profileContent -match '\.agyo\\completion\.ps1')) {
        Write-Host "`n  ⚠️  ATTENTION: Found agyo shell completion in your PowerShell profile!" -ForegroundColor Yellow
        Write-Host "  File: $PROFILE" -ForegroundColor Gray
        Write-Host "  Please open your profile (e.g. 'notepad `$PROFILE') and remove the agyo completion line(s)" -ForegroundColor Yellow
        Write-Host "  to avoid startup errors in new terminal sessions.`n" -ForegroundColor Yellow
    } else {
        Write-Host "  ✓ PowerShell profile is clean." -ForegroundColor Green
    }
}

Write-Host "===============================================" -ForegroundColor Green
Write-Host "✓ agy-orbit uninstallation tasks completed." -ForegroundColor Green
Write-Host "Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact." -ForegroundColor Gray
Write-Host "If you wish to log out from Antigravity entirely, run: 'agy auth logout'`n" -ForegroundColor Gray
