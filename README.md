# agy-orbit (agyo)

[![CI](https://github.com/gxwane/agy-orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/gxwane/agy-orbit/actions)
[![Crates.io](https://img.shields.io/crates/v/agy-orbit.svg)](https://crates.io/crates/agy-orbit)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.98+](https://img.shields.io/badge/Rust-1.98%2B-orange.svg)](Cargo.toml)

**English** | [简体中文](README_zh.md)

> 🪐 **The lightweight multi-account switcher and quota dashboard for Google Antigravity CLI (`agy`).**  
> Switch accounts in 1s without re-login. Inspect live model quotas at a glance. Zero risk to chat history.

---

## ✨ Why agy-orbit?

During intensive coding sessions with Google Antigravity CLI (`agy`), hitting **rate limits (HTTP 429)** is a common frustration, yet official multi-account management is not yet available. Re-authenticating manually requires tedious browser OAuth flows, and bluntly copying configuration directories risks destroying your local conversation history (`brain/`) or hitting file-lock errors.

`agy-orbit` (`agyo`) solves this seamlessly:

- ⚡ **Instant Account Switching**: Snapshot your accounts once, then switch in milliseconds with `agyo use <name>`. Or simply run `agyo` to open an intuitive arrow-key interactive menu.
- 📊 **Aggregated Quota Dashboard**: Run `agyo quota --all` to view Gemini & Claude usage percentages, account health, and exact reset countdowns across all your accounts at a glance.
- 🔒 **Zero-Touch Safety**: Strictly scoped to auth tokens. **Never touches, scans, or overwrites** your chat history (`brain/`) or plugins. All secrets are secured with OS-native encryption (Windows DPAPI / POSIX AES-256-GCM).
- 🛡️ **Crash-Resilient & Concurrency-Safe**: Built-in 4-phase WAL state machine and process lifetime locks ensure your credentials are never corrupted, even during sudden power loss or multi-terminal runs.

---

## ⚡ Quick Look

```bash
# 🎯 Interactive switching: Just type agyo for an arrow-key menu (Enter to switch, Vim j/k supported)
$ agyo
? Select an Orbit to activate:
  personal (dev.alice@gmail.com)
> work     (alice@company.com)  [Active]
  backup   (spare.alice@gmail.com)

# 🚀 One-shot command line switch (<5ms, zero browser popups)
$ agyo use personal
✔ Switched active Orbit to 'personal' (dev.alice@gmail.com).

# 📊 Unified quota dashboard across all accounts (never get surprised by 429s again)
$ agyo quota --all
┌──────────┬──────────────────────┬───────────┬───────────┬───────────┬───────────┬─────────┬────────────┐
│ Orbit    │ Account              │ Gemini 5h │ Gemini Wk │ Claude 5h │ Claude Wk │ Health  │ Next Reset │
├──────────┼──────────────────────┼───────────┼───────────┼───────────┼───────────┼─────────┼────────────┤
│ * work   │ alice@company.com    │     85.0% │    100.0% │     60.0% │     90.0% │ Ready   │ 3h 42m     │
│   person │ dev.alice@gmail.com  │      0.0% │     45.0% │     15.0% │     50.0% │ Throttl │ 45m        │
└──────────┴──────────────────────┴───────────┴───────────┴───────────┴───────────┴─────────┴────────────┘
```

---

## 📦 Installation

### Mode 1: Automated One-Liner (Recommended)

Fast, secure web installation with SHA-256 integrity verification and automated PATH configuration:

- **Windows (PowerShell)**:
  ```powershell
  irm https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/install.ps1 | iex
  ```
- **macOS & Linux (Bash)**:
  ```bash
  curl -fsSL https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/install.sh | bash
  ```

### Mode 2: Prebuilt Standalone Binaries

Download standalone release archives directly from [GitHub Releases](https://github.com/gxwane/agy-orbit/releases), extract, and place `agyo` (`agyo.exe` on Windows) anywhere in your system `PATH`:
- **Windows (x86_64 MSVC)**: `agyo-x86_64-pc-windows-msvc.zip`
- **macOS (Apple Silicon)**: `agyo-aarch64-apple-darwin.tar.gz`
- **macOS (Intel x86_64)**: `agyo-x86_64-apple-darwin.tar.gz`
- **Linux (x86_64 glibc)**: `agyo-x86_64-unknown-linux-gnu.tar.gz`

> 💡 After installing the binary, you can keep it up to date at any time by running `agyo upgrade`.

### Mode 3: From Source via Cargo

```bash
# Install from crates.io
cargo install agy-orbit

# Or build locally from git repository
git clone https://github.com/gxwane/agy-orbit.git
cd agy-orbit
cargo install --path .
```

#### Linux System Prerequisites
On Linux distributions, `agy-orbit` utilizes system SecretService (D-Bus) for secure keyring integration. If building from source, ensure development headers are installed:
```bash
# Ubuntu / Debian
sudo apt-get install -y pkg-config libsecret-1-dev libdbus-1-dev

# Fedora / RHEL
sudo dnf install -y pkgconf libsecret-devel dbus-devel

# Arch Linux
sudo pacman -S --needed pkgconf libsecret dbus
```

---

## 🚀 Quick Start

```bash
# 1. Log in to your first account using official Antigravity CLI
agy auth login

# 2. Snapshot current credentials as a named Orbit
agyo save personal -l "Personal Gmail"

# 3. Log in to your second account
agy auth login

# 4. Snapshot as another Orbit
agyo save work -l "Company Workspace"

# 5. List all saved Orbits
agyo list

# 6. Globally switch active account in milliseconds
agyo use work

# 7. Or launch an isolated session with lifetime lease protection & auto token sync
agyo run personal -- agy

# 8. Inspect live quota consumption dashboard across all accounts
agyo quota --all

# 9. Diagnose environment health, authentication targets & connectivity
agyo doctor
```

---

## 📖 CLI Command Reference

| Command | Alias | Description |
| :--- | :---: | :--- |
| `agyo` *(no args)* | - | Smart TTY interactive dashboard with arrow-key TUI menu |
| `agyo save <orbit>` | `s` | Snapshot active credentials as a named Orbit (`-l, --label`, `-f, --force`) |
| `agyo use <orbit>` | `u`, `sw` | Atomically switch active account with WAL crash-recovery guarantee |
| `agyo list` | `ls` | Display formatted table of all saved Orbits, emails, and active state |
| `agyo whoami` | `w` | Show currently active Google account and Keyring details |
| `agyo run <orbit> [-- <cmd...>]`| `r` | Run command under Lifetime Lease lock with reverse token sync (`--restore`) |
| `agyo quota [orbit]` | `q` | Check live model quota and countdowns (`-a, --all`, `-r, --refresh`) |
| `agyo remove <orbit>` | `rm` | Safely delete a saved Orbit profile |
| `agyo doctor` | `doc`, `dr` | Run zero-mutation 5-dimension health diagnostic & connectivity probe (`-o, --offline`) |
| `agyo completion [shell]` | `comp`| Generate shell completion script (`--raw`, supports bash, zsh, fish, powershell, elvish) |
| `agyo upgrade` | `update`, `up` | Check for updates and self-upgrade binary in-place (`-c, --check`, `-f, --force`, `-p`) |
| `agyo uninstall` | `purge` | Safely uninstall agy-orbit and clean runtime data (`-y, --yes`, `--keep-vault`, `--dry-run`, `--delete-self`) |

### Detailed Flags & Options

#### `agyo save <NAME>`
- `-l, --label <STRING>`: Optional human-readable description (e.g. `"Work Pro Plan"`).
- `-f, --force`: Overwrite existing Orbit snapshot without prompting.

#### `agyo run <NAME> [--restore] [-- <CMD...>]`
- `--restore`: Automatically revert global credentials back to the previous Orbit upon child process exit.
- `cmd`: Command and arguments to execute (defaults to `agy`).

#### `agyo quota [NAME]`
- `-a, --all`: Display aggregated multi-account dashboard across all saved Orbits.
- `-r, --refresh`: Bypass 60-second local cache and fetch fresh remote data.

#### `agyo completion [SHELL]`
- `[SHELL]`: Target shell family (`bash`, `zsh`, `fish`, `powershell`, `elvish`). Auto-detected if omitted in interactive terminals.
- `--raw`: Output raw completion script without setup instructions.

#### `agyo doctor` (aliases: `doc`, `dr`)
- `-o, --offline`: Skip live Google Cloud Code and OAuth endpoint probing, performing local-only system, credential, and storage inspection.

#### `agyo upgrade`
- `-c, --check`: Check for updates without downloading or installing.
- `-f, --force`: Force reinstall or upgrade even if already on the latest version.
- `-p, --include-prereleases`: Include pre-release versions (Alpha / Beta / RC).

#### `agyo uninstall` (alias: `purge`)
- `-y, --yes`: Automatically confirm uninstallation without interactive prompts.
- `--dry-run`: Preview resources that would be affected without deleting any files.
- `--keep-vault` (alias `--keep-data`): Preserve encrypted multi-account vaults and orbits (`~/.agyo/orbits/`).
- `--delete-self`: Remove the running executable binary itself (handles OS file locking safely).

---

## 🐚 Shell Completion Setup

```bash
# PowerShell
agyo completion powershell >> $PROFILE

# Bash
agyo completion bash > ~/.local/share/bash-completion/completions/agyo

# Zsh
agyo completion zsh > ~/.zfunc/_agyo

# Fish
agyo completion fish > ~/.config/fish/completions/agyo.fish
```

---

## 🗑️ Uninstallation

`agy-orbit` creates no background daemons or stealth persistence. When you need to remove it, choose the method that best matches your workflow:

### Mode 1: Built-in Native CLI Command (Recommended — Zero Dependency)

`agyo` comes with a built-in uninstaller that operates under an exclusive lifetime lease lock and performs guarded, safe teardown:

```bash
# Interactive uninstallation with confirmation
agyo uninstall

# Unattended removal for scripts and automation (also deletes binary)
agyo uninstall -y --delete-self

# Remove runtime data and cache while preserving encrypted multi-account credentials
agyo uninstall --keep-vault

# Preview actions without deleting anything
agyo uninstall --dry-run
```

### Mode 2: Remote Web One-Liner / Bundled Script

If the binary has already been deleted or you prefer a standalone script:

- **Windows (PowerShell)**:
  ```powershell
  # Remote one-liner
  irm https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/uninstall.ps1 | iex

  # Or using the uninstaller script bundled in release archives
  powershell -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
  ```
- **macOS & Linux (Bash)**:
  ```bash
  # Remote one-liner
  curl -fsSL https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/uninstall.sh | bash

  # Or using the uninstaller script bundled in release archives
  ./scripts/uninstall.sh
  ```

### Mode 3: Transparent Native Shell Commands (Manual)

- **Windows (PowerShell)**:
  ```powershell
  # 1. Remove binary and PATH directory
  Remove-Item -Force "$HOME\.agyo\bin\agyo.exe" -ErrorAction SilentlyContinue

  # 2. Remove encrypted storage (or skip to keep credentials)
  Remove-Item -Recurse -Force "$HOME\.agyo" -ErrorAction SilentlyContinue

  # 3. Clean runtime locks
  Remove-Item -Recurse -Force "$env:LOCALAPPDATA\agy-orbit" -ErrorAction SilentlyContinue
  ```
- **macOS & Linux (Bash)**:
  ```bash
  # 1. Remove executable binary
  rm -f ~/.local/bin/agyo /usr/local/bin/agyo

  # 2. Remove encrypted storage (or skip to keep credentials)
  rm -rf ~/.agyo

  # 3. Clean runtime locks
  rm -rf "${XDG_RUNTIME_DIR:-/tmp}/agyo" 2>/dev/null || true
  rm -rf "${TMPDIR:-/tmp}/agyo-run-$(id -u)" 2>/dev/null || true
  ```

### Clean Up Shell Completion

If you previously configured shell completion, remove the corresponding line from your shell profile:
- **PowerShell**: Open `$PROFILE` and remove lines referencing `agyo completion` or `.agyo\completion.ps1`.
- **Bash / Zsh**: Remove `agyo completion` lines from `~/.bashrc` or `~/.zshrc`, or delete `~/.local/share/bash-completion/completions/agyo`.
- **Fish**: Delete `~/.config/fish/completions/agyo.fish`.

> [!NOTE]
> **Antigravity Credential Sovereignty**  
> `agy-orbit` strictly adheres to its Narrow Surface design invariant. Uninstalling `agyo` will **never** log out or delete credentials currently used by Google Antigravity (`~/.gemini/` and your OS Keyring). Your official `agy` session remains fully authenticated after uninstallation. To log out from Google entirely, use the official command: `agy auth logout`.

---

## 🔧 Environment Variables

| Variable | Default Value | Description |
| :--- | :--- | :--- |
| `AGYO_HOME` | `~/.agyo/` | Path to persistent Orbit storage root |
| `GEMINI_HOME`| `~/.gemini/` | Target Antigravity configuration directory |
| `AGYO_RUNTIME_DIR` | Windows: `%LOCALAPPDATA%\agy-orbit\run`<br>Unix: `$XDG_RUNTIME_DIR/agyo` | Ephemeral runtime lock & lease directory |
| `AGYO_KEYRING_TARGET` | `LegacyGeneric:target=gemini:antigravity` | Windows Credential Manager target entry |
| `AGYO_KEYRING_SERVICE`| `gemini` | Linux SecretService / macOS Keychain service name |
| `AGYO_NO_UPDATE_CHECK`| `0` (disabled) | Set to `1` or `true` to completely disable startup background update checks |
| `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` | *(None)* | Standard HTTP/HTTPS/SOCKS5 proxy URLs respected by upgrade and network operations |
| `NO_PROXY` | *(None)* | Comma-separated list of hostnames exempt from proxy routing |

---

## 🛡️ Architecture & Security Invariants (Deep Dive)

`agy-orbit` is built on explicit, verifiable engineering constraints to guarantee credential security and system stability:

### 1. Core Architectural Invariants
- **Narrow Runtime Scoping**: Strictly avoids touching, scanning, or overwriting `brain/` (session history) or `plugins/`. Never modifies `HOME` or `USERPROFILE` to ensure zero interference with external developer toolchains.
- **Strictly Manage 3 Authentication Targets**:
  - Target ①: `~/.gemini/oauth_creds.json` (active OAuth session)
  - Target ②: `~/.gemini/google_accounts.json` (account mappings)
  - Target ③: OS Credential Keyring (Windows: `LegacyGeneric:target=gemini:antigravity`, macOS: Keychain, Linux: SecretService)
- **Tripartite Decoupled Storage Topology**:
  - **Target Plane**: `~/.gemini/` (narrow, read/write active session only).
  - **Storage & State Plane**: `~/.agyo/` (independent, crash-resilient encrypted database).
  - **Ephemeral Runtime Plane**: `%LOCALAPPDATA%\agy-orbit\run` (Windows) or `$XDG_RUNTIME_DIR/agyo` (Linux/macOS) (tmpfs, memory-backed, immune to cloud drive sync locks).
- **Crash-Resilient WAL State Machine**: 4-phase atomic transitions (`PREPARE -> APPLY -> VERIFY -> COMMIT`). Auto-heals and rolls back interrupted switches on crash or power loss.
- **Lifetime Lease Supervisor & Two-Way Sync**: `agyo run <orbit> -- agy` holds an exclusive kernel lock during child process execution to prevent terminal switching collisions; on process exit, automatically captures newly refreshed OAuth tokens back to Orbit storage.

### 2. Anti-Malware & Security Design Constraints
To maintain architectural transparency and minimize false-positive detections under modern EDR and antivirus heuristics, `agy-orbit` strictly enforces the following engineering constraints:
1. **Scoped Credential Access**: Queries strictly the single target `gemini:antigravity`. Never enumerates or dumps other system secrets.
2. **No Process Injection**: Never calls `CreateRemoteThread` or injects code into external processes.
3. **No API Hooking**: Relies exclusively on standard, documented Win32 and POSIX system APIs.
4. **No Binary Packing**: Distributed as clean, deterministic Rust binaries without UPX or custom obfuscators.
5. **Zero Outbound Credential Telemetry**: Zero network requests other than standard Google quota endpoints and GitHub update checks.
6. **No Silent Persistence**: Never creates background daemons, scheduled tasks, or startup registry keys.
7. **Read-Only Quota Invariant**: `agyo quota` strictly reads existing access tokens and **never** rotates or exchanges refresh tokens.

---

## 🤝 Contributing

Contributions are welcome! Please ensure all pull requests pass the quality checks before submitting:

```powershell
# Windows
powershell -ExecutionPolicy Bypass -File .\scripts\verify_gauntlet.ps1

# Linux / macOS
./scripts/verify_gauntlet.sh
```

Please adhere to the [Code of Conduct](CODE_OF_CONDUCT.md).

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
