# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-17

### Added
- **Zero-Mutation Health Diagnostic Command (`agyo doctor`)**:
  - Brand-new `agyo doctor` diagnostic command (aliases: `doc`, `dr`) and `--offline` probe flag.
  - 5-dimensional non-destructive health analysis:
    - *Target Plane (`~/.gemini`)*: File existence, JSON schema validity, and active client ID inspection (`Antigravity` vs legacy `Gemini CLI`).
    - *OS Keyring*: Proactive reachability and read check for `LegacyGeneric:target=gemini:antigravity` credentials without mutating system secrets.
    - *Runtime & Crash Journal*: Strict read-only inspection of cross-process lease locks and uncommitted WAL transaction journals (`wal.journal`).
    - *Network & Proxy Reachability*: Live connectivity, HTTP status semantics, and latency probing against Google Cloud Code PA endpoints (`cloudcode-pa.googleapis.com`, `daily-cloudcode-pa.googleapis.com`) and OAuth token endpoint (`oauth2.googleapis.com`).
    - *Orbit Vault Storage*: Multi-account storage hierarchy and index schema integrity validation (`~/.agyo/orbits/`).
  - SOCKS5 proxy auto-detection (`has_socks5`) alerting users to potentially incompatible `socks5://` proxy variables causing HTTP client timeouts.
  - Full-color TTY and graceful non-TTY plaintext reporter with actionable fix recommendations and non-zero exit code on critical failures.
- **Interactive Terminal Ergonomics Hints**:
  - Contextual `whoami` guidance recommending `agyo save <name>` when running in an interactive terminal under an unmanaged Antigravity account.
  - Contextual quota tip suggesting `agyo quota -a` for multi-orbit inspection.

### Changed
- **Typed Quota Error Architecture & Fail-Fast Cascading**:
  - Granular typed errors: `OrbitError::QuotaUnauthorized` (401), `OrbitError::QuotaForbidden` (403), and `OrbitError::QuotaRateLimited` (429) with `Retry-After` header extraction.
  - Implemented HTTP 403 fail-fast behavior stopping downstream endpoint retry cascades immediately, preventing unnecessary retry storms against Google Quota APIs.

### Fixed
- **PowerShell 5.1 RemoteException in Uninstallation Scripts**:
  - Isolated `$ErrorActionPreference` during `cargo uninstall agy-orbit` in `scripts/uninstall.ps1` to prevent terminating native stderr false-alarms.
  - Added automatic fallback deletion for orphaned `agyo.exe` / `agyo` binaries in Cargo bin directories when not tracked by Cargo metadata.
  - Hardened POSIX uninstaller (`scripts/uninstall.sh`) with robust Cargo bin resolution and fallback cleanup.

### Security
- **Strict Read-Only Guarantee in Diagnostics**:
  - Enforced zero-mutation design invariants across all doctor probes (zero disk writes, zero keyring modifications, zero lock acquisition side-effects).

## [0.1.0] - 2026-09-16

### Added
- **Tripartite Storage Architecture**: Complete physical decoupling across Target Plane (`~/.gemini/`), Storage & State Plane (`~/.agyo/`), and Ephemeral Runtime Plane (`%LOCALAPPDATA%\agy-orbit\run` / `$XDG_RUNTIME_DIR/agyo`).
- **Crash-Resilient WAL State Machine**: Four-phase atomic transition engine (`PREPARE -> APPLY -> VERIFY -> COMMIT`) with automated crash detection and idempotent rollback recovery.
- **Lifetime Lease Supervisor**: Kernel-level cross-process mutual exclusion (`flock` on Unix, `LockFileEx` on Windows) preventing keyring collision and state clobbering during concurrent terminal sessions (`agyo run`).
- **Two-Way Credential Capture**: Automatic detection, validation, and reverse synchronization of dynamically refreshed OAuth tokens upon child process exit within the lease window.
- **Silent Auto-Refresh Engine**: Proactive (<60s expiration) and reactive (HTTP 401 / `invalid_grant` retry) token refresh using Google OAuth refresh tokens, with encrypted persistence directly to Orbit Vault without clobbering the active environment.
- **Multi-Account Quota Dashboard (`agyo quota -a`)**: Aggregated view across all managed accounts with 6-state health triage (`Ready`, `Throttled`, `Exhausted`, `TokenStale`, `AuthExpired`, `Error`), `MetricState::Disabled` recognition, and smart bottleneck reset countdowns.
- **Safe In-Place Self-Update (`agyo upgrade`)**: Automated update command with architecture auto-detection, check-only mode (`-c, --check`), and SHA-256 integrity verification.
- **Safe Lifecycle Orchestration (`agyo uninstall`)**: Complete uninstall command with `--dry-run`, `--keep-vault`, and multi-tier directory protection (`remove_guarded_directory`).
- **Runtime Identity Alignment Engine**: Automatic reconciliation and detection of active account mismatch between disk credentials and orbit vault upon startup.
- **Pure-Keyring Support**: Compatibility with modern `LegacyGeneric:target=gemini:antigravity` credentials and zero-disk-write authentication modes.
- **Platform-Native Cryptographic Vault**:
  - Windows: Native DPAPI (`CryptProtectData`) with CurrentUser scope, zero UI popup, and memory buffer volatile-zeroization.
  - POSIX (macOS / Linux / BSD): AES-256-GCM authenticated encryption derived from machine ID, kernel UID, and persistent seed fallback.
- **Context-Aware Ergonomics**:
  - Interactive TTY: Beautiful TUI dashboard with arrow-key navigation.
  - Non-TTY / Pipes: Silent, non-blocking plaintext fallback (`whoami`).
  - Full first-class CLI aliases (`s`, `u`/`sw`, `ls`, `w`, `r`, `q`, `rm`, `comp`).
- **Dynamic Shell Completion**: Native script generation for Bash, Zsh, Fish, PowerShell, and Elvish with smart runtime auto-detection (<5ms fast-path).
- **Rust Edition 2024**: Full upgrade to Rust Edition 2024 with a 100% zero-unsafe hermetic test architecture.

### Changed
- **Quota Endpoint Prioritization**: Route primary quota queries to `daily-cloudcode-pa.googleapis.com` matching official `agy` CLI behavior, with production fallback to `cloudcode-pa.googleapis.com`.
- **Keyring Storage Format**: Write keyring secrets as compact raw UTF-8 bytes (`set_secret`) instead of UTF-16 strings (`set_password`), restoring the full 2560-byte platform capacity.
- **CI/CD Pipeline**: Upgraded to `actions/checkout@v7` natively on Node 24, with automated multi-platform release matrix and MSRV enforcement (Rust 1.98+).

### Fixed
- **Gemini False Capacity Quota Display**: Fixed 100% false capacity illusion for Gemini models caused by querying cold-start production endpoints instead of the active `daily-cloudcode` cluster.
- **Universal Invariant 0 (Quota Countdown)**: Fixed misleading 5-hour reset countdowns by strictly suppressing reset timestamps from Google-disabled quota buckets when weekly limits are exhausted.
- **Quota Cache Fallback on 401**: Gracefully fall back to valid cached quota data when inactive accounts encounter short-lived token expiration (401), preventing ghost full `-` displays.
- **Windows Keyring UTF-16 Limit Overflow**: Resolved `Keyring error: Attribute 'password encoded as UTF-16' is longer than platform limit of 2560 chars` during account switching.
- **Windows Console ANSI Escape Leakage**: Initialize Windows Virtual Terminal Processing (`ENABLE_VIRTUAL_TERMINAL_PROCESSING`) on legacy conhost consoles with automatic plaintext fallback.
- **Terminal Panic Cleanup**: Automatic terminal raw mode and cursor restoration via global panic hook on abnormal exit.

### Security
- **10-Point Self-Upgrade Decompression Defense**:
  - Zip-Slip and Tar-Slip path traversal elimination via strict relative component and parent directory validation.
  - Decompression bomb defense using a `+1` byte-stream probe bounded at 64 MiB.
  - Strict HTTPS enforcement, GitHub official domain whitelist, Userinfo (`@`) injection blocking, and redirect sandboxing (max 5 hops).
- **Destructive Deletion Guards (`remove_guarded_directory`)**: Multi-layer safety guards preventing accidental deletion of root, home, temp, or non-whitelisted paths, with symlink non-traversal protection.
- **OAuth Client Credential Obfuscation**: Static 0x5A XOR mask protection against static binary string extraction, with runtime environment variable override support (`AGYO_GOOGLE_CLIENT_ID`, `AGYO_GOOGLE_CLIENT_SECRET`).
- **In-Memory Credential Redaction**: All token and credential fields strictly masked (`[REDACTED]`) across all `Debug` implementations to eliminate log leakage.
- **Strict Target Invariant**: Non-negotiable guarantee never touching `~/.gemini/antigravity-cli/brain/`, plugins, presence, or external developer toolchains (Git, SSH, Cargo).
