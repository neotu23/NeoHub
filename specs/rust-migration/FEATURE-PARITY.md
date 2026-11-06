# 功能对照表（Rust + GPUI 迁移）

> 本表是阶段 0 从旧版源码逐项盘出的功能清单，也是后续每个阶段回填"Rust 侧进度"的唯一表格。
> 状态图例：`已验证` = 有测试或人工实测支撑；`部分` = 代码存在但缺少直接验证；`未开始` = Rust 侧无实现；
> `阻塞` = 有明确外部阻塞（写入 PROGRESS.md 的原因编号）。
>
> Rust 目标位置相对 `native/crates/`。依赖方向：domain ← storage ← core ← {mcp, ui} ← desktop。
> 最后更新：2026-11-06（阶段 0）。

## 1. 领域模型与契约

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| D1 | `App`/`Task`/`RunningProcess`/`RunningTask`/`EnvVar`/`NodeInstallation`/`DetectionResult`/`WorkspaceCandidate` 类型 | `src/shared/types.ts` | 已验证（类型 + 51 例 MCP 往返） | `domain/src/{ids,models}.rs` | 已验证（`domain/src/models.rs`；serde 形状有断言） |
| D2 | `ProcessState` 六态 `idle/starting/running/exiting/exited/crashed` | `types.ts`, `lib/processState.ts` | 已验证（`AppOrchestrator.state.test.ts`） | `domain/src/models.rs` | 已验证（六态字符串/序列化测试） |
| D3 | `ReadinessSignal` 六种 `none/port/log/http/exit/delay` | `types.ts`, `services/readiness/*` | 已验证（`HttpReadiness.test.ts` 4 例 + MCP set_readiness） | `domain/src/models.rs` | 已验证（六种 readiness 编码 + 往返测试） |
| D4 | `NodeVersionPref` auto/system/explicit + `PackageManager` × 4 | `types.ts`, `NodeResolver.ts`, `PMDetector.ts` | 已验证（`NodeResolver.test.ts` 5 例） | `domain/src/models.rs` | 已验证（{kind:…} 编码与旧库一致） |
| D5 | `SettingsState` 17 个字段 + `McpStatus` | `ipc.ts`, `Settings.ts` | 已验证（`Settings.test.ts`） | `domain/src/settings.rs` | 部分（domain + `storage/src/settings.rs` 读写，重启不丢；UI 未接） |
| D6 | IPC 契约：**59 invoke + 22 event** 频道，preload 白名单与类型同源 | `ipc.ts`, `preload/index.ts` | 已验证（`satisfies` 编译期对齐） | `core/src/commands.rs` + `events.rs` + `docs/interfaces.md`| 部分（`CoreCommand`/`CoreReply`/`CoreEvent`/`CoreSnapshot`/`CoreHandle` 已落地并测试；59+22 全量映射随阶段推进补齐） |

## 2. 项目 / 任务 / 分组（CRUD）

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| A1 | 应用 CRUD（list/add/update/remove/findByPath）、身份字段 pinning 校验 | `ipc/index.ts`, `AppRegistry.ts` | 已验证 | `core/src/catalog/` | 已验证（core catalog：列出/读取/更新/删除/按路径查） |
| A2 | 原子创建 `apps:create`：应用 + 首个任务 + 多任务 + 环境变量，单事务 | `AppRegistry.ts`, `AddAppDrawer.tsx` | 已验证 | `core/src/catalog/` | 已验证（单事务创建 app+任务+env，失败不留半个 app） |
| A3 | realpath 去重、路径不存在报错 | `AppRegistry.ts` | 部分（无独立测试） | `core/src/catalog/` | 已验证（realpath 去重 + 非目录/不存在报错，含 symlink 用例） |
| A4 | 任务 CRUD + 重排 `tasks:reorder`、`TaskRegistry` 字段冻结 | `TaskRegistry.ts` | 已验证（MCP add_task/remove_task） | `core/src/catalog/` | 已验证（任务 CRUD/重排；重排必须全覆盖） |
| A5 | 任务依赖 `dependsOn` + 拓扑/环/未知依赖校验 | `topo.ts`, `topo.test.ts`（8 例） | 已验证 | `core/src/orchestrator/topology.rs` | 部分（环/未知依赖/自依赖检测已实现；拓扑启动顺序在阶段 7） |
| A6 | 文件夹分组：list/rename/clear、`apps.folder` | `AppRegistry.ts`, `0005_app_folders.sql` | 已验证 | `core/src/catalog/` | 已验证（list/rename/clear，清空不动磁盘目录） |
| A7 | 扫描导入 `apps:scanFolder`（浅层扫描 package.json） | `DetectionService.ts`, `ImportProjectsDrawer.tsx` | 部分 | `core/src/environment/detect.rs` | 部分（monorepo 工作区展开与检测已实现；`scanFolder` 浅层扫描待接） |
| A8 | 项目检测 `apps:detect`：scripts、PM、`.env`、engines、monorepo workspaces | `DetectionService.ts`, `PMDetector.ts` | 已验证 | `core/src/environment/detect.rs` | 已验证（PM/scripts/建议脚本/env 文件/engines/workspaces 全量检测，含损坏 JSON 与软链） |

## 3. 进程 / PTY / 生命周期

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| P1 | PTY 启动：脚本模式（PM + script）与 custom 命令模式 | `TaskRunner.ts` | 已验证（`AppOrchestrator.e2e.test.ts` 真起进程） | `core/src/process/spawn_spec.rs` | 部分（`SpawnSpec` 命令拼装 + PTY openpty/spawn 已实现；catalog/detect → spec 的接线在 service 层） |
| P2 | 同任务启动去重（并发 start 只 spawn 一次） | `TaskRunner.ts` | 已验证（e2e 断言） | `core/src/process/runner.rs` | 已验证（并发 start 串行化，第二个被拒绝，只 spawn 一次） |
| P3 | 停止：SIGTERM → grace(kill_grace_ms) → SIGKILL，**整棵进程树**；不误杀自身 | `TaskRunner.ts`（`tree-kill`） | 已验证 | `core/src/process/process_tree.rs` | 已验证（TERM→grace→KILL 作用于整个进程组；含祖父/子进程、忽略 TERM、拒绝自身组） |
| P4 | PTY resize（最小 2×2，初始 120×30） | `TaskRunner.ts`, `task:resize` | 部分 | `core/src/process/runner.rs` | 部分（初始 120×30、resize 下限 2×2、ioctl 已实现；UI 拖拽接线在阶段 11） |
| P5 | 退出结算：用户停止 / 正常退出 / 崩溃三态，只结算一次 | `TaskRunner.ts`, `AppOrchestrator.ts` | 已验证（`AppOrchestrator.state.test.ts`） | `core/src/process/runner.rs` | 已验证（用户停止/正常退出/崩溃三态 + generation 校验只结算一次） |
| P6 | 应用级启停重启 `proc:start/stop/restart` + 反向拓扑停止 | `AppOrchestrator.ts` | 已验证 | `core/src/orchestrator/` | 未开始 |
| P7 | 任务级启停 `task:start/stop`（手动） | `ipc/index.ts` | 部分 | `core/src/service.rs` | 未开始 |
| P8 | 运行历史写入 `run_history` + 每应用上限清理 + 重启后恢复 stopped/crashed | `RunHistory.ts`, `0003_run_history_tasks.sql` | 已验证（`AppOrchestrator.state.test.ts` 的 boot seeding） | `storage/src/history.rs`| 已验证（start 写行、退出结算、shutdown 关闭残留行并按上限 prune；5 个 service 测试覆盖） |
| P9 | 优雅退出：确认 → 反向停止全部受管任务 → 写历史 → 关库 | `index.ts` before-quit | 部分 | `core/src/service.rs`| 已验证（拒绝新启动 → 停止全部 → 写历史 → checkpoint → 关库；窗口关闭不触发） |

## 4. 日志与终端

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| L1 | 主进程有界日志环：每任务 5 MiB / `log_ring_size` 行、全局 100 MiB、退出后淘汰 | `LogBuffer.ts` | 已验证（`LogBuffer.test.ts`） | `core/src/logs/buffer.rs`| 已验证（每任务 5 MiB / 全局 100 MiB / 单 chunk 10 KiB / 退出 10 分钟淘汰，按真实 UTF-8 字节计） |
| L2 | 可见性订阅 `task:subscribeLogs/unsubscribeLogs`，只为可见任务推送 | `ipc/index.ts` | 部分 | `core/src/logs/subscription.rs`| 部分（`bootstrap_logs` + 事件流已实现，脱离订阅仍采集；"只为可见任务推送" 的门控随 UI 在阶段 11 接） |
| L3 | 合批推送 ~30 Hz / 16 KiB | `ipc/index.ts` | 部分 | `core/src/logs/buffer.rs` | 未开始 |
| L4 | 读取/尾读/清空缓冲 `readBuffer`/`tailBuffer`/`clearBuffer` | `ipc/index.ts` | 部分 | `core/src/logs/`| 已验证（`GetLogs`/`ClearLogs`/`BootstrapLogs` 命令；清空不影响进程） |
| L5 | 跨任务全局日志搜索 `logs:searchAll`（正则 + limit） | `mcp/tools.ts`, `GlobalLogSearch.tsx` | 部分 | `core/src/logs/search.rs`| 已验证（`SearchLogs` 支持单任务与全局，字面量/正则/大小写/limit，非法正则报错） |
| L6 | 单任务日志过滤视图（虚拟化 + 高亮 + 行号） | `LogSearchView.tsx` | 部分 | `ui/src/views/log_search.rs` | 未开始 |
| L7 | 终端渲染：ANSI 16/256/真彩、CR 覆写、滚动历史、自动跟随、选择复制、字号 | `LogTerminal.tsx`（xterm 6） | 部分（截图） | `ui/src/terminal/`| 部分（`vte` 解析 + 网格/scrollback/宽字符/CR 覆写/清行/光标移动已实现并测试；GPUI 绘制在阶段 11） |
| L8 | 日志复制/保存导出 | `AppDetail.tsx` | 部分 | `ui/src/terminal/` | 未开始 |
| L9 | 崩溃日志钉住最后 200 行 + 重启入口 + 端口冲突提示 | `CrashPin.tsx` | 部分 | `ui/src/views/app_detail.rs`| 部分（崩溃/退出后日志仍可搜索查看，10 分钟后淘汰；崩溃钉住入口随 UI） |

## 5. 环境变量与密钥

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| E1 | 三层作用域 Global → App → Task 覆盖（`env_vars.scope` 部分唯一索引） | `EnvStore.ts`, `EnvBuilder.ts`, `0006_env_scope_unique.sql` | 已验证（`EnvLayering.test.ts` 9 例） | `core/src/environment/env_builder.rs` | 未开始 |
| E2 | `.env*` 文件优先级 `.env` → `.env.development` → `.env.local` → `.env.development.local` | `src/shared/dotenv.ts` | 已验证（EnvLayering） | `core/src/environment/dotenv.rs` | 已验证（`.env`→`.env.development`→`.env.local`→`.env.development.local`，多行/引号/转义有测试） |
| E3 | 进程控制键保护：项目 `.env` 不能覆盖 `PATH/NODE_OPTIONS/NODE_PATH/DYLD_*/LD_*` | `EnvBuilder.ts` | 已验证 | `core/src/environment/env_builder.rs` | 已验证（`PATH/NODE_OPTIONS/NODE_PATH/DYLD_*/LD_*` 被项目文件丢弃；UI 变量仍可覆盖） |
| E4 | 强制 `FORCE_COLOR=1`、`TERM=xterm-256color` | `EnvBuilder.ts` | 已验证 | `core/src/environment/env_builder.rs` | 已验证（`FORCE_COLOR=1`、`TERM=xterm-256color` 强制写入） |
| E5 | 秘密值加密存储（Electron `safeStorage` → `enc1:` 前缀） | `EnvStore.ts` | 部分（依赖 Keychain，未在测试中断言解密） | `storage/src/secrets.rs` + `platform-macos/src/keychain.rs` | 已验证（`nhk1:` + AES-256-GCM，master key 在 login Keychain；真实 round-trip 实测通过；启动来源头待接） |
| E6 | 密钥不可用时报 `SecretUnavailable` 并阻止受影响任务启动 | 计划要求（旧版行为需在阶段 4 复核） | **待复核** | `core/src/environment/` | 部分（`read_secret_value` 明确拒绝 `enc1:` 并报告 Locked/Corrupt；启动流程拦截待接） |
| E7 | 环境变量编辑 UI：三 tab、启用开关、秘密遮挡、备注、批量粘贴、合并预览 | `EnvEditor.tsx` | 部分（截图 missing，见 BASELINE §5） | `ui/src/views/env_editor.rs` | 未开始 |
| E8 | `.env` 文件变化推送 `env:fileChanged` + 横幅 | `EnvFileWatcher.ts` | 部分 | `core/src/watch/environment.rs`| 已验证（notify 非递归监听 `.env*`，去抖 250 ms，非 `.env` 文件不产生事件） |

## 6. Node 环境与 PATH

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| N1 | 版本管理器检测 nvm/fnm/volta/asdf/system + `node:list` | `NodeResolver.ts` | 已验证（5 例，含特殊字符路径） | `core/src/environment/node_resolver.rs` | 部分（nvm/fnm/volta/asdf/system 扫描 + 降序去重已实现并测试；`node:list` 命令层未接） |
| N2 | 版本选择规则 `.nvmrc` / `.node-version` / `engines.node`（semver）+ 不自动安装 | `NodeResolver.ts` | 已验证 | `core/src/environment/node_resolver.rs` | 已验证（`.nvmrc`/`.node-version` 精确与 major-only、`engines.node` 范围、缺版本报错、不自动安装） |
| N3 | Finder 启动无终端 PATH：登录 shell PATH 探测（有超时与缓存） | `PathProbe.ts` | 已验证（3 例） | `core/src/environment/path_probe.rs` | 部分（探测/哨兵解析/超时/缓存已实现并测试；尚未接到启动流程） |
| N4 | `node:resolve` 单应用解析 | `ipc/index.ts` | 部分 | `core/src/environment/` | 部分（`NodeResolver::resolve` 已实现并测试，含 system/explicit/auto 分支） |

## 7. 就绪与编排

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| R1 | `none` / `port` / `log`(regex+flags) / `http`(2xx/3xx 或指定状态) / `exit` / `delay` 六种探测 | `services/readiness/*` | 已验证（`HttpReadiness.test.ts`） | `core/src/orchestrator/readiness.rs`| 已验证（none/port/log/http/exit/delay 六种；port/log 用真实 fixture 集成验证，http 含 503→200 预热） |
| R2 | readiness 超时 `readiness_timeout_ms`（默认 60000）与取消 | `AppOrchestrator.ts` | 已验证 | `core/src/orchestrator/`| 已验证（读取 `readiness_timeout_ms`，超时/任务退出即中止；start 被 stop 取消） |
| R3 | 启动失败回滚：只回收本次启动新增的任务 | `AppOrchestrator.ts` | 部分 | `core/src/orchestrator/state.rs`| 已验证（回滚只停止本次启动新增的任务，用户先前手动启动的任务保持运行） |
| R4 | 依赖分层并行启动、disabled 任务跳过、autoStart 不重复启动 | `AppOrchestrator.ts` | 部分 | `core/src/orchestrator/state.rs`| 部分（拓扑分层并行、disabled 跳过、同任务已在运行不重复启动已验证；app autoStart 接线在阶段 12/15） |
| R5 | readiness 诚实性：未配置探测时 MCP 返回 `readinessVerified: null` | `mcp/tools.ts` | 已验证（`tools.test.ts` 真实断言） | `core/src/orchestrator/state.rs`| 已验证（`none` 记为 `(kind=none, verified=false)`，MCP 不会谎报 readinessVerified=true） |

## 8. 监控、端口、监听

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| M1 | CPU/内存采样 `task:stats`（pidusage 1 Hz，批量 1 fork/tick） | `StatsMonitor.ts` | 部分（截图 sparkline） | `core/src/monitor/stats.rs`| 已验证（`ps` 批量采样 + own/tree 口径分离；后台循环默认 1000 ms／下限 200 ms，空闲不采样；`TaskStats`/`TaskPorts` 事件与快照缓存） |
| M2 | 端口探测 `task:ports`：`lsof` 进程表 + stdout 提示，3 次未确认淘汰 | `PortDetector.ts` | 已验证（`PortDetector.test.ts` 7 例） | `core/src/monitor/ports.rs`| 已验证（一次批量 `lsof`，多级后代端口归属实测；进程退出竞态采样为 0） |
| M3 | confirmed owner 与"仅日志提示"的端口区分 | `PortDetector.ts`, `PortChip.tsx` | 已验证 | `core/src/monitor/ports.rs`| 已验证（confirmed 与 hinted 分离，未确认提示连续 3 次淘汰） |
| M4 | 变更自动重启 `RestartWatcher`（chokidar + 500 ms 去抖 + glob/ignore） | `RestartWatcher.ts` | 已验证（`RestartWatcher.test.ts`） | `core/src/watch/restart.rs`| 已验证（notify 递归监听、去抖 500 ms、忽略 node_modules/.git/.next/dist/build/target、`**/` 兼容 picomatch） |

## 9. 桌面系统集成

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| S1 | 关窗销毁面板进入菜单栏常驻，Dock 隐藏，后台继续运行 | `index.ts`, `PanelWindow.ts` | 已验证（`panel-lifecycle.test.ts`, `panel-session.test.ts`） | `desktop/src/window_session.rs` + `platform-macos/src/{tray,dock}.rs` | 未开始 |
| S2 | 关窗保存屏障 `panel:prepareClose` / `panel:closeReady`（保存失败不关窗） | `panelSaves.ts`, `DebouncedSaveQueue.ts` | 已验证（`DebouncedSaveQueue.test.ts`） | `ui/src/state/save_barrier.rs` | 未开始 |
| S3 | 菜单栏：聚合状态、逐项目启停、Stop all、Open、Quit | `TrayController.ts` | 已验证（`TrayController.test.ts`） | `platform-macos/src/tray.rs` | 未开始 |
| S4 | 单实例锁（第二实例只激活不抢库） | `index.ts` requestSingleInstanceLock | 部分 | `desktop/src/session_lock.rs` | 未开始 |
| S5 | 应用菜单与快捷项：⌘, 设置 / ⌘N 添加 / ⌘⇧N 新建文件夹 / 检查更新 / 打开日志 | `menu.ts` | 部分 | `platform-macos/src/` + `desktop/src/` | 未开始 |
| S6 | 深链接 `devharbor://open?id=…&path=…`、`start?id=…`（start 需用户确认） | `DeepLinks.ts` | 已验证（`DeepLinks.test.ts`） | `platform-macos/src/deep_links.rs` | 未开始 |
| S7 | Open In：Finder/Terminal/VS Code/Cursor/Sublime + 能力探测 | `OpenIn.ts` | 部分 | `platform-macos/src/open_in.rs` | 未开始 |
| S8 | 崩溃/就绪桌面通知（受设置控制） | `index.ts` Notification | 部分 | `platform-macos/src/notifications.rs` | 未开始 |
| S9 | 登录启动 `launch_at_login` + 每应用 `autoStart`（迁移 0007） | `Settings.ts`, `0007_app_autostart.sql` | 部分 | `platform-macos/src/login_item.rs` | 未开始 |
| S10 | 自动更新（electron-updater + UpdateBanner 进度/错误/说明） | `Updater.ts`, `UpdateBanner.tsx` | 已验证（`Updater.test.ts`，无 live feed） | `platform-macos/src/updater.rs` | 未开始 |
| S11 | 崩溃/未捕获异常日志文件（`~/Library/Logs/…`）+ 打开日志目录 | `Logger.ts` | 部分 | `core/src/` + `platform-macos/` | 未开始 |

## 10. MCP 服务（22 个工具，全量保留）

| # | 工具 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| 1 | `get_overview` | `mcp/tools.ts` | 已验证（`tools.test.ts`） | `mcp/src/tools.rs` | 未开始 |
| 2 | `list_apps` | 同上 | 已验证 | 同上 | 未开始 |
| 3 | `get_app` | 同上 | 已验证 | 同上 | 未开始 |
| 4 | `list_running` | 同上 | 已验证 | 同上 | 未开始 |
| 5 | `get_logs` | 同上 | 已验证 | 同上 | 未开始 |
| 6 | `search_logs` | 同上 | 已验证 | 同上 | 未开始 |
| 7 | `list_env_vars` | 同上 | 已验证 | 同上 | 未开始 |
| 8 | `get_ports` | 同上 | 已验证 | 同上 | 未开始 |
| 9 | `scan_folder` | 同上 | 已验证 | 同上 | 未开始 |
| 10 | `list_node_versions` | 同上 | 已验证 | 同上 | 未开始 |
| 11 | `add_app` | 同上 | 已验证 | 同上 | 未开始 |
| 12 | `update_app` | 同上 | 已验证 | 同上 | 未开始 |
| 13 | `remove_app` | 同上 | 已验证 | 同上 | 未开始 |
| 14 | `add_task` | 同上 | 已验证 | 同上 | 未开始 |
| 15 | `remove_task` | 同上 | 已验证 | 同上 | 未开始 |
| 16 | `set_env_var` | 同上 | 已验证 | 同上 | 未开始 |
| 17 | `set_readiness` | 同上 | 已验证 | 同上 | 未开始 |
| 18 | `start_app` | 同上 | 已验证 | 同上 | 未开始 |
| 19 | `stop_app` | 同上 | 已验证 | 同上 | 未开始 |
| 20 | `restart_app` | 同上 | 已验证 | 同上 | 未开始 |
| 21 | `start_task` | 同上 | 已验证 | 同上 | 未开始 |
| 22 | `stop_task` | 同上 | 已验证 | 同上 | 未开始 |
| 23 | Streamable HTTP `/mcp` 端点、loopback + Host/Origin 校验、Bearer Token、可选 HTTPS（自签证书） | `McpService.ts`, `gate.ts`, `tls.ts`, `token.ts` | 已验证（`gate.test.ts` 10 例、`tls.test.ts` 3 例） | `mcp/src/{server,auth,tls}.rs` | 未开始 |
| 24 | 脱敏：secret/Token 不进日志尾、搜索结果与错误 | `hygiene.ts` | 已验证（`hygiene.test.ts` 9 例） | `mcp/src/redaction.rs` | 未开始 |

## 11. 数据库与迁移

| # | 功能 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| B1 | SQLite（WAL）+ 7 次迁移 `0001_init` → `0007_app_autostart`，`_schema_migrations` 版本记录 | `db/index.ts`, `db/migrations/*` | 已验证（启动即跑） | `storage/src/migrations.rs` | 已验证（7 次迁移原样执行，版本名与旧库一致，重开幂等） |
| B2 | TS 补数 `backfill.ts`（老应用 → 单任务、env_overrides 延迟迁移） | `db/backfill.ts` | 部分 | `storage/src/` | 已验证（Rust 重写补数：custom > script，无命令不造任务，幂等） |
| B3 | 外键与级联（app 删除 → task/env/history） | `0001`–`0007` | 部分 | `storage/src/` | 已验证（删除 app 级联清空 tasks/env/history） |
| B4 | `db:export` / `db:reset` / `db:path`（WAL 安全导出） | `ipc/index.ts` | 部分 | `storage/src/` + `ui/src/views/settings.rs` | 未开始 |
| B5 | 历史用户目录发现（`NeoHub` → `DevHarbor` → `devharbor`，含 `devharbor.db`） | `AppIdentity.ts` | 已验证（`AppIdentity.test.ts` 4 例） | `storage/src/import/legacy_db.rs` | 未开始 |
| B6 | 旧库 + 密钥迁移（`enc1:` safeStorage → 新版本前缀密文） | 计划阶段 13 | 未实现（旧版没有导出入口） | `storage/src/import/` | 未开始 |

## 12. 界面视图与组件

| # | 视图/组件 | 旧版源码依据 | 旧版基线 | Rust 目标 | Rust 状态 |
|---|---|---|---|---|---|
| U1 | 外壳 + 侧栏（分组/标签双模式、折叠、排序、筛选 ⌘P、⋮ 菜单、拖拽分组） | `Sidebar.tsx`, `sortApps.ts` | 部分（截图） | `ui/src/views/{shell,sidebar}.rs`| 部分（侧栏分组/标签/排序/筛选/选中态已实现并测试；折叠、⋮ 菜单、拖拽分组未做） |
| U2 | 仪表盘（汇总卡、项目卡、CPU sparkline、端口 chip、标签筛选、全部停止） | `Dashboard.tsx` | 部分（截图） | `ui/src/views/dashboard.rs`| 部分（仪表盘汇总卡 + 项目卡 + 端口 chip + 状态色已渲染真实快照；sparkline/空态/标签下拉未做） |
| U3 | 项目详情（任务 tabs、启停/重启、端口、历史、日志区、Open In、复制路径） | `AppDetail.tsx`, `TaskTabs.tsx` | 部分（截图） | `ui/src/views/app_detail.rs` | 未开始 |
| U4 | 添加项目抽屉（浏览 → 检测 → 命名/Node/PM/脚本 → 环境；monorepo 每包一任务） | `AddAppDrawer.tsx` | 部分 | `ui/src/views/add_app.rs` | 未开始 |
| U5 | 扫描导入抽屉（候选列表、已注册标记、批量添加） | `ImportProjectsDrawer.tsx` | 部分 | `ui/src/views/import_projects.rs` | 未开始 |
| U6 | 任务编辑器抽屉（命令/脚本/工作目录/依赖/readiness/oneShot/enabled/重排） | `TaskEditor.tsx` | 部分 | `ui/src/views/task_editor.rs` | 未开始 |
| U7 | 应用配置抽屉（名称/颜色/文件夹/标签/自动重启/watch globs/autoStart/Node/PM） | `AppConfigDrawer.tsx`, `TagInput.tsx`, `FolderSelect.tsx` | 部分 | `ui/src/views/app_config.rs` | 未开始 |
| U8 | 环境变量编辑器（见 E7） | `EnvEditor.tsx` | 部分 | `ui/src/views/env_editor.rs` | 未开始 |
| U9 | 设置抽屉（17 项设置 + 危险区 db 导出/重置 + MCP 状态/Token） | `SettingsDrawer.tsx` | 部分（截图） | `ui/src/views/settings.rs` | 未开始 |
| U10 | 运行历史子视图（含 Rerun） | `AppDetail.tsx` History | 部分 | `ui/src/views/run_history.rs` | 未开始 |
| U11 | 命令面板 ⌘K（Apps/Start/Stop/Restart/Actions 分组） | `CommandPalette.tsx`（cmdk） | 部分 | `ui/src/components/` | 未开始 |
| U12 | 全局日志搜索 ⌘⇧F | `GlobalLogSearch.tsx` | 部分 | `ui/src/views/log_search.rs` | 未开始 |
| U13 | 通用组件：Toast、ErrorBanner、ErrorBoundary、ContextMenu、PromptModal、StatusDot、PortChip、NodeVersionPicker、OpenInMenu、EmptyState、CrashPin、UpdateBanner | 对应 `components/*.tsx` | 部分（截图） | `ui/src/components/` | 未开始 |
| U14 | 主题系统（Radix slate 语义变量、light/dark/system、终端浅色主题） | `styles.css`, `useTheme.ts`, `tailwind.config.ts` | 部分（截图 light/dark） | `ui/src/theme.rs`| 部分（两套调色板 + 间距/圆角/字号常量已接入视图；主题切换跟随设置） |
| U15 | 国际化 en / zh-CN + `system` 跟随系统（19 组文案命名空间） | `i18n/locales/*`, `useLanguage.ts` | 已验证（`i18n` 4 个测试文件 17 例） | `ui/locales/` | 未开始 |
| U16 | 无障碍：焦点陷阱 `useDialog`、Esc、键盘可导航菜单、`aria-label`、非颜色状态线索 | `hooks/useDialog.ts` | 部分 | `ui/src/components/` | 未开始 |
| U17 | 窗口尺寸/位置持久化 `window-state.json` | `index.ts` loadBounds/saveBounds | 部分 | `desktop/src/window_session.rs` | 未开始 |
| U18 | 导航会话持久化（view/selectedAppId/selectedTaskByApp、排序、分组模式） | `store.ts`, `sortApps.ts` | 已验证（`panel-session.test.ts`） | `ui/src/state/session.rs`| 已验证（`session.json` 持久化 view/selectedAppId/selectedTaskByApp/sort/groupBy，失效引用回退仪表盘） |
| U19 | 编辑草稿与防抖保存队列（MCP 刷新不覆盖在途编辑） | `DebouncedSaveQueue.ts`, `useDebouncedSave.ts` | 已验证（`DebouncedSaveQueue.test.ts`） | `ui/src/state/{edit_session,save_barrier}.rs` | 未开始 |

## 13. 旧版已知缺陷（迁移时不得误记为新引入）

| # | 现象 | 依据 | 迁移态度 |
|---|---|---|---|
| K1 | 交互式 shell 输入未接入：终端只展示输出，不能向 PTY 发键盘输入 | `ipc.ts` 无写 PTY 频道 | 不扩范围，Rust 首版同样只做展示（阶段 11 明确） |
| K2 | 自动更新没有 live feed，`Updater.test.ts` 只覆盖检查逻辑 | `specs/PROGRESS.md` Phase 4 备注 | Rust 首版提供真实检查版本/跳转下载，自动安装可标记未完成 |
| K3 | 就绪探测中的 `log` 正则语义按 JS RegExp 解释，Rust 正则存在差异 | `LogReadiness.ts` | 阶段 7 单列兼容报告，不静默改语义 |
| K4 | 旧 `specs/PROGRESS.md` 矩阵停留在 2026-07-10 且使用旧名 DevHarbor | 本次复核 | 以源码为准；阶段 17 随 README 一并更新 |
| K5 | 运行期资源占用从未在本机隔离测量过 | BASELINE §6 | 阶段 16 用两侧隔离数据目录实测，不引用估算值 |
