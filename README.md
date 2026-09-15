# agy-orbit (agyo)

[![CI](https://github.com/gxwane/agy-orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/gxwane/agy-orbit/actions)
[![Crates.io](https://img.shields.io/crates/v/agy-orbit.svg)](https://crates.io/crates/agy-orbit)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.98+](https://img.shields.io/badge/Rust-1.98%2B-orange.svg)](Cargo.toml)

**English** | [简体中文](README_zh.md)

> 🪐 **Lightweight, transactional multi-account profile manager and lifetime lease supervisor for Google Antigravity CLI (`agy`).**
> 
> Fast (<5ms), cross-platform account switching and quota inspection with narrow authentication scoping and crash-resilient state guarantees.

---

## 💡 Why agy-orbit?

When using Google Antigravity CLI (`agy`) for intensive AI coding:
- `agy` does not natively support seamless multi-account profile switching or quota rotation.
- **Full Directory Mirroring vs. Granular Target Isolation**: Coarse-grained approaches often treat the entire `~/.gemini/antigravity-cli/` runtime tree as a swappable profile. Because this directory hosts active conversation states (`brain/`), cached artifacts, and process lockfiles (often spanning tens of thousands of volatile files), blanket directory swapping introduces significant risks of unintended history loss, heavy disk I/O, and cross-platform file-locking collisions (e.g., Windows `Sharing Violation`).

### 🛡️ Core Design Invariants

`agy-orbit` is built on explicit, verifiable engineering constraints:
1. **Narrow Runtime Scoping**: Leaves `~/.gemini/antigravity-cli/brain/`, `plugins/`, and runtime directories untouched. Avoids modifying `HOME` or `USERPROFILE` to prevent interference with developer toolchains (Git, OpenSSH, Cargo, npm, Docker).
2. **Strictly Manage 3 Authentication Targets**:
   - Target ①: `~/.gemini/oauth_creds.json` (active OAuth session)
   - Target ②: `~/.gemini/google_accounts.json` (account mappings)
   - Target ③: OS Credential Keyring (`gemini:antigravity`)
3. **Tripartite Decoupled Storage Topology**:
   - **Target Plane**: `~/.gemini/` (narrow, read/write active session only).
   - **Storage & State Plane**: `~/.agyo/` (independent, crash-resilient encrypted database).
   - **Ephemeral Runtime Plane**: `%LOCALAPPDATA%\agy-orbit\run` (Windows) or `$XDG_RUNTIME_DIR/agyo` (Linux/macOS) (tmpfs, memory-backed, immune to cloud drive sync locks).
4. **Crash-Resilient WAL State Machine**: 4-phase atomic transitions (`PREPARE -> APPLY -> VERIFY -> COMMIT`). Auto-heals and rolls back interrupted switches on crash or power loss.
5. **Platform-Native Encryption**: Windows DPAPI (`CryptProtectData`) / POSIX AES-256-GCM authenticated encryption. Zero plaintext secrets on disk.
6. **Lifetime Lease Supervisor & Two-Way Sync**:
   - `agyo run <orbit> -- agy`: Holds an exclusive kernel lock during child process execution to prevent terminal switching collisions.
   - On process exit, automatically captures newly refreshed OAuth tokens back to Orbit storage before releasing locks.
7. **Context-Aware Ergonomics**: Beautiful interactive TUI menu in interactive TTYs; silent plaintext fallback in scripts and pipes (`agyo | grep`).

---

## 📦 Installation

### Prerequisites (Linux Only)
On Linux distributions, `agy-orbit` utilizes system SecretService (D-Bus) for secure keyring integration. Install the required build libraries:

```bash
# Ubuntu / Debian
sudo apt-get install -y pkg-config libsecret-1-dev libdbus-1-dev

# Fedora / RHEL
sudo dnf install -y pkgconf libsecret-devel dbus-devel

# Arch Linux
sudo pacman -S --needed pkgconf libsecret dbus
```

### Prebuilt Binaries
Download standalone release archives from [GitHub Releases](https://github.com/gxwane/agy-orbit/releases), extract, and place the `agyo` binary (`agyo.exe` on Windows) somewhere in your system `PATH`:
- **Windows (x86_64 MSVC)**: `agyo-x86_64-pc-windows-msvc.zip`
- **macOS (Apple Silicon)**: `agyo-aarch64-apple-darwin.tar.gz`
- **macOS (Intel x86_64)**: `agyo-x86_64-apple-darwin.tar.gz`
- **Linux (x86_64 glibc)**: `agyo-x86_64-unknown-linux-gnu.tar.gz`

### From Source via Cargo
```bash
# Install from crates.io
cargo install agy-orbit

# Or build locally from clone
git clone https://github.com/gxwane/agy-orbit.git
cd agy-orbit
cargo install --path .
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
| `agyo completion [shell]` | `comp`| Generate shell completion script (`--raw`, supports bash, zsh, fish, powershell, elvish) |
| `agyo upgrade` | `update`, `up` | Check for updates and self-upgrade binary in-place (`-c, --check`, `-f, --force`, `-p`) |

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

#### `agyo upgrade`
- `-c, --check`: Check for updates without downloading or installing.
- `-f, --force`: Force reinstall or upgrade even if already on the latest version.
- `-p, --include-prereleases`: Include pre-release versions (Alpha / Beta / RC).

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

`agy-orbit` adheres to strict zero-pollution engineering: no startup entries, no registry manipulation, no background daemons. To cleanly uninstall, follow the graduated cleanup tiers:

### Tier 1: Remove Binary Only (Upgrade or Pause)

- **Installed via Cargo**:
  ```bash
  cargo uninstall agy-orbit
  ```
- **Installed via Prebuilt Releases**:
  Simply delete the `agyo` (or `agyo.exe`) binary from your `$PATH` directory.
  > 💡 This preserves your encrypted multi-account storage (`~/.agyo/`), allowing seamless reuse after reinstallation.

### Tier 2: Full Physical Teardown (Destroy Data & Locks)

- **One-click official safe uninstaller (recommended)**:
  ```powershell
  # Windows PowerShell
  powershell -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
  ```
  ```bash
  # macOS / Linux
  ./scripts/uninstall.sh
  ```
- **Manual cleanup commands**:
  - **Windows (PowerShell)**:
    ```powershell
    # 1. Remove encrypted storage
    Remove-Item -Recurse -Force "$HOME\.agyo" -ErrorAction SilentlyContinue
    # 2. Remove ephemeral runtime locks
    Remove-Item -Recurse -Force "$env:LOCALAPPDATA\agy-orbit" -ErrorAction SilentlyContinue
    ```
  - **macOS / Linux**:
    ```bash
    # 1. Remove encrypted storage
    rm -rf ~/.agyo
    # 2. Remove ephemeral runtime locks
    rm -rf "${XDG_RUNTIME_DIR:-/tmp}/agyo" 2>/dev/null || true
    rm -rf "${TMPDIR:-/tmp}/agyo-run-$(id -u)" 2>/dev/null || true
    ```

### Tier 3: Shell Completion Cleanup

If you previously configured shell completion, remove the corresponding lines from your shell profile to prevent startup errors:
- **PowerShell**: Open `$PROFILE` and remove any line referencing `agyo completion` or `.agyo\completion.ps1`.
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

---

## 🛡️ Security Design Constraints & Anti-Malware Invariants

To maintain architectural transparency and minimize false-positive detections under modern EDR and antivirus heuristics, `agy-orbit` strictly enforces the following engineering constraints:
1. **Scoped Credential Access**: Queries strictly the single target `gemini:antigravity`. Never enumerates or dumps other system secrets.
2. **No Process Injection**: Never calls `CreateRemoteThread` or injects code into external processes.
3. **No API Hooking**: Relies exclusively on standard, documented Win32 and POSIX system APIs.
4. **No Binary Packing**: Distributed as clean, deterministic Rust binaries without UPX or custom obfuscators.
5. **Zero Outbound Credential Telemetry**: Zero network requests other than standard Google quota endpoints.
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
