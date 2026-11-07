# Rust 版交付状态（2026-11-07）

分支 `rewrite/rust-gpui`，独立 worktree `/Users/neil/Code/NeoHub-Rust`，代码在 `native/`。
**未合入 `main`，未替换稳定版，Electron 源码完整保留** —— 按你的决定保持分支隔离。

## 一句话结论

**旧版的功能面已经对齐**：94 项对照里 **84 项已验证、4 项部分、1 项不适用、5 项说明性
条目**。剩下的四项各有明确原因（见下），不是"还没做"，而是"需要额外前提或大改动"。

## 数字

| 项目 | 数值 |
|---|---|
| Rust 代码 | 125 个文件 / 约 3.86 万行 |
| 测试 | **553 passed / 0 failed**；clippy **0 warnings** |
| 提交 | 82（每阶段独立提交，范围明确） |
| 功能对照表 | 94 项：**已验证 84 / 部分 4 / 不适用 1 / 说明 5** |
| 打包 | `NeoHub.app` 31 MB、`NeoHub-1.4.0.dmg` 13 MB（ad-hoc 签名，带"产物不得落后于源码"自检） |

## 已经能做什么

- 窗口 + 侧栏（分组/标签/排序/筛选/选中）、仪表盘（汇总卡、项目卡、端口 chip）
- 详情页：任务 tabs、单任务与项目级启停重启、日志尾、最近运行历史、编辑任务、环境变量
- 扫描目录导入项目（含 monorepo 一包一任务）、MCP 22 个工具全量可用
- PTY + 实时日志（ANSI 16/256/真彩、滚动、跟随、选择复制、导出可读/原始）
- 关窗后台继续、单实例锁、窗口位置记忆、Dock 显隐
- 旧版数据只读巡检与安全导入（`.import-staging` → 校验 → 文件级 rename）
- 设置（18 项）、三层环境变量（**行内编辑**）、密钥 Keychain 加密、内置 MCP HTTP 服务（gate + 脱敏）
- 菜单栏（聚合状态、逐项启停、随快照刷新）、登录启动（LaunchAgent）、顶部应用菜单
- 命令面板 ⌘K、快捷键（⌘, ⌘N ⌘P ⌘⇧F ⌘R）、每行 ⋮ 菜单、分组折叠、空态引导
- 崩溃卡片（冻结最后 200 行、端口冲突识别、重试）、就绪/崩溃通知、更新检查与"忽略此版本"
- 数据库危险区（导出/重置/路径）、应用日志文件（含 panic 捕获）、Open In、批量导入

## 剩下的四项"部分"

| # | 缺口 | 为什么留着 | 需要什么 |
|---|---|---|---|
| 1 | **日志虚拟化 + 高亮**（L6） | 现在把可见行一次性渲染成元素；几千行会变慢 | 一次渲染层重构：虚拟滚动 + 搜索高亮，属于工程量大但无外部依赖的活 |
| 2 | **深链接的 Apple Event 接收**（S6） | `Info.plist` 已注册 scheme、链接会被解析，但把 URL 送进运行中的应用要走 `NSAppleEventManager` + ObjC target | 手写一段 ObjC 绑定；风险集中在"点击后崩溃"这类只能在真机验证的问题 |
| 3 | **`enc1:` 密钥重录**（B6） | 旧版的 `safeStorage` 密文只有旧版进程能解 | 旧版加一个 opt-in 导出入口，或者用户手动重录 |
| 4 | **ErrorBoundary**（U13） | GPUI 渲染期 panic 的实际行为我还没有验证过 | 先写一个会 panic 的探针确认 GPUI 是 unwind 还是 abort，再决定这层兜底有没有意义 |

一项标"不适用"：`D6`（旧版的 59 invoke + 22 event IPC 频道）—— Rust 版是同进程调用，
用 `CoreCommand`/`CoreEvent` 取代，不存在 preload 边界。

## 其他已知差异

- **TLS/SSE**：MCP 端点只支持 `POST /mcp` 的 JSON 响应；默认 loopback + Bearer + 默认关闭
- **公证**：无 Developer ID 证书，只能 ad-hoc 签名，分发到其他 Mac 会被 Gatekeeper 拒绝
- **架构**：只构建 arm64
- **`enc1:` 密钥**：原样复制并计数，需要重录或用旧版导出
- **真实数据迁移未执行**：工具与步骤就绪（`native/docs/MIGRATION.md`），等你确认后执行

## 怎么用

```bash
cd native
cargo test --workspace --locked     # 410 tests
bash scripts/package.sh             # -> dist/NeoHub.app, dist/NeoHub-1.4.0.dmg
bash scripts/bench.sh               # 启动 82 ms / RSS 135 MB
open dist/NeoHub.app                # 默认数据目录 ~/Library/Application Support/NeoHub-Rust-Dev
```

默认**不碰**旧版数据。要试用旧数据，先按 `MIGRATION.md` 走"预览 → 确认 → 导入"。

## 回退

Rust 版与旧版完全独立：

1. 退出 Rust 版（⌘Q 或 `pkill neohub-desktop`）
2. 继续用 `legacy/electron` 分支或 `electron-final-20261106` 标签构建的旧版
3. 旧版 userData **未被修改**（数据库 sha256 与阶段 1 冷备份一致，见 `PROGRESS.md` 的安全事件记录）
4. 冷备份：`~/Backups/NeoHub-Migration-20261106/`（bundle + userData）

## 建议的下一步

1. **真实数据迁移**：按 `native/docs/MIGRATION.md` 的"预览 → 确认 → 导入"走一遍，
   这是唯一还没做的验证；工具与步骤已就绪，源库只读且有测试保证字节不变
2. **人眼确认**：菜单栏图标、⌘K、行内编辑、通知横幅、拖拽窗口后的终端重排 —— 这些都无法自动化
3. 之后若继续投入：`L6` 虚拟化（工程量大但无外部依赖）> `U13`（先写 panic 探针）>
   `S6`（ObjC 绑定）> `B6`（需要旧版配合）

## 本轮之前的验证快照

| 检查 | 结果 |
|---|---|
| `cargo test --workspace --locked` | 553 passed / 0 failed |
| `cargo clippy --workspace --all-targets` | 0 warnings |
| `bash scripts/package.sh` | 31 MB app / 13 MB dmg，ad-hoc 签名，产物与源码一致自检通过 |
| 启动（临时 `--data-dir`） | 进程存活、窗口 onscreen、数据库初始化、日志无 error/panic |
| 应用日志 | `~/Library/Logs/NeoHub/neohub.log` 记录启动与数据目录 |
