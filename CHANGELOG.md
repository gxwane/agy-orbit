# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-14

### Added
- **Tripartite Storage Architecture**: Complete physical decoupling across Target Plane (`~/.gemini/`), Storage & State Plane (`~/.agyo/`), and Ephemeral Runtime Plane (`%LOCALAPPDATA%\agy-orbit\run` / `$XDG_RUNTIME_DIR/agyo`).
- **Crash-Resilient WAL State Machine**: Four-phase atomic transition engine (`PREPARE -> APPLY -> VERIFY -> COMMIT`) with automated crash detection and rollback recovery.
- **Lifetime Lease Supervisor**: Kernel-level cross-process mutual exclusion preventing keyring collision and state clobbering during concurrent terminal sessions (`agyo run`).
- **Two-Way Credential Capture**: Automatic detection and reverse synchronization of dynamically refreshed OAuth tokens upon child process exit.
- **Platform-Native Cryptographic Storage**:
  - Windows: Native DPAPI (`CryptProtectData`) with CurrentUser scope, zero UI popup, and memory buffer volatile-zeroization.
  - POSIX (macOS / Linux / BSD): NIST SP 800-38D AES-256-GCM authenticated encryption with 3-tier entropy waterfall (machine ID + kernel UID + persistent home directory + CSPRNG seed).
- **Reverse Quota Engine (`agyo quota`)**:
  - Live model quota consumption status via Google Cloud Code PA endpoint fallback chain.
  - Read-only access token invariant (zero token exchange / zero refresh risk).
  - Single-account query, multi-account aggregated dashboard (`-a, --all`), and 60-second cache with force refresh (`-r, --refresh`).
- **Context-Aware Ergonomics**:
  - Interactive TTY: Beautiful TUI dashboard with arrow-key navigation.
  - Non-TTY / Pipes: Silent, non-blocking plaintext fallback (`whoami`).
  - Full first-class CLI aliases (`s`, `u`/`sw`, `ls`, `w`, `r`, `q`, `rm`, `comp`).
- **Dynamic Shell Completion**: Native script generation for Bash, Zsh, Fish, PowerShell, and Elvish with smart runtime auto-detection.
- **Clean Craftsmanship Quality Suite**: 100% Clippy zero-warning policy, strict formatting checks, and 64 automated unit & end-to-end integration tests.
