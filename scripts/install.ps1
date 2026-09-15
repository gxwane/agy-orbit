# agy-orbit Windows Installer (PowerShell)
# Installs prebuilt binary from GitHub Releases or from local source
[CmdletBinding()]
param(
    [string]$Version = $(if ($env:AGYO_VERSION) { $env:AGYO_VERSION } else { "latest" }),
    [switch]$FromSource
)

$ErrorActionPreference = 'Stop'
$OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

# Ensure TLS 1.2 and TLS 1.3 are enabled for network requests
try {
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12 -bor [System.Net.SecurityProtocolType]::Tls13
} catch {
    # Fallback for environments where Tls13 is not defined in enum
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12
}

Write-Host "`n===============================================" -ForegroundColor Cyan
Write-Host "       agy-orbit (agyo) Windows Installer       " -ForegroundColor Cyan
Write-Host "===============================================`n" -ForegroundColor Cyan

if ($FromSource) {
    Write-Host "Installing agy-orbit from local source via Cargo..." -ForegroundColor Cyan
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Error "Cargo was not found in PATH. Please install Rust toolchain first."
        exit 1
    }
    cargo install --path . --force
    if ($LASTEXITCODE -eq 0) {
        Write-Host "`n✓ agyo has been successfully installed to Cargo bin directory!" -ForegroundColor Green
        Write-Host "Run 'agyo --help' to get started.`n" -ForegroundColor Cyan
        exit 0
    } else {
        Write-Error "Installation from source failed."
        exit 1
    }
}

# 1. Platform architecture assertion
if ([System.IntPtr]::Size -ne 8) {
    Write-Error "Unsupported architecture: agy-orbit Windows prebuilt binaries require 64-bit (x86_64) Windows."
    exit 1
}

$repoOwner = "gxwane"
$repoName = "agy-orbit"
$assetName = "agyo-x86_64-pc-windows-msvc.zip"
$checksumName = "agyo-x86_64-pc-windows-msvc.sha256"

if ($Version -eq "latest") {
    $baseUrl = "https://github.com/$repoOwner/$repoName/releases/latest/download"
} else {
    $cleanVer = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $baseUrl = "https://github.com/$repoOwner/$repoName/releases/download/$cleanVer"
}

$assetUrl = "$baseUrl/$assetName"
$checksumUrl = "$baseUrl/$checksumName"

# 2. Download and verify binary in a secure temporary directory
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) "agyo-install-$([System.Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
    $archivePath = Join-Path $tempDir $assetName
    $checksumPath = Join-Path $tempDir $checksumName

    Write-Host "[1/4] Downloading release assets ($Version)..." -ForegroundColor Cyan
    Write-Host "  Asset URL: $assetUrl" -ForegroundColor Gray

    try {
        try {
            Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing
        } catch {
            Invoke-WebRequest -Uri "$baseUrl/$assetName.sha256" -OutFile $checksumPath -UseBasicParsing
        }
        Invoke-WebRequest -Uri $assetUrl -OutFile $archivePath -UseBasicParsing
    } catch {
        Write-Host "`n❌ Download failed: $($_.Exception.Message)" -ForegroundColor Red
        Write-Host "Please check your network connection or verify that release version '$Version' exists." -ForegroundColor Yellow
        Write-Host "You can also install from source: cargo install agy-orbit`n" -ForegroundColor Gray
        exit 1
    }

    Write-Host "`n[2/4] Verifying SHA-256 checksum..." -ForegroundColor Cyan
    $checksumContent = (Get-Content -LiteralPath $checksumPath -Raw).Trim()
    $expectedHash = ($checksumContent -split '\s+')[0].Trim()
    $actualHash = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.Trim()

    if (-not ($actualHash -ieq $expectedHash)) {
        Write-Error "SHA-256 verification failed!`nExpected: $expectedHash`nActual:   $actualHash`nDownloaded asset may be corrupted or compromised."
        exit 1
    }
    Write-Host "  ✓ Checksum verified: $actualHash" -ForegroundColor Green

    # 3. Extract and place binary
    Write-Host "`n[3/4] Installing executable..." -ForegroundColor Cyan
    $userHome = [System.Environment]::GetFolderPath('UserProfile')
    $installBinDir = Join-Path $userHome ".agyo\bin"
    if (-not (Test-Path -LiteralPath $installBinDir)) {
        New-Item -ItemType Directory -Force -Path $installBinDir | Out-Null
    }

    # Extract archive safely to temporary extraction folder
    $extractDir = Join-Path $tempDir "extracted"
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force

    $extractedExe = Join-Path $extractDir "agyo.exe"
    if (-not (Test-Path -LiteralPath $extractedExe)) {
        Write-Error "Archive did not contain 'agyo.exe'."
        exit 1
    }

    $targetExe = Join-Path $installBinDir "agyo.exe"

    # If agyo.exe exists and is running, warn and attempt replacement
    if (Test-Path -LiteralPath $targetExe) {
        $running = Get-Process -Name "agyo" -ErrorAction SilentlyContinue
        if ($running) {
            Write-Warning "Stopping active agyo process before replacement..."
            Stop-Process -Name "agyo" -Force -ErrorAction SilentlyContinue
            Start-Sleep -Milliseconds 500
        }
    }

    Copy-Item -LiteralPath $extractedExe -Destination $targetExe -Force
    Write-Host "  ✓ Installed binary to: $targetExe" -ForegroundColor Green

    # 4. Register in User PATH environment variable preserving REG_EXPAND_SZ
    Write-Host "`n[4/4] Configuring Environment PATH..." -ForegroundColor Cyan
    $envSubKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
    if ($envSubKey) {
        try {
            $rawPath = $envSubKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
            $regKind = $envSubKey.GetValueKind('Path')
        } catch {
            $rawPath = ''
            $regKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        }

        # Keep existing kind, default to ExpandString if not set
        if ($null -eq $regKind -or $regKind -eq [Microsoft.Win32.RegistryValueKind]::None) {
            $regKind = [Microsoft.Win32.RegistryValueKind]::ExpandString
        }

        $pathParts = if ($rawPath) { $rawPath -split ';' } else { @() }
        $alreadyInPath = $false
        foreach ($part in $pathParts) {
            $trimmed = $part.Trim()
            if ($trimmed -ieq $installBinDir -or $trimmed -ieq "%USERPROFILE%\.agyo\bin" -or $trimmed -ieq '$HOME\.agyo\bin') {
                $alreadyInPath = $true
                break
            }
        }

        if (-not $alreadyInPath) {
            $newPath = if ([string]::IsNullOrWhiteSpace($rawPath)) {
                $installBinDir
            } else {
                "$rawPath;$installBinDir"
            }
            $envSubKey.SetValue('Path', $newPath, $regKind)
            Write-Host "  ✓ Added '$installBinDir' to User PATH." -ForegroundColor Green

            # Broadcast WM_SETTINGCHANGE to notify running applications of environment change
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
                # Non-fatal if broadcast fails in headless/container environments
            }
        } else {
            Write-Host "  ✓ User PATH already contains Orbit binary directory." -ForegroundColor Green
        }
        $envSubKey.Close()
    }

    # Update current session path
    if ($env:Path -notmatch [regex]::Escape($installBinDir)) {
        $env:Path = "$installBinDir;$env:Path"
    }

    # Run --version check
    $installedVersion = & $targetExe --version 2>&1
    Write-Host "`n===============================================" -ForegroundColor Green
    Write-Host "✓ Installation completed successfully!" -ForegroundColor Green
    Write-Host "  Version: $installedVersion" -ForegroundColor Green
    Write-Host "  Location: $targetExe" -ForegroundColor Gray
    Write-Host "===============================================" -ForegroundColor Green
    Write-Host "Run 'agyo --help' to get started.`n" -ForegroundColor Cyan

} finally {
    # Clean up temporary directory
    if (Test-Path -LiteralPath $tempDir) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
