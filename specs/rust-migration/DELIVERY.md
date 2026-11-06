# Rust 版交付状态（2026-11-07）

分支 `rewrite/rust-gpui`（42 个提交），独立 worktree `/Users/neil/Code/NeoHub-Rust`，
代码在 `native/`。**未合入 `main`，未替换稳定版，Electron 源码完整保留。**

## 一句话结论

**核心能力已经可用并打包成 `.app`；功能面约 8 成到位，还有 10 项旧版功能未实现。**
可以本机日常使用，但还不能声称"功能完整、可以直接替换旧版"。

## 数字

| 项目 | 数值 |
|---|---|
| Rust 代码 | 106 个文件 / 约 3.1 万行 |
| 测试 | **410 passed / 0 failed**；clippy **0 warnings** |
| 提交 | 42（每阶段独立提交，范围明确） |
| 功能对照表 | 94 项：**已验证 46 / 部分 32 / 未开始 11 / 说明 5** |
| 打包 | `NeoHub.app` 29 MB、`NeoHub-1.4.0.dmg` 13 MB（ad-hoc 签名） |

## 已经能做什么

- 窗口 + 侧栏（分组/标签/排序/筛选/选中）、仪表盘（汇总卡、项目卡、端口 chip）
- 详情页：任务 tabs、单任务与项目级启停重启、日志尾、最近运行历史、编辑任务、环境变量
- 扫描目录导入项目（含 monorepo 一包一任务）、MCP 22 个工具全量可用
- PTY + 实时日志（ANSI 16/256/真彩、滚动、跟随、选择复制、导出可读/原始）
- 关窗后台继续、单实例锁、窗口位置记忆、Dock 显隐
- 旧版数据只读巡检与安全导入（`.import-staging` → 校验 → 文件级 rename）
- 设置（17 项）、三层环境变量、密钥 Keychain 加密、内置 MCP HTTP 服务（gate + 脱敏）

## 还没做（这 10 项 + 1 个遗留）

| # | 缺口 | 影响 |
|---|---|---|
| 1 | **NSStatusItem target/action 绑定** | 菜单栏图标点不开窗口 —— "菜单栏重开闭环"靠激活文件兜底，不是旧版的交互 |
| 2 | 登录启动 + 每应用 autoStart（S9） | 开机自动拉起与自动起服务不可用 |
| 3 | 应用菜单快捷键（S5：⌘, ⌘N ⌘⇧N 等） | 只能点界面 |
| 4 | 命令面板 ⌘K（U11） | 无快速操作入口 |
| 5 | 应用配置抽屉（U7） | 标签/文件夹/自动重启等只能在 MCP 里改 |
| 6 | Open In（S7：Finder/Terminal/VS Code…） | 详情页无跳转外部工具 |
| 7 | 桌面通知（S8：崩溃/就绪） | 后台失败无提示 |
| 8 | 深链接 `devharbor://`（S6） | 外部唤起失效 |
| 9 | 自动更新（S10） | 需手动换包 |
| 10 | 崩溃日志文件 + 打开日志目录（S11） | 崩溃无落盘日志 |
| 11 | `db:export/reset/path`（B4） | 危险区操作缺失 |

另有 32 项标"部分"（详见 `FEATURE-PARITY.md`），主要是 UI 细节（折叠、⋮ 菜单、拖拽分组、
sparkline、Toast 组件、焦点陷阱）与 `get_ports` 的 pid/process 字段。

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

## 建议的下一步顺序

1. 菜单栏绑定 + S9 登录启动（这两个决定"能不能替代旧版"）
2. U7 应用配置抽屉 + S5 快捷键（日常使用频率最高）
3. 其余（通知、Open In、深链接、更新、崩溃日志、db 危险区）
4. 真实数据迁移（需要你确认后执行）
