# agy-orbit (agyo)

[English](README.md) | 简体中文

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust: 1.80+](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](Cargo.toml)

> 🪐 **面向 Google Antigravity CLI (`agy`) 的轻量级多账号事务管理与排他租约监护工具。**

---

## 核心痛点与工程承诺

在日常高强度使用 Antigravity CLI (`agy`) 编程时：
- `agy` 官方尚未原生支持便捷的多账号切换与配额轮换（Issue #381 仍未合并）。
- 全量目录镜像方案的局限性：若尝试通过整体复制或替换 `~/.gemini/antigravity-cli/` 目录来实现切号，因其包含数以万计的内部会话与日志文件（`brain/`），极易触发跨终端文件锁竞争并带来意外覆盖本地持久状态的风险。

**agy-orbit 遵循核心设计约束 (Core Design Invariants)**：
- **不触碰运行时目录**：不修改 `HOME` / `USERPROFILE`，不复制、不扫描、不删除 `brain/`（会话记录）与 `plugins/`，不打断 Git、OpenSSH、Cargo、npm 等外部工具链。
- **精准管理三大认证标的**：仅管理 `~/.gemini/oauth_creds.json`、`~/.gemini/google_accounts.json` 与系统密钥环。
- **三态解耦物理拓扑**：靶标系统（`~/.gemini/`）保持最小介入；工具自身数据库收敛在独立的 `~/.agyo/`；租约锁放置在本地易失内存目录，避免网盘同步死锁。
- **预写崩溃事务日志 (WAL)**：四阶段原子状态机（`PREPARE -> APPLY -> VERIFY -> COMMIT`），断电/崩溃自动检测恢复。
- **平台原生加密**：Windows 原生 DPAPI / macOS Keychain 加密保护，避免明文凭据落盘泄露。
- **生命周期排他租约 (Lifetime Lease)**：防止长期运行期间多终端凭据冲突，并在退出时自动反向捕获最新的 OAuth Refresh Token。
- **智能双模人机工效**：TTY 终端敲 `agyo` 直接唤起看板与方向键盲切菜单；脚本与管道中自动静默降级为单行文本。

---

## 命令总览

| 命令 | 别名 | 功能说明 |
| :--- | :--- | :--- |
| `agyo save <orbit>` | `s` | 将当前登录的 Google 账号加密快照归档为指定轨道 |
| `agyo use <orbit>` | `u`, `sw` | 全局切换至目标轨道（WAL 崩溃恢复保护） |
| `agyo` (无参) | - | 交互式看板与上下方向键 TUI 切换菜单（智能环境感知） |
| `agyo list` | `ls` | 格式化表格展示所有已保存轨道、邮箱、标签及激活状态 |
| `agyo whoami` | `w` | 查看当前系统中活跃的 Google 账号详情 |
| `agyo run <orbit> -- <cmd...>` | `r` | 持有生命周期排他租约运行子进程，退出时反向同步最新 Token |
| `agyo quota [orbit] [-a] [-r]` | `q` | 查询各模型池配额水位（支持单账号或 `-a` 全量多账号聚合大盘与倒计时） |
| `agyo remove <orbit>` | `rm` | 安全删除指定轨道及其快照 |

---

## 质量检查

本项目全面遵循 Clean Craftsmanship 质量工程体系，所有提交必须 100% 通过自动化质量检查：
```powershell
# Windows
powershell -ExecutionPolicy Bypass -File .\scripts\verify_gauntlet.ps1

# Linux / macOS
./scripts/verify_gauntlet.sh
```

---

## 许可证

本项目基于 [MIT 许可证](LICENSE) 开源。
