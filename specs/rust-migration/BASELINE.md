# Rust + GPUI 迁移 —— 旧版验收基线（阶段 0）

> 采集人：执行代理（Reasonix）。
> 采集时间：2026-11-06T12:28Z – 12:35Z（UTC）。本机时区换算：UTC+8。
> 目的：给 `rewrite/rust-gpui` 重建提供可复现的起点。本文件只记录**实测**结果；
> 未实测的项目明确标注为缺失，不用推断值填充。

## 1. 仓库事实

| 项目 | 实测值 |
|---|---|
| 工作目录 | `/Users/neil/Code/NeoHub` |
| `git remote -v` | `origin ssh://git@ssh.github.com:443/neotu23/NeoHub.git`（fetch/push 同源） |
| `git rev-parse HEAD` | `02acedb290a7edad463a0497deff097e16bd4244` |
| `git log -5 --oneline` | `02acedb (HEAD -> main, tag: v1.4.0) feat: destroy the panel on close and restore it on reopen` / `0cd4a2e (tag: v1.3.0, origin/main) feat: close to the menubar instead of quitting` / `ca19cdf docs: document the unsigned macOS install path` / `d305719 docs: rewrite README in Chinese for NeoHub` / `bb8ca5f feat: rebrand to NeoHub with an English/Chinese UI` |
| 分支状态 | `## main...origin/main [ahead 1]` —— 本地 `main` 比 `origin/main` 多 1 个提交（`02acedb`） |
| `git fetch origin` | 成功，exit 0（2026-11-06T12:28:59Z）。远程 `refs/heads/main` = `0cd4a2ee…`，远程标签只有 `v1.3.0` |
| 远程确认 | `git ls-remote --heads origin` 只返回 `refs/heads/main`；与 `neotu23/NeoHub` 一致，无其他分支 |
| 本地未提交改动 | 仅 `?? docs/superpowers/plans/2026-11-06-neohub-rust-gpui-deepseek.md`（本计划文档，非用户业务改动）；没有已修改的被跟踪文件 |
| 已存在的迁移引用 | 无 `legacy/electron`、无 `electron-final-20261106`、无 `rewrite/rust-gpui`（本地与远端均无） |

本地 `v1.4.0` 标签从未推送到远端，`02acedb` 也尚未推送。阶段 1 的备份提交必须是
`02acedb` 之后包含本文档的提交，不能硬编码 `02acedb`。

## 2. 构建环境

| 项目 | 实测值 |
|---|---|
| OS | macOS 27.0（build 26A428），arch arm64，Apple M4 |
| Node | v22.22.3（`~/.local/bin/node`） |
| npm / pnpm | npm 10.9.8 / pnpm 11.1.2 |
| rustc / cargo | rustc 1.97.0 (2d8144b78 2026-07-07) / cargo 1.97.0 (c980f4866 2026-06-30) |
| `xcode-select -p` | `/Library/Developer/CommandLineTools`（**不是**完整 Xcode） |
| `xcrun --find metal` | **失败**（`unable to find utility "metal"`） |
| `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer xcrun --find metal` | 成功：`/var/run/com.apple.security.cryptexd/mnt/…/MetalToolchain-v27.1.266.1…/usr/bin/metal`；`xcodebuild -version` = Xcode 27.1 (27A9275) |
| Electron | `node_modules/electron` 33.x（`package.json` 声明 `^33.2.0`） |

**阶段 2 的前置条件**：GPUI 走 Metal，构建需要 Metal 工具链。当前活动开发者目录是
CommandLineTools，直接在 `native/` 里 `cargo build` 会找不到 `metal`。可行做法是构建时
导出 `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`（无需 sudo、不改系统设置），
这一点会在阶段 2 写进 `native/docs/DEPENDENCIES.md` 并实测；若仍失败，再向用户申请
`sudo xcode-select -s`。

## 3. 旧版质量门（实测退出码）

| 命令 | 退出码 | 结果摘要 |
|---|---|---|
| `pnpm typecheck` | 0 | `tsc --noEmit` node + web 两个 project 均通过 |
| `pnpm lint` | 0 | `eslint .`，无输出、无警告 |
| `pnpm test` | 0 | 28 个测试文件 / **202 个测试**全部通过，耗时 5.74s（vitest 2.1.9） |
| `pnpm build` | 0 | electron-vite 产出 `out/main/index.js` 276.45 kB、`out/preload/index.js` 2.88 kB、`out/renderer` bundle 1,510.93 kB + CSS 50.76 kB |

构建日志里只有两类无害提示：chokidar 的 `"Stats" is imported from external module "node:fs" but never used`（上游打包提示），
以及 `Browserslist: caniuse-lite is 6 months old`。**没有** TypeScript、lint 或测试失败，
因此本次基线没有"旧版实测缺陷"需要单列。

## 4. 功能清单来源

`FEATURE-PARITY.md` 的每一项都以源码/测试为依据：

- IPC 契约：`src/shared/ipc.ts` 实测 **59 个 invoke 频道 + 22 个 event 频道**（含 `panel:prepareClose` 的关窗保存握手）。
- 领域类型：`src/shared/types.ts`（`App`、`Task`、`ProcessState`= idle/starting/running/exiting/exited/crashed、`ReadinessSignal`= none/port/log/http/exit/delay、`NodeVersionPref`、`DetectionResult` 等）。
- 主进程服务：`src/main/services/*.ts`（36 个文件）、`src/main/db/migrations/0001–0007`、`src/main/mcp/*`。
- 渲染层：`src/renderer/components/*.tsx` 30 个组件 + `store/store.ts` + `lib/{DebouncedSaveQueue,panelSaves}.ts` + `i18n/locales/{en,zh-CN}.ts`。
- 测试样本：`src/**/__tests__/*.test.ts` 共 28 个文件，其中 `mcp/tools.test.ts`（51 例）、`gate.test.ts`（10 例）、`hygiene.test.ts`（9 例）、`AppOrchestrator.e2e.test.ts`（真实 PTY 启停）是行为断言的直接依据。
- MCP 工具：`src/main/mcp/tools.ts` 实测 22 个 `tool(...)` 注册。

`specs/PROGRESS.md` 的过时处（本次复核发现）：该文件最后一次同步是 2026-07-10，
但它把 Phase 9 之后的多轮改动（rebrand → NeoHub、菜单栏常驻、面板销毁重建、批量导入、
全局日志搜索、崩溃/就绪通知、登录启动、每应用 autoStart）都写在正文而不在矩阵里，
且矩阵仍用旧名 DevHarbor。**本次迁移以源码为准**，不按该矩阵删功能。

## 5. 视觉基线截图

截图目录：`specs/rust-migration/screenshots/`。全部使用**合成数据**
（`/tmp/neohub-synth` 下的假项目路径 `demo-web` / `demo-worker` / `demo-api`），
不含真实用户项目名、路径或密钥。

| 文件 | 内容 |
|---|---|
| `dashboard.png` | 仪表盘：3 个应用（2 运行中）、CPU/内存/端口汇总卡、文件夹分组 `Synthetic`、标签筛选、中文 UI、深色主题、端口 chip `web :4319` |
| `app.png` | 项目详情：任务 tabs（`web` / `mock api`）、任务端口 chip、实时终端（真实 xterm + 合成日志、中文字符）、复制/保存/清空、重启/停止、Open In |
| `settings.png` | 设置抽屉：通用（主题/语言/自动更新/登录启动/菜单栏图标）、通知、仪表盘采样间隔 |
| `empty.png` | 空态首启页（欢迎 + 4 个能力说明 + 添加/导入入口） |
| `light.png` | 浅色主题下的仪表盘 |

**采集方式与限制（必须如实记录）：**

- `screencapture -x` 在本会话失败（`could not create image from display`）——进程缺少
  屏幕录制权限，因此无法用常规截屏。
- 替代做法：用仓库自带的 Electron 运行时加载 `out/renderer`（真实 React 组件、真实
  Tailwind/主题 CSS），用 `/tmp` 下的合成数据 preload 替换 IPC 桥，再
  `webContents.capturePage()` 取图。**这是真实渲染管线产出的像素，但数据是合成的**，
  不能算作"真实运行验证"。
- **未能完成**：用真实旧版进程跑通"导入合成项目 → 启动 → 实时日志 → 关窗 → 菜单栏重开"。
  原因是旧版在 `src/main/index.ts` 里无条件执行
  `app.setPath('userData', resolveUserDataDirectory(app.getPath('appData')))`，
  `--user-data-dir` 会被覆盖，`HOME` 覆盖对它无效（实测 `app.getPath('appData')` 仍为
  `/Users/neil/Library/Application Support`）。而该目录下已存在**真实用户数据目录**
  `~/Library/Application Support/devharbor`（13 MB，含 114 KB `devharbor.db`），
  按"不得让开发流程读写生产数据库"的约束，本阶段**没有**启动真实旧版进程。
  该闭环的行为基线改由源码 + 202 个测试 + 截图表达；真实进程级对照留给阶段 16
  （届时用两侧各自的隔离数据目录，并在执行前把隔离方案报给用户确认）。

## 6. 资源基线

| 项目 | 实测值 | 说明 |
|---|---|---|
| `dist/mac-arm64/NeoHub.app` | **265 MB** | 未签名 arm64 1.4.0 构建（`pnpm pack:mac` 产物，构建于 2026-11-05） |
| `dist/NeoHub-1.4.0-arm64.dmg` | **104 MB** | 同上 |
| `Contents/Frameworks/Electron Framework.framework` | 231 MB | Chromium/V8，不可压缩的主因 |
| `Contents/Frameworks/{Squirrel,Mantle,ReactiveObjC}` | 276 KB / 96 KB / 372 KB | 自动更新与 ObjC 桥 |
| `Contents/Resources/app.asar` | 15 MB | 应用代码 + 生产依赖 |
| 运行期 footprint / RSS / CPU / 唤醒 / 启动时间 | **缺失** | 见下 |

**运行期资源基线缺失原因**：测量需要真实启动发布构建进程，而启动会写真实
`~/Library/Application Support/devharbor`（同上第 5 节的隔离问题）。计划阶段 16 要求
"同一台 Mac、同系统、相同负载、prewarm 30s、每状态 ≥3 轮 × 60s"，那一次必须在一个
安全隔离（或用户明确授权的独立账户/数据目录）下同时测 Electron 与 Rust，所以这里先只
锁定**体积基线**这一项可测数据，运行期数字留到阶段 16 补齐，不用估算值。

## 7. 本阶段结论

- 起点可复现：HEAD、远程、分支、未提交状态均已记录，`git fetch` 成功且远端就是 `neotu23/NeoHub`。
- 质量门全绿（typecheck / lint / 202 tests / build），记录的是真实退出码。
- 功能清单有源码依据（59 invoke + 22 event + 22 MCP 工具 + 30 个渲染组件）。
- 视觉基线有 5 张合成数据截图，采集方式与限制已写明。
- **没有**创建任何 Rust 产品代码；**没有**把"文档写好了"当作备份完成。
- 两个需要在后续阶段处理的实测限制：① Metal 工具链需要 `DEVELOPER_DIR`；
  ② 旧版 GUI 进程级验证与运行期资源测量需要先解决 userData 隔离。
