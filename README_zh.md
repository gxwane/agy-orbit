# agy-orbit (agyo)

[![CI](https://github.com/gxwane/agy-orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/gxwane/agy-orbit/actions)
[![Crates.io](https://img.shields.io/crates/v/agy-orbit.svg)](https://crates.io/crates/agy-orbit)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.98+](https://img.shields.io/badge/Rust-1.98%2B-orange.svg)](Cargo.toml)

[English](README.md) | **简体中文**

> 🪐 **面向 Google Antigravity CLI (`agy`) 的轻量级多账号事务管理与排他租约监护工具。**
> 
> 毫秒级（<5ms）切换账号与用量概览；坚持最小介入面设计，不扫描、不覆盖会话历史（`brain/`）与扩展插件。

---

## 💡 为什么需要 agy-orbit？

在日常高强度使用 Google Antigravity CLI (`agy`) 进行编程时：
- `agy` 官方尚未原生支持便捷的多账号切换与配额轮换（官方 Issue #381 仍未合并）。
- **全量目录镜像方案的局限性**：若尝试通过整体复制或替换 `~/.gemini/antigravity-cli/` 目录来实现切号，由于该目录包含大量动态会话记录（`brain/`）、插件生态及进程锁文件，粗粒度的全量镜像不仅产生极高磁盘 I/O 开销，还容易导致用户本地会话数据非预期丢失，并频繁在 Windows 平台触发文件独占锁冲突。

### 🛡️ 核心设计约束 (Core Design Invariants)

`agy-orbit` 遵循明确且具备可验证性的工程约束：
1. **最小介入面（Narrow Surface）**：不触碰、不扫描、不替换 `brain/`（会话记录）与 `plugins/`，不修改全局 `HOME` / `USERPROFILE`，保障外部开发工具链的独立与稳定。
2. **精准管理三大认证标的**：
   - 标的 ①：`~/.gemini/oauth_creds.json`（活动 OAuth 会话令牌）
   - 标的 ②：`~/.gemini/google_accounts.json`（账户映射表）
   - 标的 ③：系统级密钥环（`gemini:antigravity`）
3. **三态解耦物理拓扑**：
   - **靶标平面 (Target Plane)**：`~/.gemini/`（最小介入面，仅读写当前活动凭据）。
   - **存储平面 (Storage & State Plane)**：`~/.agyo/`（独立的加密存储目录，可轻松独立备份）。
   - **易失运行期平面 (Runtime Plane)**：Windows `%LOCALAPPDATA%\agy-orbit\run` / Unix `$XDG_RUNTIME_DIR/agyo`（基于 tmpfs 内存文件系统，断电后自动清除，不会被网盘同步，避免跨机器残留锁）。
4. **预写崩溃事务日志 (WAL)**：四阶段原子状态机（`PREPARE -> APPLY -> VERIFY -> COMMIT`），断电或崩溃启动后自动回滚恢复。
5. **平台原生加密**：Windows 原生 DPAPI (`CryptProtectData`) / POSIX AES-256-GCM 认证加密，避免明文凭据写入磁盘。
6. **生命周期排他租约 (Lifetime Lease) 与反向同步 (Two-Way Sync)**：
   - `agyo run <orbit> -- agy` 在子进程生命周期内全程持有内核排他锁，防止运行期跨终端凭据冲突；
   - 子进程退出时，自动捕获运行期刷新的最新 Token 回写 Orbit 存储，避免用旧快照覆盖新令牌。
7. **智能双模人机工效**：交互式 TTY 敲 `agyo` 直接唤起看板与方向键 TUI 切换；脚本与管道（`agyo | grep`）自动静默降级为单行纯文本。

---

## 📦 安装指南

### 模式一：网络一键自动化安装（推荐）

通过官方生产级安装脚本自动完成架构适配、SHA-256 哈希校验与系统 PATH 配置：

- **Windows (PowerShell)**：
  ```powershell
  irm https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/install.ps1 | iex
  ```
- **macOS 与 Linux (Bash)**：
  ```bash
  curl -fsSL https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/install.sh | bash
  ```

### 模式二：预编译独立二进制下载

可直接在 [GitHub Releases](https://github.com/gxwane/agy-orbit/releases) 下载对应平台的免编译压缩包，解压后将 `agyo`（Windows 为 `agyo.exe`）放置到系统 `PATH` 目录：
- **Windows (x86_64 MSVC)**: `agyo-x86_64-pc-windows-msvc.zip`
- **macOS (Apple Silicon)**: `agyo-aarch64-apple-darwin.tar.gz`
- **macOS (Intel x86_64)**: `agyo-x86_64-apple-darwin.tar.gz`
- **Linux (x86_64 glibc)**: `agyo-x86_64-unknown-linux-gnu.tar.gz`

> 💡 安装完成后，后续可直接执行 `agyo upgrade` 进行无缝原地自更新。

### 模式三：通过 Cargo 源码编译安装

```bash
# 从 crates.io 安装
cargo install agy-orbit

# 或克隆仓库后本地编译安装
git clone https://github.com/gxwane/agy-orbit.git
cd agy-orbit
cargo install --path .
```

#### Linux 系统前置依赖
在 Linux 系统上，`agy-orbit` 依赖系统的 SecretService (D-Bus) 与 Keyring 基础开发库。从源码编译前需确保已安装：
```bash
# Ubuntu / Debian
sudo apt-get install -y pkg-config libsecret-1-dev libdbus-1-dev

# Fedora / RHEL
sudo dnf install -y pkgconf libsecret-devel dbus-devel

# Arch Linux
sudo pacman -S --needed pkgconf libsecret dbus
```

---

## 🚀 快速上手

```bash
# 1. 使用官方 Antigravity 登录第一个账号
agy auth login

# 2. 快照当前凭据并保存为具名 Orbit
agyo save personal -l "个人 Gmail"

# 3. 登录第二个账号
agy auth login

# 4. 快照为另一个 Orbit
agyo save work -l "企业 Workspace"

# 5. 列出所有保存的 Orbit 轨道
agyo list

# 6. 全局原子切换活动账号
agyo use work

# 7. 以指定轨道启动独立会话（持有排他租约并在退出时反向同步 Token）
agyo run personal -- agy

# 8. 查看所有账号的实时模型用量大盘与倒计时
agyo quota --all

# 9. 随时对本地环境、凭据与网络连通性进行全面体检
agyo doctor
```

---

## 📖 CLI 完整命令手册

| 命令 | 别名 | 功能说明 |
| :--- | :---: | :--- |
| `agyo` *(无参)* | - | 交互式看板与上下方向键 TUI 切换菜单（智能感知 TTY） |
| `agyo save <orbit>` | `s` | 快照当前凭据为指定轨道（支持 `-l, --label`, `-f, --force`） |
| `agyo use <orbit>` | `u`, `sw` | 全局原子切换至目标轨道（WAL 崩溃恢复保护） |
| `agyo list` | `ls` | 格式化表格展示所有轨道、邮箱、标签及激活状态 |
| `agyo whoami` | `w` | 查看当前系统中活跃的 Google 账号与密钥环详情 |
| `agyo run <orbit> [-- <cmd...>]`| `r` | 持有生命周期排他租约运行子进程，退出时反向同步最新 Token（支持 `--restore`） |
| `agyo quota [orbit]` | `q` | 查询各模型池配额与重置倒计时（支持 `-a, --all`, `-r, --refresh`） |
| `agyo remove <orbit>` | `rm` | 安全删除指定轨道及其加密快照 |
| `agyo doctor` | `doc`, `dr` | 运行零突变五维健康巡检与网络连通性诊断（支持 `-o, --offline`） |
| `agyo completion [shell]` | `comp`| 生成 Shell 自动补全脚本（支持 `--raw`，涵盖 bash, zsh, fish, powershell, elvish） |
| `agyo upgrade` | `update`, `up` | 检查新版本并原地安全自更新（支持 `-c, --check`, `-f, --force`, `-p`） |
| `agyo uninstall` | `purge` | 安全卸载 agy-orbit 并清理运行时数据（支持 `-y, --yes`, `--keep-vault`, `--dry-run`, `--delete-self`） |

### 关键参数与选项

#### `agyo save <NAME>`
- `-l, --label <STRING>`：可选的可读描述标签（如 `"Work Pro Plan"`）。
- `-f, --force`：若同名轨道已存在，直接强制覆盖而不进行交互确认。

#### `agyo run <NAME> [--restore] [-- <CMD...>]`
- `--restore`：子进程退出并完成 Token 反向同步后，自动还原启动前的上一轨道凭据。
- `cmd`：要执行的子命令及参数（默认直接执行 `agy`）。

#### `agyo quota [NAME]`
- `-a, --all`：查询并展示全量账号聚合配额大盘（与 `[NAME]` 互斥）。
- `-r, --refresh`：穿透 60 秒本地安全缓存，强制请求远端最新数据。

#### `agyo completion [SHELL]`
- `[SHELL]`：目标 Shell 类型（`bash`, `zsh`, `fish`, `powershell`, `elvish`）。在交互终端中省略将自动探测当前环境。
- `--raw`：仅输出原始补全脚本，不包含配置指引文字。

#### `agyo doctor`（别名：`doc`, `dr`）
- `-o, --offline`：跳过 Google 远端端点探测，仅执行本地环境、凭据格式与存储状态巡检。

#### `agyo upgrade`
- `-c, --check`：仅检查是否有新版本，不执行下载与安装。
- `-f, --force`：即使当前已是最新版，也强制重新下载并覆盖。
- `-p, --include-prereleases`：检查并允许更新至先行版本（Alpha / Beta / RC）。

#### `agyo uninstall`（别名：`purge`）
- `-y, --yes`：跳过交互式二次确认，直接执行卸载。
- `--dry-run`：仅预览将被清理与保留的资源清单，不进行任何实际删除。
- `--keep-vault`（别名 `--keep-data`）：保留多账号加密快照数据（`~/.agyo/orbits/`）。
- `--delete-self`：尝试删除当前正在运行的 `agyo` 二进制本身（安全规避 OS 进程文件锁）。

---

## 🐚 Shell 自动补全配置

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

## 🗑️ 卸载指南 (Uninstallation)

`agy-orbit` 不会注册开机自启、驻留系统守护进程或篡改第三方开发环境。若您需要卸载与清理，可根据实际使用场景自由选择以下模式：

### 模式一：内置原生 CLI 命令（推荐 — 离线零依赖）

`agyo` 内置原子卸载功能，在排他生命周期租约保护下安全清理，防止运行时切号冲突：

```bash
# 交互式安全卸载（带有操作二次确认与影响范围提示）
agyo uninstall

# 自动化脚本无头全量清理（同时自动移除可执行文件）
agyo uninstall -y --delete-self

# 仅清理缓存与临时运行时文件，完整保留多账号加密存储
agyo uninstall --keep-vault

# 试运行（仅预览受影响的文件路径，不执行物理删除）
agyo uninstall --dry-run
```

### 模式二：网络一键脚本 / Release 随包独立脚本

若可执行文件已被移走，或希望脱离 CLI 二进制单独清理：

- **Windows (PowerShell)**：
  ```powershell
  # 远程一键卸载脚本
  irm https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/uninstall.ps1 | iex

  # 或运行官方 Release 压缩包内附带的离线卸载脚本
  powershell -ExecutionPolicy Bypass -File .\scripts\uninstall.ps1
  ```
- **macOS 与 Linux (Bash)**：
  ```bash
  # 远程一键卸载脚本
  curl -fsSL https://raw.githubusercontent.com/gxwane/agy-orbit/master/scripts/uninstall.sh | bash

  # 或运行官方 Release 压缩包内附带的离线卸载脚本
  ./scripts/uninstall.sh
  ```

### 模式三：透明原生 Shell 命令（手动）

- **Windows (PowerShell)**：
  ```powershell
  # 1. 移除二进制及 PATH 目录
  Remove-Item -Force "$HOME\.agyo\bin\agyo.exe" -ErrorAction SilentlyContinue

  # 2. 删除加密数据目录（若需保留账号可跳过此步）
  Remove-Item -Recurse -Force "$HOME\.agyo" -ErrorAction SilentlyContinue

  # 3. 清理运行期易失锁目录
  Remove-Item -Recurse -Force "$env:LOCALAPPDATA\agy-orbit" -ErrorAction SilentlyContinue
  ```
- **macOS 与 Linux (Bash)**：
  ```bash
  # 1. 移除可执行二进制
  rm -f ~/.local/bin/agyo /usr/local/bin/agyo

  # 2. 删除加密数据目录（若需保留账号可跳过此步）
  rm -rf ~/.agyo

  # 3. 清理运行期易失锁目录
  rm -rf "${XDG_RUNTIME_DIR:-/tmp}/agyo" 2>/dev/null || true
  rm -rf "${TMPDIR:-/tmp}/agyo-run-$(id -u)" 2>/dev/null || true
  ```

### 清理 Shell 自动补全配置

若您此前配置过 Shell 补全，请移除配置文件中的对应行，避免新终端启动时报错：
- **PowerShell**：打开 `$PROFILE`，移除包含 `agyo completion` 或 `.agyo\completion.ps1` 的行；
- **Bash / Zsh**：从 `~/.bashrc` 或 `~/.zshrc` 中移除 `agyo completion` 行，或删除 `~/.local/share/bash-completion/completions/agyo`；
- **Fish**：删除 `~/.config/fish/completions/agyo.fish`。

> [!NOTE]
> **关于 Google Antigravity 官方凭据独立性**  
> `agy-orbit` 遵循严格工程承诺（The Contractual Invariant）。卸载 `agyo` **绝不会**触碰、注销或删除当前正在被 Google Antigravity 使用的官方凭据（`~/.gemini/` 及系统密钥环中的当前会话）。卸载后官方 `agy` 会话仍将完全保持登录状态；若需从本机彻底登出 Google 账号，请直接执行官方命令：`agy auth logout`。

---

## 🔧 环境变量一览

| 环境变量 | 默认值 | 作用说明 |
| :--- | :--- | :--- |
| `AGYO_HOME` | `~/.agyo/` | 自定义持久化 Orbit 存储根目录 |
| `GEMINI_HOME`| `~/.gemini/` | 自定义目标 Antigravity 配置目录 |
| `AGYO_RUNTIME_DIR` | Windows: `%LOCALAPPDATA%\agy-orbit\run`<br>Unix: `$XDG_RUNTIME_DIR/agyo` | 易失瞬态锁与租约目录 |
| `AGYO_KEYRING_TARGET` | `LegacyGeneric:target=gemini:antigravity` | Windows 凭据管理器目标条目名 |
| `AGYO_KEYRING_SERVICE`| `gemini` | Linux SecretService / macOS Keychain 服务标识 |

---

## 🛡️ 安全设计约束与反病毒透明度规范 (Security Invariants)

为保持在操作系统与现代 EDR / 防病毒环境中的行为高度透明，`agyo` 严格遵循以下边界：
1. **仅访问审计标的 (Scoped Credential Access)**：仅精确读取单个合法目标 `gemini:antigravity`，不枚举或扫描系统其他凭据。
2. **零进程注入 (No Process Injection)**：不调用 `CreateRemoteThread` 或向子进程注入动态库。
3. **官方标准调用 (Documented System APIs Only)**：完全基于操作系统官方文档化的系统调用。
4. **零加壳压缩 (No Binary Packing)**：零加壳（不用 UPX/Themida），保持 Rust 确定性原生二进制。
5. **零非必要网络交互 (Zero Outbound Telemetry)**：不向任何外部网络上传 Token、Secret 或会话数据。
6. **零静默持久化 (No Silent Persistence)**：不写入任何自启动注册表项或创建隐蔽后台驻留服务。
7. **只读配额凭据不变量 (Read-Only Quota Invariant)**：`agyo quota` 仅读取现有有效 access token，不自行刷新 Token，避免风控与 Token 竞争吊销。

---

## 🤝 贡献指南

欢迎参与贡献！提交 Pull Request 前请确保 100% 通过本地质量检查：

```powershell
# Windows
powershell -ExecutionPolicy Bypass -File .\scripts\verify_gauntlet.ps1

# Linux / macOS
./scripts/verify_gauntlet.sh
```

请阅读并遵守 [行为准则 (Code of Conduct)](CODE_OF_CONDUCT.md)。

---

## 📄 许可证

本项目基于 [MIT 许可证](LICENSE) 开源。
