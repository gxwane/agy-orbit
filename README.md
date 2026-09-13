# agy-orbit (agyo)

[![CI](https://github.com/gxwane/agy-orbit/actions/workflows/ci.yml/badge.svg)](https://github.com/gxwane/agy-orbit/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

English | [简体中文](README_zh.md)

> 🪐 **Transactional Profile Manager & Lifetime Lease Supervisor for Antigravity CLI (`agy`).**
> 
> 专为 Google Antigravity CLI 设计的轻量级多账号事务管理与安全切换工具。

---

## 为什么需要 agy-orbit？

在日常使用 Antigravity CLI (`agy`) 进行高强度编码时：
- `agy` 原生不支持便捷的多账号切换与配额轮换。
- 全量目录镜像方案的局限性：若尝试通过整体复制或替换 `~/.gemini/antigravity-cli/` 目录来实现切号，因其包含数以万计的内部会话与日志文件（`brain/`），极易触发跨终端文件锁竞争并带来意外覆盖本地持久状态的风险。

**agy-orbit 遵循核心设计约束 (Core Design Invariants)**：
- **不破坏工作区与工具链**：不修改 `HOME` / `USERPROFILE`，不复制或扫描 `brain/`（会话记录）与 `plugins/`，不打断 Git、OpenSSH、Cargo、npm 等开发者环境。
- **精准管理三大认证标的**：仅管理 `oauth_creds.json`、`google_accounts.json` 与操作系统密钥环。
- **预写崩溃事务日志 (WAL Journal)**：四阶段原子状态机（`PREPARE -> APPLY -> VERIFY -> COMMIT`），断电/崩溃自动检测恢复。
- **平台原生加密**：Windows 原生 DPAPI / macOS Keychain Master-Key 加密保护，避免明文凭据落盘泄露。
- **生命周期排他租约 (Lifetime Lease)**：防止长期运行期间多终端凭据冲突，并在退出时自动反向捕获最新的 OAuth Refresh Token。
- **智能双模人机工效**：TTY 终端敲 `agyo` 直接唤起方向键盲切看板；脚本/管道自动静默降级为文本。
- **单二进制**：Rust 原生编写，切换耗时 < 5ms。

---

## 核心功能概览

- **`agyo save <orbit-name>`** (别名: `s`)：捕获当前登录的 Google 账号并加密归档为具名轨道。
- **`agyo use <orbit-name>`** (别名: `u`, `sw`)：全局切换当前活动轨道（WAL 事务日志与崩溃恢复保障）。
- **`agyo` (无参启动)**：交互式方向键选择菜单（TUI 导航，智能感知 TTY）。
- **`agyo list`** (别名: `ls`)：直观展示所有保存的轨道、对应邮箱、最后使用时间及当前激活项。
- **`agyo whoami`** (别名: `w`)：快速查看当前活动账号详情。
- **`agyo run <orbit-name> -- <cmd...>`** (别名: `r`)：以目标轨道启动并持有生命周期排他租约，退出时自动反向同步最新 Refresh Token。
- **`agyo quota`** (别名: `q`)：实时查询各账号模型的剩余请求配额与重置时间。
- **`agyo remove <orbit-name>`** (别名: `rm`)：安全移除指定轨道。
