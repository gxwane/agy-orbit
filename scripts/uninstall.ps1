# agy-orbit Windows Uninstaller (PowerShell)
# Compliant with Windows MSVC environments, safe path guards, and registry preservation
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [switch]$Force,
    [Alias("KeepData")]
    [switch]$KeepVault  # Allow keeping user's multi-account encrypted storage
)

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

Write-Host "`n===============================================" -ForegroundColor Cyan
Write-Host "       agy-orbit (agyo) Windows Uninstaller    " -ForegroundColor Cyan
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

# 3. Uninstall binary (one-liner install directory ~/.agyo/bin + Cargo)
Write-Host "[1/5] Removing binary files..." -ForegroundColor Cyan
$userHome = [System.Environment]::GetFolderPath('UserProfile')
$agyoBinExe = Join-Path $userHome ".agyo\bin\agyo.exe"
$agyoBinDir = Join-Path $userHome ".agyo\bin"

if (Test-Path -LiteralPath $agyoBinExe) {
    Remove-Item -LiteralPath $agyoBinExe -Force -ErrorAction SilentlyContinue
    Write-Host "  [OK] Removed executable: $agyoBinExe" -ForegroundColor Green
}
if ((Test-Path -LiteralPath $agyoBinDir) -and (Get-ChildItem -LiteralPath $agyoBinDir -ErrorAction SilentlyContinue).Count -eq 0) {
    Remove-Item -LiteralPath $agyoBinDir -Force -ErrorAction SilentlyContinue
}

# Check if installed via Cargo (supports CARGO_INSTALL_ROOT, CARGO_HOME, and default ~/.cargo/bin)
$cargoBinDir = if (-not [string]::IsNullOrWhiteSpace($env:CARGO_INSTALL_ROOT)) {
    Join-Path $env:CARGO_INSTALL_ROOT "bin"
} elseif (-not [string]::IsNullOrWhiteSpace($env:CARGO_HOME)) {
    Join-Path $env:CARGO_HOME "bin"
} elseif (-not [string]::IsNullOrWhiteSpace($userHome)) {
    Join-Path $userHome ".cargo\bin"
} else {
    $null
}

if ($cargoBinDir) {
    $cargoAgyoExe = Join-Path $cargoBinDir "agyo.exe"
    if ((Test-Path -LiteralPath $cargoAgyoExe) -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
        # Isolate ErrorActionPreference to prevent PowerShell 5.1 RemoteException / NativeCommandError
        # triggered by Cargo writing status/warnings to stderr even on successful operations
        $prevEAP = $ErrorActionPreference
        $cargoSucceeded = $false
        try {
            $ErrorActionPreference = 'SilentlyContinue'
            $null = & cargo uninstall agy-orbit 2>&1
            if ($LASTEXITCODE -eq 0) {
                $cargoSucceeded = $true
            }
        } catch {
            # Suppress any terminating error from native command stderr redirection
        } finally {
            $ErrorActionPreference = $prevEAP
        }

        if ($cargoSucceeded) {
            Write-Host "  [OK] Successfully uninstalled agy-orbit via Cargo." -ForegroundColor Green
        } elseif (Test-Path -LiteralPath $cargoAgyoExe) {
            # Fallback: if cargo uninstall failed (not tracked in Cargo metadata), purge orphaned binary directly
            Write-Host "  [i] Cargo package 'agy-orbit' not registered; removing orphaned binary: $cargoAgyoExe" -ForegroundColor Gray
            Remove-Item -LiteralPath $cargoAgyoExe -Force -ErrorAction SilentlyContinue
            if (-not (Test-Path -LiteralPath $cargoAgyoExe)) {
                Write-Host "  [OK] Removed orphaned executable from Cargo bin: $cargoAgyoExe" -ForegroundColor Green
            }
        }
    }
}

# 4. Remove User PATH registration from Registry
Write-Host "`n[2/5] Cleaning Environment PATH..." -ForegroundColor Cyan
$envSubKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
if ($envSubKey) {
    try {
        $rawPath = $envSubKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $regKind = $envSubKey.GetValueKind('Path')
    } catch {
        $rawPath = ''
        $regKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    }

    if ($null -eq $regKind -or $regKind -eq [Microsoft.Win32.RegistryValueKind]::None) {
        $regKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    }

    if ($rawPath) {
        $parts = $rawPath -split ';'
        $cleanedParts = @()
        $found = $false
        foreach ($p in $parts) {
            $trimmed = $p.Trim()
            if ($trimmed -ieq $agyoBinDir -or $trimmed -ieq "%USERPROFILE%\.agyo\bin" -or $trimmed -ieq '$HOME\.agyo\bin') {
                $found = $true
            } elseif (-not [string]::IsNullOrWhiteSpace($trimmed)) {
                $cleanedParts += $trimmed
            }
        }

        if ($found) {
            $newPath = $cleanedParts -join ';'
            $envSubKey.SetValue('Path', $newPath, $regKind)
            Write-Host "  [OK] Removed Orbit binary directory from User PATH." -ForegroundColor Green

            # Broadcast WM_SETTINGCHANGE
            try {
                Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition @"
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Auto)]
public static extern System.IntPtr SendMessageTimeout(
    System.IntPtr hWnd,
    uint Msg,
    System.IntPtr wParam,
    string lParam,
    uint fuFlags,
    uint uTimeout,
    out System.IntPtr lpdwResult
);
"@ -ErrorAction SilentlyContinue

                $HWND_BROADCAST = [System.IntPtr]0xffff
                $WM_SETTINGCHANGE = 0x001a
                $SMTO_ABORTIFHUNG = 0x0002
                $sendResult = [System.IntPtr]::Zero
                [Win32.NativeMethods]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [System.IntPtr]::Zero, 'Environment', $SMTO_ABORTIFHUNG, 2000, [ref]$sendResult) | Out-Null
            } catch {
                # Non-fatal
            }
        } else {
            Write-Host "  [OK] No Orbit directory entry found in User PATH." -ForegroundColor Gray
        }
    }
    $envSubKey.Close()
}

# 5. Clean ephemeral runtime locks (%LOCALAPPDATA%\agy-orbit)
Write-Host "`n[3/5] Cleaning ephemeral runtime locks..." -ForegroundColor Cyan
if (-not [string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    Remove-SafeDirectory -Path (Join-Path $env:LOCALAPPDATA "agy-orbit") -ExpectedSuffix "agy-orbit" -Description "Runtime directory"
}
$tempFallback = Join-Path ([System.IO.Path]::GetTempPath()) "agy-orbit-run"
if (Test-Path -LiteralPath $tempFallback) {
    Remove-SafeDirectory -Path $tempFallback -ExpectedSuffix "agy-orbit-run" -Description "Fallback temp runtime directory"
}

# 6. Clean Orbit storage directory (~/.agyo)
Write-Host "`n[4/5] Checking Orbit storage data (~/.agyo)..." -ForegroundColor Cyan
if (-not [string]::IsNullOrWhiteSpace($userHome)) {
    $agyoDir = Join-Path $userHome ".agyo"
    if (Test-Path -LiteralPath $agyoDir) {
        if ($KeepVault) {
            Write-Host "  [i] User requested to preserve Orbit storage at: $agyoDir" -ForegroundColor Cyan
        } else {
            $proceedDelete = $Force
            if (-not $Force) {
                Write-Host "`n  [WARN] WARNING: Storage directory contains your encrypted multi-account profiles!" -ForegroundColor Yellow
                $ans = Read-Host "  Do you want to permanently delete all Orbit credentials in '$agyoDir'? [y/N]"
                if ($ans -match '^[yY]') { $proceedDelete = $true }
            }
            if ($proceedDelete) {
                Remove-SafeDirectory -Path $agyoDir -ExpectedSuffix ".agyo" -Description "Orbit storage directory"
                Write-Host "  [OK] Orbit storage removed." -ForegroundColor Green
            } else {
                Write-Host "  [i] Preserved storage at $agyoDir." -ForegroundColor Gray
            }
        }
    } else {
        Write-Host "  [OK] No storage directory found at ~/.agyo" -ForegroundColor Gray
    }
}

# 7. Shell completion inspection & warning (Non-destructive: never silently mutate user profile)
Write-Host "`n[5/5] Inspecting Shell Profile configuration..." -ForegroundColor Cyan
if ($PROFILE -and (Test-Path -LiteralPath $PROFILE)) {
    $profileContent = Get-Content -LiteralPath $PROFILE -Raw -ErrorAction SilentlyContinue
    if ($profileContent -and ($profileContent -match 'agyo completion' -or $profileContent -match '\.agyo\\completion\.ps1')) {
        Write-Host "`n  [WARN] ATTENTION: Found agyo shell completion in your PowerShell profile!" -ForegroundColor Yellow
        Write-Host "  File: $PROFILE" -ForegroundColor Gray
        Write-Host "  Please open your profile (e.g. 'notepad `$PROFILE') and remove the agyo completion line(s)" -ForegroundColor Yellow
        Write-Host "  to avoid startup errors in new terminal sessions.`n" -ForegroundColor Yellow
    } else {
        Write-Host "  [OK] PowerShell profile is clean." -ForegroundColor Green
    }
}

Write-Host "===============================================" -ForegroundColor Green
Write-Host "[OK] agy-orbit uninstallation tasks completed." -ForegroundColor Green
Write-Host "Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact." -ForegroundColor Gray
Write-Host "If you wish to log out from Antigravity entirely, run: 'agy auth logout'`n" -ForegroundColor Gray
