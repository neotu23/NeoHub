# NeoHub Rust + GPUI 重建执行计划（DeepSeek 执行版）

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task if available. DeepSeek 若没有该技能，直接按本文顺序执行，使用复选框和进度记录恢复上下文；不依赖特定插件，也不要求启用多代理。

**Goal:** 完整备份现有 Electron 版 NeoHub，在同一 GitHub 仓库的独立分支中，以 Rust + GPUI 重建客户端，保留现有功能和视觉风格，并实测自身资源占用。

**Architecture:** 同仓库保留旧版分支和不可移动的版本标签，使用独立 worktree 开发。Rust 版使用单进程后台核心、GPUI 界面与窄范围 macOS 平台适配；窗口销毁后核心继续运行。现有 Node.js 项目仍由各自的 Node.js 运行时执行。

**Tech Stack:** Rust、GPUI、匹配版本的 Longbridge 组件库、Tokio、SQLite、PTY、Rust MCP SDK；版本与特性在阶段 2 用可编译验证结果锁定。

**Spec:** 本文第 1—6 节为本次重建的设计基线；现有功能细节参照 `specs/03-features.md`、`src/shared/types.ts`、`src/shared/ipc.ts` 及本文源码映射。旧架构文档的 Electron 技术选择不约束 Rust 重建分支。

**文档日期：** 2026-11-06。本文是执行方案，编写本文不代表已完成备份、推送、分支创建或 Rust 重建。

## Global Constraints

- 当前版本是 Electron + React + TypeScript，不是 Next.js。不得按 Next.js 桌面壳迁移。
- 远程仓库继续使用 `neotu23/NeoHub`，不新建仓库；Rust 工作分支为 `rewrite/rust-gpui`。
- 保留产品名 NeoHub、现有图标和现有许可证及作者声明；不要把整个项目改成 GPUI 自身的许可证。
- 第一个发布目标是 macOS Apple Silicon；Intel、Windows、Linux不作为首版验收条件。
- 完整 GPUI 界面，不使用 Electron、Chromium、WebView 或常驻 Node.js 辅助服务实现 Rust 版功能。
- 不把被管理的 Node.js 项目改写为 Rust，也不宣称迁移可以合并这些项目的 Node.js 内存。
- 必须保留关闭窗口后继续运行任务、隐藏 Dock、菜单栏重开、恢复选中项目和任务的行为。
- 迁移开发使用独立数据目录；不得让两版同时读写生产数据库、自动启动同一批真实项目或争抢 MCP 端口。
- 不在备份、Git、日志、测试截图和报告中包含明文环境变量、Token、Keychain 密钥。
- 功能不完整必须明确记录；不能用静态数据、空函数、固定返回成功替代真实实现。
- 默认顺序执行，每个阶段验证通过并更新进度后继续；需要系统能力或依赖尚不可用时准确报告阻塞，不擅自更换为 WebView。

## Review Focus

1. **关闭窗口时仍有未保存编辑或启动请求：** 保存失败时保留窗口；成功关闭不取消后台启动，也不重复启动任务。阶段 5、10、12验证。
2. **旧数据库包含 WAL、旧品牌路径和加密字段：** 备份一致、来源可追溯、重复导入不覆盖新数据，解密失败不静默写回密文。阶段 1、3、4、13验证。
3. **PTY 输出跨 UTF-8/ANSI 边界并持续高吞吐：** 日志不损坏、不无界排队，慢界面可从核心恢复且不重复。阶段 6、11验证。
4. **停止与启动竞态、子进程及 PID 复用：** 同一任务不会双开，停止整个受管进程树，绝不结束 NeoHub 自身或无关进程。阶段 5、7验证。
5. **后台窗口不存在时收到 MCP、菜单栏、深链接：** 使用同一份核心状态，正确重开窗口；未经确认的外部深链接不执行命令。阶段 8、12、14验证。

---

## 0. 给 DeepSeek 的启动提示词

将以下内容连同本文件一起提供给执行者：

```text
你正在维护 /Users/neil/Code/NeoHub，远程仓库是 neotu23/NeoHub。
先完整阅读 docs/superpowers/plans/2026-11-06-neohub-rust-gpui-deepseek.md。
任务是按文档备份旧版，再用 Rust + GPUI 完整重建 NeoHub；不是讨论是否可行。

从阶段 0 开始，按依赖顺序连续推进。备份验证成功后才能进入重建。
不要新建 GitHub 仓库，不要覆盖 main，不要删除 Electron 源码，不要 force push。
在 rewrite/rust-gpui 的独立 worktree 开发，Rust 文件统一放在 native/。
不要执行真实用户项目作为自动化测试，不要读取或输出真实密钥。
先使用合成测试项目和独立数据目录；实际数据导入走专门迁移流程。

先做出“真实 GPUI 窗口 → 导入合成项目 → PTY 启动 → 实时终端日志 →
关闭窗口后台继续 → 菜单栏重开”的运行闭环，再补齐全部功能。
不允许用 WebView、静态演示或常驻 Node 辅助进程交付 Rust 版。
不能照猜过时 GPUI API；先查官方示例，用 cargo check 和实机启动锁定组合。

每完成一个阶段：运行该阶段验证、更新 specs/PROGRESS.md 与 Rust 执行进度，
提交明确范围的改动，简短汇报证据，然后继续下一阶段。
如果会话中断，从 Rust 执行进度中最后一个未完成项恢复，不要重做已完成备份。
最终交付可运行的 .app、功能对照表、数据迁移报告、性能对比和构建说明。
当用户将本提示词交给你并明确要求执行时，按上述范围开展迁移。
正式替换已安装稳定版、合入 main 和正式发布前，先提交完整可审查结果，
按用户当时的指示执行。
```

## 1. 已检查的当前状态

以下是编写时的本地事实，执行时必须复核，不能把这里的提交号当作固定起点。

| 项目 | 编写时状态 |
|---|---|
| 工作目录 | `/Users/neil/Code/NeoHub` |
| 远程 | `ssh://git@ssh.github.com:443/neotu23/NeoHub.git` |
| 分支 | `main` |
| 代码版本 | `02acedb`，`feat: destroy the panel on close and restore it on reopen` |
| 远程跟踪记录 | 本地 `origin/main` 为 `0cd4a2e`，本地领先 1 个提交；尚未用 fetch 验证远端最新状态 |
| 代码检查时工作区 | 无未提交改动；本文创建后会出现文档改动 |
| 技术栈 | Electron 33、React 18、TypeScript、Vite、Tailwind、Zustand |
| 核心能力 | SQLite、PTY、多任务编排、日志、Node 选择、文件监听、端口/资源监控、菜单栏、MCP |
| 数据库文件名 | `devharbor.db`，SQLite WAL 模式 |
| 旧用户目录选择 | 优先检查 `NeoHub`、`DevHarbor`、`devharbor` 中包含上述数据库的目录 |
| 已有数据迁移 | `src/main/db/migrations/0001_init.sql` 至 `0007_app_autostart.sql` |
| 现有深链接 | `devharbor://open`、`devharbor://start` |
| MCP 工具数 | 当前源码注册 22 个，清单见阶段 14 |

`specs/PROGRESS.md` 部分库存信息落后于代码，例如某些依赖在代码中已使用但表中仍写待实现。实施前把差异列入功能清单；不依据过时表格删掉功能。

## 2. 方案与边界

### 2.1 仓库策略

| 方案 | 权衡 | 本计划采用 |
|---|---|---|
| 同仓库保留旧版分支，Rust 独立分支 | 继承历史、Issues、品牌资源，便于比较；需要分清两套构建 | 是 |
| 新建 Rust 远程仓库 | 方便独立维护两个产品，但文档、发布和问题追踪分散 | 否 |
| 直接在 main 删除旧版后重建 | 短期目录干净，但重建期间 main 无稳定运行版本 | 否 |

保留 `legacy/electron` 分支和 `electron-final-20261106` 标签。分支可承载明确需要的旧版修复；标签永久指向备份时的确切提交，不移动、不覆盖。

Rust 代码先加在 `native/`，旧 `src/` 留作参考。Rust 稳定后，是否删除主线中的旧构建文件另行处理；历史分支与标签始终保留。

### 2.2 应用架构

- 一个原生应用进程：后台 `CoreService`、数据库线程、PTY 读写/等待工作单元、异步探测与 GPUI 主线程。
- 窗口和界面实体属于可释放的 `WindowSession`；核心由应用生命周期持有，不能由窗口持有唯一强引用。
- GPUI 主线程不做阻塞 SQL、PTY 读取、进程等待、目录扫描和网络就绪探测。
- UI 与 MCP 调用同一套核心命令；不得各实现一套启动逻辑或分别写数据库。
- UI 通过有界事件通道同步状态，生命周期事件不能静默丢弃；统计允许合并，日志通过带序号的缓冲区恢复。
- 不预先拆分后台 daemon、远程 RPC、账号系统或云同步。这些不在本次迁移范围。

### 2.3 资源与兼容目标

这次验收衡量 NeoHub 自身资源。Node 项目单独统计。没有实测之前，不写“降低 80%”“后台只有 10 MB”之类结论。

当前 Electron 包声明 macOS 11。GPUI 依赖的最低系统版本必须在阶段 2核对；不能直接把旧版本号复制进新包。若必须提高系统要求，在开始后续功能实现前明确记录并告知用户。

## 3. 当前源码与 Rust 模块映射

所有下列旧文件均需实际阅读，不要只看文件名。其单元测试是行为样本，需移植重要断言，不是把 TypeScript 语法逐行翻成 Rust。

| 当前文件/目录 | Rust 新位置（相对 `native/`） | 保留重点 |
|---|---|---|
| `src/shared/types.ts`、`src/shared/ipc.ts` | `crates/domain/src/` | ID、任务、状态、设置与所有操作契约 |
| `src/main/db/index.ts`、`migrations/`、`backfill.ts` | `crates/storage/src/`、`migrations/legacy/` | SQL 历史、迁移标识、补数、外键、事务 |
| `AppRegistry.ts`、`TaskRegistry.ts`、`AppOps.ts` | `crates/core/src/catalog/` | 原子创建、路径去重、任务依赖、文件夹/标签 |
| `PathProbe.ts`、`NodeResolver.ts`、`PMDetector.ts`、`DetectionService.ts` | `crates/core/src/environment/` | Finder 启动时 PATH、版本管理器、工作区识别 |
| `EnvBuilder.ts`、`EnvStore.ts`、`src/shared/dotenv.ts` | `crates/core/src/environment/`、`crates/storage/src/secrets.rs` | 三层覆盖、文件优先级、密文兼容 |
| `TaskRunner.ts` | `crates/core/src/process/` | PTY、启动去重、停止竞态、退出回收 |
| `AppOrchestrator.ts`、`topo.ts`、`readiness/` | `crates/core/src/orchestrator/` | 依赖分层、反向停止、就绪、失败回滚 |
| `LogBuffer.ts`、`RunHistory.ts` | `crates/core/src/logs/`、`crates/storage/src/history.rs` | 有界日志、历史、退出保留与清理 |
| `StatsMonitor.ts`、`PortDetector.ts` | `crates/core/src/monitor/` | PID 树、真实监听与日志端口提示的区别 |
| `EnvFileWatcher.ts`、`RestartWatcher.ts` | `crates/core/src/watch/` | 合并事件、排除目录、关闭窗口仍工作 |
| `src/main/index.ts`、`PanelWindow.ts`、`TrayController.ts`、`menu.ts` | `crates/desktop/src/` | 关闭/退出、菜单栏、Dock、重开、单实例 |
| `DeepLinks.ts`、`OpenIn.ts`、`Updater.ts` | `crates/platform-macos/src/` | 原生系统操作与协议兼容 |
| `src/main/mcp/` | `crates/mcp/src/` | 22 工具、鉴权、回环、TLS、脱敏 |
| `src/renderer/store/`、`lib/panelSaves.ts` | `crates/ui/src/state/` | 导航持久化、编辑暂存、保存屏障 |
| `src/renderer/components/` | `crates/ui/src/views/`、`components/` | 所有可见功能、空态、失败态与快捷键 |
| `LogTerminal.tsx`、`LogSearchView.tsx`、`GlobalLogSearch.tsx` | `crates/ui/src/terminal/`、`views/log_search.rs` | 终端解析、选择复制、搜索、导出 |
| `src/renderer/i18n/`、`styles.css` | `crates/ui/locales/`、`crates/ui/src/theme.rs` | 中英文、主题、间距、颜色、字体 |
| `build/icon.icns`、`build/icon.png` | `assets/` | 现有品牌图标 |

`native/` 下 Cargo 包名统一为 `neohub-domain`、`neohub-storage`、`neohub-core`、`neohub-mcp`、`neohub-platform-macos`、`neohub-ui`、`neohub-desktop`；桌面可执行文件名为 `neohub-desktop`。

依赖方向：domain 无 GUI/数据库依赖；storage 依赖 domain；core 依赖 domain/storage；mcp 依赖 core；ui 通过 core 句柄访问数据；desktop 装配这些模块。macOS 适配不得反向依赖 UI 视图。不要让 GPUI 类型渗入 domain、storage 和进程管理。

## 4. 依赖选择与版本规则

| 能力 | 候选选择 | 执行要求 |
|---|---|---|
| 界面 | GPUI + Longbridge 兼容组件层 | 先验证官方当前最小应用；本次查阅旧 `gpui-component` 仓库会重定向至 `gpui-kit`，不要混用不同年代 API |
| 异步 | Tokio | 核心显式持有 Runtime；GPUI 主线程不 `block_on` |
| SQLite | rusqlite | 单独数据库工作线程；显式启用外键和忙等待策略 |
| PTY | portable-pty | 阻塞读取/等待独立工作单元，验证 macOS 子进程组终止能力 |
| 终端解析 | alacritty_terminal 或已验证的同等 Rust 终端状态库 | 先检查具体版本许可证与 API；不自行用正则删除 ANSI 来代替终端 |
| 文件监听 | notify | 事件合并、过滤、无需 UI 存活 |
| 监控 | sysinfo 与必要的 macOS 系统接口 | 明确本进程/根 PID/整棵受管树的口径，避免重复计数 |
| MCP | 官方 rust-sdk 的 rmcp | 保留当前 HTTP `/mcp` 客户端接入方式，不改成仅 stdio |
| 菜单栏 | tray-icon 或窄范围 AppKit 桥接 | macOS 主线程创建，复用 GPUI 事件循环，不创建第二套主循环 |
| Keychain 与系统操作 | security-framework、objc2 系列或等价维护中绑定 | 按实际需要引入，不顺带引入 Tauri/WebView |
| 公共能力 | serde、serde_json、ulid、thiserror、tracing、regex 等 | 仅启用使用的 features；正则语义差异见阶段 7 |

不要使用 `version = "*"` 作为交付配置。注册表依赖用验证过的明确版本范围并提交 `Cargo.lock`；Git 依赖固定 `rev`，记录完整 SHA。不要凭本文候选库名断言它们已相互兼容。

## 5. 核心接口和一致性约定

阶段 2先落地接口说明，阶段 3—8逐步实现；不得一口气为所有接口填空函数。

- `CoreHandle::request(command: CoreCommand) -> Future<Output = Result<CoreReply, CoreError>>`：唯一业务命令入口。
- `CoreHandle::subscribe() -> CoreSubscription`：状态事件订阅；接收溢出通知时请求新快照。
- `CoreCommand::GetSnapshot` 返回 `CoreSnapshot { revision, apps, tasks, running_tasks, app_states, settings }`。
- 命令覆盖现有 IPC 的所有业务操作；对具体 CRUD 使用显式输入结构，禁止接受可以覆盖 ID、路径、时间戳等任意字段的无验证 map。
- `CoreEvent { revision, kind }`：领域状态 revision 单调递增；新 UI 订阅先建立接收缓冲，再取快照，只应用比快照新的事件。
- `AppId`、`TaskId` 使用透明字符串新类型；存储沿用现有 ULID 字符串，不批量重新生成。
- `ProcessState` 精确保留 `idle/starting/running/exiting/exited/crashed`。`ready` 和 `listening` 独立，MCP 的未配置探针结果必须可表示 `null`。
- `Settings` 字段与 `src/shared/ipc.ts` 的 `SettingsState` 一致，存储编码兼容旧表；不要默默重置用户设置。
- `SpawnSpec { program, args, cwd, env, initial_cols, initial_rows }`：命令拼装在启动前完成；脚本模式使用参数数组，用户明确配置的 custom 模式才允许 shell 命令字符串。
- `TaskRunner::start(task_id)`、`stop(task_id)`、`resize(task_id, cols, rows)`、`Orchestrator::start_app(app_id)` 等都通过上述命令入口调用，UI/MCP 不直接访问它们。
- 日志核心持有每个任务的有界终端数据状态及原始输出环；`LogCursor { run_id, seq }` 区分重启前后的日志。
- `subscribe_logs(task_id, after: Option<LogCursor>) -> LogSubscription` 原子返回 `LogBootstrap` 和从其高水位之后开始的事件。若游标已淘汰，返回 `ResetRequired` 和终端状态快照，不静默制造连续日志。
- `WindowSession::prepare_close()` 异步提交当前编辑与会话状态，失败返回可见错误，成功后才销毁窗口；它不能触发 `CoreCommand::Shutdown`。
- `Shutdown` 先拒绝新启动，等待或取消在途启动并回收可能创建的进程，再停止全部受管任务、写运行历史、关闭工作单元和数据库，最后退出主循环。

接口具体枚举和结构在实现时写入 `native/docs/interfaces.md`。上述名字是本计划约定，不是当前已有 API。

## 6. 验收范围与推荐顺序

| 里程碑 | 阶段 | 验收后能做什么 |
|---|---|---|
| A：可回退且技术链路可行 | 0—2 | 远程旧版可恢复，原生窗口可启动，依赖锁定 |
| B：真实核心闭环 | 3—8 | 测试数据库、项目识别、进程、就绪、日志、监控可工作 |
| C：首个可用桌面闭环 | 9—12 | 导入/启停/日志/菜单栏/关闭重开真实运行 |
| D：功能完整 | 13—15 | 实际数据可迁移、22 MCP 工具与系统功能完成、构建安装包 |
| E：交付可审查版本 | 16—17 | 功能、迁移、性能和回退证据齐全，可决定合入和发布 |

以下阶段复选框只在真实完成后打勾。先前阶段发现的问题回到所属阶段修复，不绕过关键阻塞继续堆界面。

## 阶段 0：确认起点和旧版验收基线

**读取：** `AGENTS.md`（若存在）、`specs/WORKFLOW.md`、`specs/PROGRESS.md`、`package.json`、`README.md`、本文源码映射。

**创建：** `specs/rust-migration/BASELINE.md`、`specs/rust-migration/FEATURE-PARITY.md`、`specs/rust-migration/PROGRESS.md`。

- [ ] 在 `/Users/neil/Code/NeoHub` 执行 `git status --short --branch`、`git remote -v`、`git rev-parse HEAD`、`git log -5 --oneline`，记录实际值和时间。
- [ ] 执行 `git fetch origin` 并确认远程确实属于 `neotu23/NeoHub`；网络失败记录为阻塞，不能用旧跟踪记录冒充远端备份。
- [ ] 区分已有用户改动和本计划产生的文档改动；不自动暂存全部文件、不 stash 或覆盖未知改动。
- [ ] 按 IPC 清单、组件和测试列出完整功能，每项标记 `未开始/开发中/已验证/阻塞`，列出验证命令或人工操作。保留旧版实测缺陷，别误当迁移新增缺陷。
- [ ] 使用合成项目验证旧版，记录仪表盘、项目详情、任务配置、环境变量、设置、导入、搜索、深浅主题与空态截图；私密路径和项目名使用测试数据。
- [ ] 在依赖可用时运行 `pnpm typecheck`、`pnpm lint`、`pnpm test`、`pnpm build`，记录通过/失败、退出码。失败要分析归属，不先修改旧版业务代码凑绿灯。
- [ ] 建立旧版发布构建的资源基线，测试方法见阶段 16；暂时无法得到发布构建则明确写缺失。
- [ ] 明确提交本文和上述基线文件，检查暂存区后提交 `docs: define Rust GPUI migration baseline`。不要把本机数据或截图中的密钥提交进去。

**验收：** 起点可复现，功能清单有源码依据；没有创建 Rust 产品代码，也没有把“文档写好了”记作备份完成。

## 阶段 1：远程代码备份、数据备份、独立工作区

**修改：** 仅在需要时给 `.gitignore` 增加 `.worktrees/`、`native/target/`，显式提交这一小项。Git 根目录脚本保留 Electron 现有入口。

**输出：** 不进 Git 的代码 bundle 与用户数据备份；`specs/rust-migration/BACKUP.md` 只记录脱敏路径、哈希、时间和恢复方法。

### 1A. Git 备份

- [ ] 完成阶段 0文档提交和忽略规则提交后，确保 `git status --porcelain` 为空。
- [ ] 在本阶段使用的同一个 shell 中执行 `BACKUP_COMMIT=$(git rev-parse HEAD)`，取得本次确切备份提交。记录该值，后续若重开 shell 先从记录恢复变量并核验提交；不要硬编码 `02acedb`。
- [ ] 检查本地及远端是否已有 `legacy/electron`、`electron-final-20261106`、`rewrite/rust-gpui`。已有且指向相同起点则复用；不相同则报告冲突，不强行重置。
- [ ] 创建旧版分支和 annotated tag。以下命令只在确认对应引用不存在时执行：

```bash
git branch legacy/electron "$BACKUP_COMMIT"
git tag -a electron-final-20261106 "$BACKUP_COMMIT" -m "NeoHub Electron baseline before Rust GPUI migration"
git push --atomic origin refs/heads/legacy/electron refs/tags/electron-final-20261106
```

- [ ] 若服务端不支持 atomic push，分开推送两个引用并分别验证；不得改用 force push。
- [ ] 使用 `git ls-remote origin refs/heads/legacy/electron 'refs/tags/electron-final-20261106*'` 核对远端。annotated tag 的 `^{}` 解引用 SHA 应等于 `BACKUP_COMMIT`，不是要求 tag 对象 SHA 等于 commit SHA。
- [ ] 在仓库外用户可写目录创建 `git bundle create <绝对备份路径>/neohub-electron-20261106.bundle --all`，执行 `git bundle verify <同一路径>`，再在临时目录 clone bundle，确认 `git rev-parse electron-final-20261106^{commit}` 一致。备份目录不是 `/tmp` 中唯一一份。
- [ ] 记录 bundle 的 SHA-256。bundle 只含 Git 追踪对象，不把它当作实际项目数据库或忽略文件的备份。

### 1B. 用户数据备份

- [ ] 在旧版通过 `db:path` 或设置中的数据库路径确认实际 userData 目录；不能仅猜 `~/Library/Application Support/NeoHub`。参考 `AppIdentity.ts` 的三个历史目录候选。
- [ ] 先停止或记录受管任务，再**完全退出 NeoHub**；关闭窗口只剩菜单栏不是退出。核实没有进程仍写 userData。
- [ ] 冷备份整个已确认 userData 目录到独立、受限访问的用户备份目录，保留文件元数据；包括数据库、可能存在的 WAL/SHM、窗口状态和旧 localStorage 数据。若应用必须持续运行，改用 SQLite backup API 并另行一致性导出设置，不能裸复制活动中的 `.db`。
- [ ] 对备份副本执行 SQLite `PRAGMA integrity_check`、`PRAGMA foreign_key_check`，记录表行数；检查只操作副本。记录 macOS Keychain 仍是外部依赖，复制文件不能保证密文在另一台电脑解密。
- [ ] 不将数据库、Chromium 存储、加密/明文迁移包或 bundle 提交至 Git。备份报告不记录变量值或 Token。

### 1C. Worktree

- [ ] 首选用户可写的相邻目录 `/Users/neil/Code/NeoHub-Rust`；如果执行环境仅允许当前仓库写入，使用已忽略的 `/Users/neil/Code/NeoHub/.worktrees/rust-gpui`。
- [ ] 先用 `git worktree list` 检查已有 worktree，复用匹配目录，不重复创建。新建时从上述 `BACKUP_COMMIT` 执行 `git worktree add -b rewrite/rust-gpui <选定绝对目录> "$BACKUP_COMMIT"`。
- [ ] 后续命令工作目录明确设为该 worktree。建立 Rust 首次提交后，用 `git push -u origin rewrite/rust-gpui` 保存进度。
- [ ] 原始目录继续停留原分支；不自动 push main，不删除或覆盖 GitHub 原引用。

**验收：** GitHub 两个备份引用和本地 bundle 均验证；用户数据备份可检查；Rust 分支与旧版工作目录分离。

## 阶段 2：锁定依赖并验证原生宿主

**创建：** `native/Cargo.toml`、`native/Cargo.lock`、`native/rust-toolchain.toml`、`native/crates/*/Cargo.toml`、`native/crates/desktop/src/main.rs`、`native/crates/ui/src/lib.rs`、`native/docs/DEPENDENCIES.md`、`native/docs/interfaces.md`。

**输入/输出：** 输入现有品牌资源；输出能打开、关闭、再次打开的真实 GPUI 窗口，以及经过构建验证的依赖组合。

- [ ] 检查 `rustc --version`、`cargo --version`、`xcode-select -p`、`xcrun --find metal` 和 Apple Silicon 架构。缺系统构建工具时说明准确依赖，不假装编译通过。
- [ ] 阅读当前 GPUI 和 Longbridge 官方最小示例，在 `DEPENDENCIES.md` 记录所用包、版本/完整 rev、启用特性、许可证、Rust 和 macOS 最低版本。
- [ ] 优先采用组件库维护者验证过的 GPUI 组合；若通过库重导出 GPUI，统一使用该类型来源，避免两份 GPUI 不兼容。用 `cargo tree -d` 分析重复项，不盲目 patch。
- [ ] 创建最小 Cargo workspace，只添加本阶段需要的代码，不预先生成所有业务空模块。
- [ ] 打开真实窗口，显示 NeoHub、侧栏占位和中文输入组件；验证中文输入法组合、选择、剪贴板、Tab 焦点、暗色主题。此时占位只能用于宿主验证，不能记为功能完成。
- [ ] 用本计划约定写出核心命令/快照/事件接口说明；GUI 仅通过异步句柄调用后台。
- [ ] 从现有 `styles.css`、组件 class 和截图提取色值、间距、圆角、字体、选中/悬浮/禁用状态。建立 `native/crates/ui/src/theme.rs`，不重新设计视觉风格。
- [ ] 在发布 profile 中验证构建和实机启动；依赖组合不成立先解决，不凭猜测继续大规模写 UI。

**验证命令（在 Rust worktree 的 `native/`）：**

```bash
cargo check --workspace --locked
cargo build -p neohub-desktop --release --locked
cargo tree -d
cargo run -p neohub-desktop --release --locked
```

**验收：** 窗口真实可交互；无 Chromium/WebView；记录构建环境和锁定版本。提交 `build: establish native GPUI workspace`。

## 阶段 3：数据模型、数据库与项目管理

**创建：** `native/crates/domain/src/{ids,models,settings,error}.rs`、`native/crates/storage/src/{db,migrations,catalog,history}.rs`、`native/crates/storage/migrations/legacy/0001_*.sql` 至 `0007_*.sql`、`native/crates/storage/tests/{legacy_import,catalog}.rs`、`native/crates/core/src/catalog/`。

**接口：** 实现 `Storage::open(data_dir)`、`Catalog::create_app(CreateAppInput)`、应用/任务 CRUD、folders 操作；返回 domain 类型，通过 Core 命令调用。

- [ ] 用现有 SQL 构造合成数据库 fixture，所有秘密字段使用假值。先写测试：7 次迁移、无任务老应用补数、三层 env 同键、外键级联、历史 task_id 可空、重复打开幂等。
- [ ] 运行 `cargo test -p neohub-storage --test legacy_import --locked`，先确认缺实现导致失败，再实现。
- [ ] 原样保存旧迁移文件内容和 `_schema_migrations` 的无 `.sql` 版本名；不能换成 `user_version` 后重复执行旧 ALTER TABLE。
- [ ] 旧 backfill 用 Rust 重写，保留用户任务 ID、顺序和 `env_overrides` 的延迟迁移语义；写入必须原子化。Rust 新迁移另设 `migrations/native/` 与独立版本记录，不能与旧 0001—0007 撞名。
- [ ] 实现 `CreateAppInput` 的应用+首任务/多个任务+环境变量原子创建；中途任一步失败不得留半个应用。
- [ ] 实现 realpath 去重、路径不存在可见错误、文件夹重命名/清空、标签、排序、任务重排和依赖验证。清空文件夹仅取消分组，不删除项目目录。
- [ ] 重复添加同一路径、重复任务顺序、未知依赖、事务回滚有明确断言。原任务删除对依赖的处理按旧源码核实，不自行静默留下悬空引用。
- [ ] 开发默认目录固定为 `~/Library/Application Support/NeoHub-Rust-Dev`，支持 `--data-dir <path>` 显式测试覆盖。测试使用 `tempfile`；此阶段不碰真实旧数据库。

**验证：** `cargo test -p neohub-storage --locked` 与 `cargo test -p neohub-core catalog --locked`；重启后合成项目、任务、分组和设置不丢失。提交 `feat: port catalog and SQLite persistence`。

## 阶段 4：Node 环境、项目检测、环境变量和密钥接口

**创建：** `native/crates/core/src/environment/{detect,node_resolver,path_probe,dotenv,env_builder}.rs`、`native/crates/storage/src/secrets.rs`、`native/crates/platform-macos/src/keychain.rs`、`native/crates/core/tests/{environment,detection}.rs`。

**接口：** `detect_project(path) -> DetectionResult`、`resolve_node(pref, cwd) -> NodeInstallation`、`build_env(app, task, resolved_node) -> EnvMap`；`SecretStore` trait 提供加密、解密和明确的不可访问错误。

- [ ] 对照 NodeResolver、DetectionService、dotenv 测试创建 fixture：带空格/中文路径、软链接、无 package.json、损坏 JSON、monorepo、缺指定 Node 版本、不同包管理器。
- [ ] 保留 nvm、fnm、Volta、asdf、system 检测和 `.nvmrc`、`.node-version`、`engines.node` 选择规则；不自动安装或升级用户 Node。
- [ ] Finder 启动没有终端 PATH 时，复用有超时和缓存的登录 shell PATH 探测；不能在每次渲染或每次监控时运行 shell。
- [ ] `.env` 文件覆盖顺序为 `.env` → `.env.development` → `.env.local` → `.env.development.local`；之后按 Global → App → Task 覆盖，最后强制 `FORCE_COLOR=1`、`TERM=xterm-256color`。
- [ ] 文件中的 `PATH/NODE_OPTIONS/NODE_PATH/DYLD_*/LD_*` 不得覆盖进程控制配置。UI 中显式设置的变量按旧逻辑处理；保留 disabled 和空字符串语义。
- [ ] 先用假 SecretStore 验证正常/锁定/损坏路径，再接 macOS Keychain。新密文使用独立版本前缀，不把新格式伪装成旧 `enc1:`。
- [ ] 测试不可解密值必须返回 `SecretUnavailable`，阻止受影响任务启动并给出恢复入口；不能把 `enc1:...` 原文作为密码传给子进程。

**验证：** `cargo test -p neohub-core --test environment --test detection --locked`；从 Finder 启动的原生包能发现预置测试 Node。提交 `feat: port project detection and environment resolution`。

## 阶段 5：PTY 运行、进程树停止与退出历史

**创建：** `native/crates/core/src/{service,commands,events,snapshot}.rs`、`native/crates/core/src/process/{runner,spawn_spec,process_tree}.rs`、`native/crates/core/tests/process_lifecycle.rs`、`native/test-fixtures/process/`。

**接口：** 实现 `CoreHandle::request/subscribe`、`GetSnapshot`、单任务 start/stop/resize、Shutdown，接入阶段 3的目录和历史存储。

- [ ] 合成测试进程只输出固定日志、延迟启动、派生子孙进程、忽略 SIGTERM 或按指定退出码退出；测试结束由 fixture 回收所有自身进程，不启动用户真实项目。
- [ ] 先写 `same_task_concurrent_start_spawns_once`、`stop_during_start_leaves_no_orphan`、`start_during_exit_waits_for_teardown`、`quit_during_spawn_reaps_late_child` 测试并确认失败。
- [ ] 在 Core actor 中串行化同任务状态变化；PTY 读取和 wait 使用独立工作单元，不能占住主 actor 或 Tokio 核心线程。
- [ ] 原生 spawn 初始网格 120×30；resize 最小 2×2。保留环境、工作目录与包管理器的实际命令行为。
- [ ] 停止流程先向自己拥有的进程组发 TERM，默认等 5000 ms 后升级 KILL；用户设置生效。确认 portable-pty 创建会话/进程组的真实行为，不把 `child.kill()` 当作必然清理全部后代。
- [ ] 只持有并终止本轮启动的 live handle/身份。重启应用不得凭数据库中的旧 PID 杀进程；核实目标组不等于 NeoHub/当前测试的进程组。
- [ ] PTY spawn 成功但数据库历史写入失败时，仍必须登记运行实体和退出回收，不遗留失控进程；日志记录错误但不输出环境值。
- [ ] 状态转换与退出事件只结算一次，正确区分用户停止、正常退出和异常退出；重启后的应用显示上次 stopped/crashed 结果，而不是一律 idle。
- [ ] 生命周期事件 channel 满时必须背压或触发 resync；Stats 可合并，不能挤掉退出事件。修订号快照恢复要有测试。

**验证：** `cargo test -p neohub-core --test process_lifecycle --locked`；同时验证正常/忽略信号子进程树停止后不再监听测试端口。提交 `feat: implement PTY task lifecycle and shutdown`。

## 阶段 6：有界日志、终端状态与订阅恢复

**创建：** `native/crates/core/src/logs/{buffer,subscription,terminal_state,search}.rs`、`native/crates/core/tests/log_stream.rs`、`native/test-fixtures/logs/`。

**接口：** 实现阶段 5中的带 run_id/seq 日志订阅、原始日志读取/导出和全局搜索；终端网格状态可产生只含数据、不含 GPUI 实体的快照。

- [ ] 使用跨 chunk 的中文/emoji UTF-8、跨 chunk ANSI、CR 覆写、清行、光标移动、超长行、重启和大量输出样本，先写失败测试。
- [ ] 初始原始缓冲预算沿用旧版意图：每任务 5 MiB、总预算 100 MiB、单 chunk 上限 10 KiB、已退出任务约 10分钟回收；设置中的 `log_ring_size` 初值 10000。旧代码以 JS 字符串长度近似字节，Rust 必须用真实 UTF-8 字节且不切坏字符/控制序列；差异记入兼容报告。
- [ ] 终端状态、scrollback、raw buffer、搜索结果与队列全部有独立上限并可报告占用。达到原始缓冲上限可以淘汰旧历史，不能丢弃解析器尚未处理的任意字节导致终端永久失步。
- [ ] 继续在后台消费 PTY，保留有界终端状态；仅可见日志面板创建绘制实体并接收界面刷新。关闭窗口停止 UI 订阅，不停止 PTY 输出采集。
- [ ] 正常界面日志刷新按约 33 ms 或 16 KiB 批量；慢订阅者必须收到明确 gap/reset，随后从原子 bootstrap 恢复，不能形成无限队列。
- [ ] 终端快照包含解析位置和网格/scrollback。重开窗口不从随机截断的 ANSI 片段开始盲目重放；ANSI 后端使用既有维护中库。
- [ ] 退出任务在淘汰前仍能搜索/查看，清日志只清所属任务，不影响进程；原始文件导出与搜索可读文本要明确区别。
- [ ] 测试“先订阅再快照”的高水位边界：连续 1000 条编号输出，挂载时穿插新日志，不重不漏；淘汰时显示截断提示。

**验证：** `cargo test -p neohub-core --test log_stream --locked`；长时间高输出队列不持续增大。提交 `feat: add bounded logs and recoverable terminal streams`。

## 阶段 7：任务依赖、就绪探测、失败回滚

**创建：** `native/crates/core/src/orchestrator/{mod,topology,readiness,state}.rs`、`native/crates/core/tests/{orchestration,readiness}.rs`。

**接口：** `start_app/stop_app/restart_app` 与 app-level 状态汇总，均调用已有 TaskRunner；手动启动的任务也有可复用就绪状态。

- [ ] 移植 topology 及 AppOrchestrator 测试中的环、未知依赖、多层并行、反向停止、取消和启动失败断言。
- [ ] 实现六种 domain readiness：`none/port/log/http/exit/delay`。`exit` 保留给 one-shot 任务，不因为当前 MCP set_readiness 不支持它而删除。
- [ ] `none` 表示没有验证就绪；内部可以启动后继续编排，但 MCP 不能谎报 readinessVerified=true。
- [ ] HTTP 检测默认按旧版接受 2xx/3xx，可指定状态，超时/退出会取消。按源码核实回环和自签名支持范围，不能全局关闭 TLS 校验。
- [ ] 检查已有 JS 正则与 flags 到 Rust 正则的兼容性；Rust 不支持的表达式返回明确校验错误，不静默改变语义。对已有依赖此能力的数据单列阻塞；不能用不可取消的灾难回溯塞住核心线程。
- [ ] 任一就绪失败时回收本次启动新增的任务，避免停止之前已由用户独立运行的任务；检查并发 start/stop/restart 的取消令牌。
- [ ] 默认 readiness timeout 60000 ms，disabled 任务不启动；应用 autoStart 不重复启动。

**验证：** `cargo test -p neohub-core --test orchestration --test readiness --locked`；失败、超时、取消后探测 timer/listener/socket 全部释放。提交 `feat: port task orchestration and readiness`。

## 阶段 8：端口、资源、文件监听与通知事件

**创建：** `native/crates/core/src/monitor/{stats,ports}.rs`、`native/crates/core/src/watch/{environment,restart}.rs`、`native/crates/core/tests/monitor_watch.rs`。

- [ ] 先写测试：父任务经 pnpm/node 子进程监听端口、多级后代、同端口提示随后改端口、进程退出恰逢采样、文件事件风暴。
- [ ] 默认统计间隔 1000 ms，可配置且保持旧下限 200 ms；端口探测默认 2000 ms。无运行任务时停止相关轮询，不建立永久空转循环。
- [ ] 端口 UI/MCP 区分 confirmed owner 与只有日志提示的端口；旧逻辑中连续 3次未确认的提示会淘汰。不得把日志中的任意 URL 当成当前任务已监听的证据。
- [ ] 初版可以使用参数数组调用系统 `ps/lsof` 保留兼容行为，但每次采样批量处理，不为每个任务每秒启动多条 shell；命令有超时且不在 GPUI 主线程执行。
- [ ] 统计继续保留旧 UI 语义；若增加进程树合计，单列字段和口径，不静默改变已有 memMB 的含义。
- [ ] `.env` 变化生成可读事件；restart watcher 合并短时变化，排除 `node_modules/.git/.next/dist` 等生成目录，复用启动锁避免重启风暴。
- [ ] 关闭窗口后 MCP、监听与通知事件继续可用；系统通知开关遵循现有设置，实际通知的授权和跳转由阶段 15接入。

**验证：** `cargo test -p neohub-core --test monitor_watch --locked`；无任务静置没有周期 shell 进程。提交 `feat: port process monitoring and file watchers`。

## 阶段 9：原生主界面、侧栏、仪表盘、导入

**创建：** `native/crates/ui/src/views/{shell,sidebar,dashboard,add_app,import_projects}.rs`、`native/crates/ui/src/components/`、`native/crates/ui/src/state/{session,store}.rs`、`native/crates/ui/tests/navigation.rs`。

**输入/输出：** 输入 CoreSnapshot、CoreEvent 和主题；输出用户命令与导航状态，不在 render 中读数据库或 spawn。

- [ ] 按阶段 0截图逐项重建：侧栏分组/标签/排序/筛选、项目卡片、CPU/内存/端口、添加项目、扫描导入、Node 选择、空态、错误提示、加载态。
- [ ] 界面读取真实核心快照，按钮真实调用命令。列表使用 GPUI 虚拟列表；后台 stats 变化不导致整个 UI 无差别重建。
- [ ] 实现焦点、Tab、Esc、右键菜单边界、弹窗遮罩和命令面板；文本不能仅靠颜色表达状态。
- [ ] 导航持久化到 Rust 数据目录的独立 session 文件，包含 view、selectedAppId、selectedTaskByApp 和排序；引用失效时回退仪表盘。
- [ ] 使用合成数据和至少 100个项目验证滚动、筛选、折叠和分组。窗口 960×600及更大尺寸无重要操作被遮挡。
- [ ] 真实操作完成“导入合成 Node 项目 → 启动 → 卡片显示运行 → 端口打开浏览器 → 停止”。

**验证：** `cargo test -p neohub-ui --test navigation --locked`，另人工记录中英文、浅色/深色、空/运行/异常三种状态截图。提交 `feat: build native dashboard and project navigation`。

## 阶段 10：项目详情、任务编辑、环境变量与设置

**创建：** `native/crates/ui/src/views/{app_detail,task_editor,app_config,env_editor,settings,run_history}.rs`、`native/crates/ui/src/state/{edit_session,save_barrier}.rs`、`native/crates/ui/tests/editor_save.rs`、`native/crates/ui/locales/{zh-CN,en}.json`。

- [ ] 详情页保留任务 tabs、每任务启停、项目级启停/重启、历史退出结果、崩溃日志入口、Open In 与环境文件变化提示。
- [ ] 任务编辑保留依赖、readiness、oneShot、enabled、命令、工作目录、包管理器/Node 覆盖和重排；后端验证错误定位到字段。
- [ ] 环境编辑保留 Global/App/Task、启用开关、秘密遮挡、备注、粘贴解析和覆盖来源；不要将掩码字符作为真实值写回。
- [ ] 设置覆盖全部 `SettingsState`，未知字段拒绝，默认值参照 `Settings.ts`；数据库导出/重置、MCP、启动登录项、通知、菜单栏开关均有实际状态。
- [ ] 实现编辑修订号和保存队列。MCP 改动引发的快照刷新不能覆盖正在输入但尚未提交的编辑；使用冲突提示或重新基于最新版本提交，不静默丢用户文字。
- [ ] 保存屏障等待所有已登记的编辑队列；失败在原窗口保留输入并可重试/明确放弃。不能捕获失败后无条件关窗。
- [ ] 单元测试 `close_flushes_pending_edits`、`save_failure_keeps_window_and_input`、`mcp_refresh_preserves_active_edit`、`stale_save_does_not_overwrite_newer_revision`。
- [ ] 中英文文案从现有语言包对照迁移；主题与 system 模式随系统变化，必要文本字段支持中文输入法。

**验证：** `cargo test -p neohub-ui --test editor_save --locked`；手动修改后立刻关窗重开，内容已保存且没有重复启动任务。提交 `feat: port native editors and settings`。

## 阶段 11：原生终端、日志搜索与导出

**创建：** `native/crates/ui/src/terminal/{view,selection,search,keyboard}.rs`、`native/crates/ui/src/views/log_search.rs`、`native/crates/ui/tests/terminal.rs`。

- [ ] 从 Core 的有界终端状态渲染，不使用 HTML/xterm.js；仅创建可见任务的 GUI 绘制与选择状态，不为所有任务缓存全套离屏 UI。
- [ ] 保留 ANSI 16/256/真彩色、CR 进度条重绘、清行、光标移动、滚动历史、自动跟随开关、选择复制、字体大小、普通与正则搜索、高亮/前后跳转、日志导出。
- [ ] PTY 列行数随可见终端变化，通过核心 resize；保持 UTF-8、中文宽字符、组合字符和 emoji 网格一致。
- [ ] 当前旧版是日志展示，未接入向 PTY 发送键盘输入的 IPC；不把“交互式 shell 输入”当作此次功能保留要求擅自扩展。
- [ ] 全局日志搜索按任务/项目定位并限制结果数；已退出且未淘汰的日志仍可搜，点击结果切换正确项目/任务。
- [ ] 同时跑两个不同速率日志任务，验证切 tab、关闭重开和重启 run_id 后不混日志、不重复历史、慢界面不拖死核心。
- [ ] 复用阶段 6 ANSI fixture 做终端状态断言；人工截图对比同一输出在旧 xterm 和 GPUI 的显示，包括暂停跟随后的选择复制。

**验证：** `cargo test -p neohub-ui --test terminal --locked` 和真实 PTY 连续输出至少 10分钟；保存截图与内存趋势。提交 `feat: implement native terminal and log search`。

## 阶段 12：窗口销毁、菜单栏、Dock、单实例

**创建：** `native/crates/desktop/src/{app_host,window_session,session_lock}.rs`、`native/crates/platform-macos/src/{tray,dock,activation}.rs`、`native/crates/desktop/tests/window_lifecycle.rs`。

- [ ] 主线程创建菜单栏图标并复用 GPUI/Cocoa 主循环；禁止再启动 tao/winit/AppKit 主循环造成抢占。
- [ ] 关闭按钮和 Cmd+W 调用 prepare_close，成功后销毁窗口及 GUI 终端实体。Core、数据库、PTY 和 MCP 仍由 AppHost 持有。
- [ ] 菜单栏启用时关窗后隐藏 Dock，重开前恢复；菜单栏关闭时关窗保留 Dock 的重新打开入口。用户不能进入“无窗口、无菜单栏、无 Dock”的不可达状态。
- [ ] 菜单栏提供 Open NeoHub、逐项目启动/停止、Stop all、Quit；显示使用当前核心状态，操作不要求窗口存在。
- [ ] 从菜单栏、Finder、Spotlight 重开都进入原进程并恢复窗口位置/尺寸、项目/任务选择和日志。每种路径不重新执行项目 autoStart。
- [ ] 实现单实例 IPC/锁，第二实例只传递激活/深链接并退出，不抢数据库、不杀已有实例。Dev 和正式版使用各自锁命名空间。
- [ ] Cmd+Q 和菜单栏 Quit 走完整 Shutdown；不能为保持后台而把用户明确退出操作也拦截成隐藏。
- [ ] 执行自动化生命周期测试：在途启动时关窗、保存失败关窗、无 UI 时日志继续、连续 20次关闭重开无订阅/timer 增长。
- [ ] Dock、Finder 激活、菜单栏和最后窗口关闭后事件循环不退出的行为，在打包的 `.app` 上实测；不能仅根据单元测试宣布完成。

**验证：** `cargo test -p neohub-desktop --test window_lifecycle --locked` 及上述实机操作。提交 `feat: preserve menubar residence and panel recreation`。

## 阶段 13：旧版真实数据与密钥迁移

**创建：** `native/crates/storage/src/import/{mod,legacy_db,manifest}.rs`、`native/crates/storage/tests/migration_atomicity.rs`、`native/crates/ui/src/views/migration.rs`、`native/docs/MIGRATION.md`。必要时在保留的 Electron 源码上增加明确 opt-in 的本地迁移导出入口，文件位置为 `src/main/services/LegacyMigrationExport.ts` 及其最小 IPC/UI 接线，不改动旧标签。

**接口：** `inspect_legacy(source) -> MigrationPreview`、`import_legacy(preview, options) -> MigrationReport`；检查与导入分开，导入 staging 完成后原子发布到 Rust 数据目录。

- [ ] 先在合成副本验证：WAL 未 checkpoint、损坏 JSON、未知未来迁移版本、未知设置、重复导入、目标已有新编辑、失败中断、权限不足和 Keychain 锁定。
- [ ] 来源目录按旧 AppIdentity 优先级发现，展示准确来源和应用/任务数量，不自动导入错误历史目录。生产旧目录只读，写入隔离的 Rust 正式数据目录 `~/Library/Application Support/NeoHub-Rust`。
- [ ] SQLite 使用 backup API 或经确认静止的完整来源副本。迁移清单记录来源快照哈希与 schema 版本；同来源重复导入不重复生成任务，也不覆盖 Rust 中后续编辑。
- [ ] 导入保留 ID、路径、文件夹、标签、环境作用域、任务依赖、历史、设置和 autoStart 配置。首次迁移验证期间强制运行时暂停 autoStart 和 MCP 启用，界面明确标示；底层保留旧配置意图，用户确认启用前不执行旧项目。
- [ ] `enc1:` 是 Electron safeStorage 的密文，不是 Keychain API 可直接替代的格式。先验证原版同身份、同 Keychain 环境能否解密假值；若不能直接兼容，由旧版本地进程通过明确 opt-in 入口导出解密后的必要字段，再由 Rust 重新加密。
- [ ] 导出范围必须同时处理 `env_vars` 中的秘密与 settings 中的 `mcp_token`；不能只迁移环境密码后让所有 MCP 客户端失效。无法解密时保留原密文于备份、生成缺失项清单，不默默生成新 Token 假装兼容。
- [ ] 优先使用一次性本机 IPC 传递秘密。确需文件时仅生成受限目录内权限 0600 的一次性文件，不写 stdout、不进 Git/云目录；Rust 成功导入并验证后删除本次创建的临时文件，不声称文件删除等于物理擦除。
- [ ] 原 userData 中 localStorage 选中项目/排序需要通过旧 Electron 入口读取并导出；Rust 不直接写 Chromium LevelDB。窗口状态从 `window-state.json` 验证导入，损坏时回退默认。无法导出的导航偏好列明，不影响核心业务数据。
- [ ] 对 staging 数据库运行完整性/外键检查，比较逐表行数、关键 ID/依赖哈希和秘密字段解密成功数量；验证失败不替换目标。报告只包含数量、字段名和脱敏错误。
- [ ] 实际启用前停止旧版受管任务并完全退出旧版；检查冲突端口、旧实例/锁和 MCP 6872占用，Rust 不自动结束不属于自己的进程。

**验证：** `cargo test -p neohub-storage --test migration_atomicity --locked`；只读旧数据哈希不变，成功后重启仍能读取迁移数据，失败可继续使用旧版备份。提交 `feat: migrate legacy data and encrypted credentials safely`。

## 阶段 14：MCP 全量迁移与协议兼容

**创建：** `native/crates/mcp/src/{server,auth,tls,redaction,tools}.rs`、`native/crates/mcp/tests/{compatibility,auth,redaction}.rs`、`native/test-fixtures/mcp/`。

**接口：** MCP 仅依赖 CoreHandle 及经授权的 TokenStore；不用 GUI 作为服务端，不为 MCP 单独 spawn Node。

必须对照现有输入 schema、错误结构、返回值与行为迁移下面 22个工具，不仅保留名字：

| 查询 | 配置与导入 | 生命周期 |
|---|---|---|
| `get_overview` | `scan_folder` | `start_app` |
| `list_apps` | `list_node_versions` | `stop_app` |
| `get_app` | `add_app` | `restart_app` |
| `list_running` | `update_app` | `start_task` |
| `get_logs` | `remove_app` | `stop_task` |
| `search_logs` | `add_task` | |
| `list_env_vars` | `remove_task` | |
| `get_ports` | `set_env_var` | |
| | `set_readiness` | |

- [ ] 从 TS 工具 schemas 和已有 MCP tests 生成合成 golden cases。规范化时间戳/随机 ID 后比较返回 JSON、错误与 readiness 语义；不要把旧 UI 不显示的 MCP 能力漏掉。
- [ ] 使用 rmcp 的 Streamable HTTP 与当前 `/mcp` endpoint，默认端口 6872，默认关闭服务且要求 Bearer Token；测试采用系统分配的临时端口，不与旧版冲突。
- [ ] 读取并复现 `gate.ts` 的 loopback、Host/Origin、Token 比较规则和错误码；IPv4/IPv6/localhost 规范化有测试，不改成监听 0.0.0.0。
- [ ] HTTPS 模式保留本地证书/信任说明；端口占用、证书失败与配置变更产生准确状态，错误不能假报 running。
- [ ] 启动相关工具保留现有等待上限语义：约 45秒后可返回 still starting，客户端随后查询；未配置 readiness 返回 null，listening 不等于 HTTP 已可服务。
- [ ] 脱敏实际 secret 和 Token，包括错误、日志尾部、全局搜索及命令描述；UI 允许本地查看不表示 MCP 默认可泄露。
- [ ] 保留 add_app 的工作区多任务创建、任务增删和 set_env_var 的层级；参数校验错误不导致半个事务。
- [ ] MCP 修改项目/任务后，主窗口和菜单栏收到核心事件并更新。窗口不存在时仍能完成 start/stop/get_logs，重开时获取最新快照。
- [ ] 用真实 MCP 客户端完成 initialize → tools/list → tools/call，不以 curl 一个自制 JSON REST 端点代替 MCP 协议验收；Token 轮换后旧 Token 立即失效。

**验证：** `cargo test -p neohub-mcp --locked`，22工具清单和 schema/结果 golden cases 均有结论。提交 `feat: port all MCP tools to native runtime`。

## 阶段 15：系统集成、构建、签名与更新策略

**创建：** `native/crates/platform-macos/src/{deep_links,open_in,notifications,login_item,updater}.rs`、`native/packaging/Info.plist`、`native/scripts/{build-app,package-dmg}.sh`、`.github/workflows/native-checks.yml`、`native/docs/BUILD.md`。

- [ ] 保留 `devharbor://open?path=...`、`open?id=...`、`start?id=...` 行为；start 链接只展示确认入口，不直接执行 shell。冷启动与已驻留两种入口都验证。
- [ ] 路径传系统应用使用参数/API，不拼接 shell。Finder、Terminal、VS Code、Cursor、Sublime 等能力按当前 OpenIn 源码核对，缺应用时准确禁用。
- [ ] 接入通知点击重开正确项目，按用户设置申请/使用权限；接入登录启动并读取真实系统状态，不能仅保存一个布尔值。
- [ ] 第一版原生测试包名 `NeoHub Rust Preview.app`、bundle ID `app.neohub.desktop.rustpreview`，图标沿用现有；正式产品名 NeoHub 和现有正式 bundle ID 的切换只在阶段 17发布方案明确后做。
- [ ] 原生包必须能通过 Finder 直接启动，带图标、资源、版本和正确最低系统要求；包中无 node_modules、开发路径或对 Xcode/rustc 的运行时依赖。
- [ ] `build-app.sh` 构建 Release 二进制并组装 `.app`，`package-dmg.sh` 生成测试 dmg；脚本在失败时退出非零，不覆盖 `/Applications/NeoHub.app`。
- [ ] 不沿用 electron-updater 的 Electron ZIP/feed 来更新 Rust 二进制。首个 preview 提供真实检查版本/跳转下载；如果自动下载安装尚未实现，在功能表标记未完成并禁用对应操作，不假设下载等于校验和更新。
- [ ] 完整稳定发布前补齐经签名校验的原生更新路径，或取得用户对首版手动更新的明确接受并记录差异。自动更新功能不能无声明地丢失。
- [ ] 有可用 Developer ID 时按现有凭据环境完成签名、公证和 stapling；没有凭据则提供可验证本地测试包并准确标明未公证，不伪称正式签名发行版。
- [ ] CI 在 macOS 构建测试 Rust workspace，保留迁移期间旧 Electron CI；workflow 不自动创建稳定 Release 或覆盖旧产物。

**验证：** `bash native/scripts/build-app.sh`、`bash native/scripts/package-dmg.sh`（在 worktree 根目录）；在非开发终端环境打开包，验证菜单栏、Finder 重开、登录项、深链接、通知及实际路径。提交 `build: package native macOS app and system integrations`。

## 阶段 16：全量回归和资源对比

**创建：** `native/docs/ACCEPTANCE.md`、`native/docs/PERFORMANCE.md`、`native/tests/acceptance/`；更新 FEATURE-PARITY 和 PROGRESS。

- [ ] 完整构建并运行 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`、`cargo build -p neohub-desktop --release --locked`。
- [ ] 编译器/依赖平台警告区分上游和本项目，不能用全局 allow 或关掉测试凑通过；记录确实不能自动验证的人工项目。
- [ ] 从 FEATURE-PARITY 逐项实操，无空函数、虚构成功、只演示正常路径的验收；重点覆盖数据库重启持久化、错误 Node、非法任务依赖、崩溃恢复和关闭前保存失败。
- [ ] 同一台 Mac、同系统、电源状态、相同项目和固定日志负载，分别测试 Electron 与 Rust 的 Release 包；不能用 Electron dev+DevTools 对 Rust Release。
- [ ] 两版分开运行，测试场景至少包括：无任务前台、无任务关窗、3个合成任务前台、相同任务关窗、10分钟大量日志、20次关闭重开。测量前预热 30秒，每种状态采样至少 60秒、至少 3轮，报告中位数和范围。
- [ ] Electron 自身统计含主/renderer/GPU/helper，Rust 含实际 helper（若存在）；受管项目进程树另表统计，不从总值中凭猜测扣除。不要把 Rust 进程名较少当作节省量。
- [ ] macOS 优先使用相同工具的 physical footprint/活动监视器口径；另记录 RSS 作为辅助，说明共享页可能重复。不要混用一版 footprint、一版 RSS。记录 CPU、唤醒、启动时间、窗口恢复时间、安装体积和日志缓冲高水位。
- [ ] 使用合成项目的独立进程组/专用端口对照 core 状态，验证退出后整棵树和监听端口消失；不对机器全部 Node 进程执行 kill。
- [ ] 形成预算结论：前台或后台没有明确改善时写实际数据和主因，先检查日志/队列/轮询/重复订阅；不能为了节省内存删功能后声称等价优化。
- [ ] 最终 20次关窗重开后内存趋势稳定，无窗口实体/订阅/timer 单调累积；报告最高值和回收后的稳定区间，不要求操作系统立即回收全部分配页。

性能报告表必须实际填写，不能留“预计”：

| 场景 | Electron 自身 footprint/RSS | Rust 自身 footprint/RSS | 受管项目内存（另列） | CPU | 样本数/差异说明 |
|---|---|---|---|---|---|
| 空闲前台 | 待实测 | 待实测 | 0 | 待实测 | |
| 空闲关窗 | 待实测 | 待实测 | 0 | 待实测 | |
| 3任务前台 | 待实测 | 待实测 | 待实测 | 待实测 | |
| 3任务关窗 | 待实测 | 待实测 | 待实测 | 待实测 | |
| 高频日志 | 待实测 | 待实测 | 待实测 | 待实测 | |
| 20次重开后 | 待实测 | 待实测 | 待实测 | 待实测 | |

**验收：** 每个功能和性能结论有证据，未通过项单列；不能把“cargo check 通过”等同于 macOS 产品验收。提交 `test: verify native feature parity and resource usage`。

## 阶段 17：交付、回退与主线切换准备

**更新：** `README.md`、`CONTRIBUTING.md`、`specs/01-architecture.md`、`specs/02-data-model.md`、`specs/PROGRESS.md`、`native/docs/RELEASE.md`。

- [ ] 输出 `rewrite/rust-gpui` 的最新 commit、测试摘要、预览包、迁移报告、截图和性能表，推送 Rust 分支。检查远端提交与本地一致。
- [ ] README 区分稳定 Electron 版与 Rust preview 的入口；不要在尚未稳定时把现有 Releases 下载引导到不可运行产物。
- [ ] 记录 LICENSE、上游版权、图标和新增依赖归属；技术重写不自动消除已有声明。
- [ ] 准备对 `main` 的审查差异和发布说明；默认不删除旧目录、不 merge main、不覆盖已安装稳定版、不自动发布稳定 Release。
- [ ] 达到完整验收后，按用户实际授权将 Rust 合入 main。正式包从 preview ID/数据目录切换时必须明确迁移方案，防止同机身份/Keychain/用户目录再次丢失。
- [ ] 恢复旧版：退出 Rust，确认其受管任务已停止，用 `legacy/electron` 或 immutable tag 构建/运行旧版。原始旧 userData 未被 Rust 修改，可直接继续使用；Rust 期间新增修改需要另行导出，不能直接拿新 schema 覆盖旧库。
- [ ] 如果旧 userData 被人为损坏，只从阶段 1冷备份恢复到已完全退出的旧应用目录；恢复前把当前目录另存，保留所有证据，不覆盖唯一副本。
- [ ] 不 force push、不移动旧标签、不批量删除分支、bundle 或用户数据备份。

**最终交付清单：**

- [ ] 远程旧分支和标签可恢复，本地 bundle 已 verify。
- [ ] Rust 源码、Cargo.lock、构建脚本、CI、构建说明齐全。
- [ ] `.app`/`.dmg` 真实可运行，签名与公证状态准确。
- [ ] 功能对照表，包括所有界面、22 MCP 工具和系统集成。
- [ ] 数据迁移报告：来源、版本、行数/ID 校验、解密成功/失败数量、回退路径。
- [ ] 前台/后台资源实测报告，不混入受管 Node 项目成本。
- [ ] 已知差异、阻塞和需要用户决定的发布事项独立列出。

## 7. DeepSeek 进度记录与上下文恢复

每阶段结束更新 `specs/rust-migration/PROGRESS.md`，使用固定模板：

```markdown
### 阶段 N：名称
- 状态：未开始 / 开发中 / 已验证 / 阻塞
- 工作分支与目录：
- 最近提交：
- 实际修改文件：
- 验证命令、时间与退出码：
- 人工验证及截图：
- 与旧版不同之处：
- 未解决问题及所属阶段：
- 下一项具体动作：
```

每次新会话先读取本文、BACKUP、PROGRESS、FEATURE-PARITY 与 `native/docs/DEPENDENCIES.md`，再检查 `git status` 和最近提交。已有已验证备份不重复创建，不重装已锁定工具链，不重新设计已经确定的目录结构。

每个阶段一个或少量有意义提交；每次提交前检查 `git diff --cached`，只暂存本阶段文件。不要自动提交用户无关修改，不用 `git add .` 收入数据库和本机配置。

常规依赖选择、内部命名和测试 fixture 可依据本文自行推进；改变平台、引入 WebView、丢弃现有功能、改数据目录迁移策略等会改变交付结果的决定，先把准确差异和后果写明交给用户判断。

## 8. 查阅资料与再次核查入口

以下官方/维护者入口在编写时查阅，执行时再次确认具体版本；示例的 `latest/main` 只用于阅读，不能直接成为未锁定交付依赖。

- GPUI：<https://gpui.rs/>；官方 README：<https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md>。框架仍在演进，API 兼容性需要构建验证。
- Longbridge 组件：<https://github.com/longbridge/gpui-kit>。本次访问旧 `gpui-component` 地址发生重定向；先遵循维护者当前匹配组合，勿混用旧教程。
- portable-pty：<https://docs.rs/portable-pty/latest/portable_pty/>。PTY 创建和 I/O API 可复用；进程树停止能力单独验证。
- MCP 官方 Rust SDK：<https://github.com/modelcontextprotocol/rust-sdk>。验证实际版本的 Streamable HTTP 服务接入方式。
- tray-icon：<https://github.com/tauri-apps/tray-icon>。macOS 图标和事件循环需要主线程处理，不能照搬其他平台用法。
- 终端状态库候选：<https://github.com/alacritty/alacritty/tree/master/alacritty_terminal>；SQLite：<https://github.com/rusqlite/rusqlite>；文件监听：<https://github.com/notify-rs/notify>。这些是候选核查入口，本文不声称已验证它们与所选 GPUI 版本组合。

## 9. 本文完成度说明

本文件完成的是备份与重建的执行设计和步骤，不是实际迁移结果。编写文档期间未创建备份分支或标签、未 push、未创建 worktree、未迁移用户数据库、未创建 Rust 产品工程。交给 DeepSeek 后从阶段 0开始，以实际验证记录替代本文中的待执行状态。
