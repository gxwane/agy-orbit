# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-10-30

### Added
- **GEMINI 4 ARGON Separate Quota Pool Modeling & Dynamic Dashboard**:
  - Modeled `GEMINI 4 ARGON` as an isolated domain entity (`QuotaBucket::is_argon()`, `QuotaGroup::is_argon_group()`), preserving its separate weekly limit (e.g. 21.97%) without corrupting standard Gemini 5h / weekly pools.
  - Enhanced single-orbit terminal view (`agyo quota`) with official Antigravity styling for `GEMINI 4 ARGON` (complete with official plan description and dedicated progress bar).
  - Dynamically added `Argon Wk` column to the multi-account quota dashboard (`agyo quota -a`) whenever an account possesses an Argon quota pool, maintaining a compact 8-column layout when Argon is absent.
  - Added model names deduplication preserving order in terminal model lists, eliminating repeated model names (e.g., `Gemini 3.5 Flash Lite`).
  - Added clean fallback display names for `gemini-3.8-flash-tiered` (`Gemini 3.8 Flash (Tiered)`) and `gemini-4-argon-*` (`Gemini 4 Argon (Low/Medium/High)`).

### Fixed
- **Proto3 Zero-Value Omission in Model Quota Deserialization**:
  - Fixed false 100.0% display for exhausted model pools caused by Proto3 JSON serializer omitting `remainingFraction: 0.0` while retaining `resetTime`. `QuotaBucket::remaining_percentage()` now distinguishes between metered exhausted models (0.0%) and unmetered internal models (100.0%).
- **Project Payload in `:retrieveUserQuotaSummary`**:
  - Aligned `:retrieveUserQuotaSummary` requests with official `agy.exe` Go client by passing `{"project": "<project>"}` resolved via `:loadCodeAssist` (`cloudaicompanionProject`, fallback `"aicode-consumers"`), eliminating HTTP 403 `SUBSCRIPTION_REQUIRED` and unlocking official server-side quota groups (`Claude and GPT models`, `GEMINI 4 ARGON`) across all accounts.
- **Cross-Account Credential Attribution Guard**:
  - Fixed attribution mismatch where OS Keyring tokens without `id_token` on Windows were erroneously paired with stale `~/.gemini/google_accounts.json` emails. Keyring `refresh_token` is now verified against disk and matched against saved vault orbits to prevent token cross-contamination during CLI switching.
- **Multi-Account Quota Accuracy & Antigravity PA Alignment**:
  - Re-aligned primary quota endpoint with official `agy` CLI Go architecture (`fetchAvailableModels`), completely eliminating false-positive `HTTP 403 SUBSCRIPTION_REQUIRED` errors on individual / consumer accounts.
  - Decoupled `token_type: "WTUS"` (Weighted Token Usage System) from weekly quota classification; introduced adaptive $\Delta t = T_{\text{reset}} - T_{\text{now}}$ resolution where $\Delta t \le 12\text{h}$ maps to 5-hour rolling pool (`gemini_5h_pct`) and $\Delta t > 12\text{h}$ maps to weekly tiered limits (`gemini_wk_pct`).
  - Fixed empty `-` cell in `Gemini 5h` column of aggregated multi-account dashboard (`agyo quota -a`).
  - Enhanced single-orbit terminal view (`agyo quota`) with distinct pool foldings for `Gemini Flash & Pro Models (5-Hour Rolling Pool)` and `Gemini Tiered Models (Shared Weekly Pool)`, eliminating contradictory `Weekly Limit Remaining [weekly] (resets in 5h)` labels.
  - Added non-fatal group enrichment from `retrieveUserQuotaSummary` when available to preserve partner model breakdowns without failing on consumer tiers.

## [0.3.3] - 2026-10-21

### Fixed
- **Google Cloud Code PA API Resilience & Gemini Quota Restoration**:
  - Implemented non-fatal supplementary fetch for per-model quota buckets (`retrieveUserQuota`) when Google's daily cluster drops the aggregated `"Gemini Models"` group in `retrieveUserQuotaSummary`.
  - Added native `modelId` and `token_type` (`WTUS`) entity deserialization in `QuotaBucket`.
  - Added deterministic weekly quota detection via `is_weekly()` without fragile dynamic deadline thresholds.
  - Added clean model pool folding in single-account CLI quota view (`agyo quota`) to prevent visual pollution from repeated identical model limits.
  - Extended network budget to 8.0s (4.0s connect / 4.0s read) to improve resilience across HTTP/HTTPS proxies.

### Security
- **Dependency Vulnerability Remediation (`cargo-audit`)**:
  - Upgraded `rustls-webpki` to `0.103.16` resolving `RUSTSEC-2026-0382`.
  - Upgraded `webpki-roots` to `1.0.10` resolving `RUSTSEC-2026-0381`.

## [0.3.2] - 2026-10-20

### Security
- **Dependency Vulnerability Remediation (`cargo-audit`)**:
  - Upgraded `rustls` from `0.23.45` to `0.23.46` resolving:
    - `RUSTSEC-2026-0346`: TLS session secret exposure in `Debug` implementations.
    - `RUSTSEC-2026-0347`: Buffered application data sent to outer server after ECH rejection.
    - `RUSTSEC-2026-0348`: Brotli certificate decompression DoS.
    - `RUSTSEC-2026-0349`: Unauthenticated ServerHello key exchange downgrade.
  - Upgraded `zip` from `2.4.2` (`2.2` spec) to `9.0.5` (`9.0` spec) resolving:
    - `RUSTSEC-2026-0367`: Insecure preservation of SUID/SGID bits during extraction.
    - `RUSTSEC-2026-0368`: Symlink extraction path traversal outside destination directory.
    - `RUSTSEC-2026-0369`: Unconfined Unix permissions applied through extracted symlinks.

## [0.3.1] - 2026-09-17

### Fixed
- **HTTP / HTTPS / SOCKS5 Proxy Environment Support (`agyo upgrade`)**:
  - Enabled `proxy-from-env` and `socks-proxy` features in `ureq` HTTP client.
  - Activated `.try_proxy_from_env(true)` in `GitHubReleaseAdapter::build_agent()`, ensuring `agyo upgrade` and non-blocking startup update check respect standard proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, and `NO_PROXY`).
  - Added full support for SOCKS5 proxy schemes (`socks5://`, `socks5h://`) commonly utilized in restricted network environments.
  - Preserved strict `ALLOWED_HOSTS` domain whitelist and SHA-256 binary checksum verification, guaranteeing zero compromise to release asset anti-tamper security.

## [0.3.0] - 2026-09-17

### Added
- **Non-Blocking Startup Update Check (Autonomous Upgrade Sensing)**:
  - Background asynchronous sub-second micro-probe (`GitHubReleaseAdapter::new_micro_probe`) checking GitHub Releases without impacting command responsiveness (main thread overhead < 0.2ms).
  - **Optimistic Timestamp Reservation**: Atomically records `last_checked_at` before spawning probe thread, eliminating retry storms and rate-limit exhaustion from high-frequency command execution.
  - **Adaptive Cooldown Engine**: 24-hour long-term cooldown on successful probe; adaptive 1-hour fast retry if background thread was terminated early by short-lived CLI commands.
  - **Cheap Guard First Nano-Benchmark Path**: Pure CPU-bound enum matching for command whitelists exits within 1~2ns for 90% of non-whitelisted commands (`run`, `use`, `save`, `list`), bypassing terminal driver syscalls.
  - **Zero-Unwrap Fail-Silent Guarantee**: 100% silent degradation on network timeouts, DNS failures, or GitHub 403 rate limits without any console error or stderr pollution.
  - **Gentle Non-Blocking UI Hint**: Outputs a single-line, non-interactive update notification (`💡 Update available: vX.X.X -> vY.Y.Y. Run 'agyo upgrade' to upgrade.`) at the very bottom of whitelisted command execution (`agyo`, `whoami`, online `doctor`).
  - **System Clock Skew Defense**: Robust backward time detection in `is_expired` preventing perpetual cache freezing on NTP clock rollbacks.
  - **Self-Healing Corrupted Cache**: Automatic fallback to cache miss on damaged or truncated `update_check.json`.
  - **Comprehensive Escape Hatches & Guardrails**: Automatically bypassed in non-interactive terminals, Unix pipes, CI environments (`CI=true`), or explicit opt-out via `AGYO_NO_UPDATE_CHECK=1`.

### Changed
- **Cross-Platform Path Hygiene**: Enhanced `is_cargo_installation` with ASCII case-insensitivity and platform-agnostic path components matching on Windows, macOS, and Linux.

## [0.2.1] - 2026-09-17

### Fixed
- **Doctor Diagnostic Scope & False Positive Reduction**:
  - Context-aware client identity scoping dynamically reporting the active Orbit name (e.g. `OAuth Client Identity (Active Orbit: gio)`) rather than misleading global vault claims.
  - Downgraded open-source Gemini CLI detection from `CheckStatus::Warn` to `CheckStatus::Info`, preserving diagnostic guidance while preventing false-positive `ATTENTION REQUIRED` health statuses on operational systems.
  - Refined network probe details from bare `HTTP 404` to `HTTP 404 (Remote server responded)` to eliminate user confusion regarding connection reachability semantics.
  - Implemented safe fallback extraction for `storage.load_index()` preventing diagnostic disruption when uninitialized.

### Documentation
- **Complete CLI Reference for `agyo doctor`**:
  - Added `agyo doctor` (aliases: `doc`, `dr`) to Quick Start walkthroughs, command reference tables, and detailed flag manuals across both `README.md` and `README_zh.md`.

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
