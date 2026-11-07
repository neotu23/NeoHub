# Rust + GPUI 迁移执行进度

> 会话恢复入口：先读
> `docs/superpowers/plans/2026-11-06-neohub-rust-gpui-deepseek.md`、本文件、
> `specs/rust-migration/BASELINE.md`、`specs/rust-migration/FEATURE-PARITY.md`、
> `native/docs/DEPENDENCIES.md`（阶段 2 之后才存在），再 `git status` + `git log -5`。
> 已完成并验证的阶段不要重做；备份引用已存在且指向同一提交时复用。

---

### 阶段 0：确认起点和旧版验收基线
- 状态：**已验证**
- 工作分支与目录：`main` @ `/Users/neil/Code/NeoHub`
- 最近提交：`66d8519` `docs: define Rust GPUI migration baseline`；随后 `58f88de` `build: ignore local worktree dir and Rust build output`
- 实际修改文件：新增 `docs/superpowers/plans/2026-11-06-neohub-rust-gpui-deepseek.md`（本计划）、
  `specs/rust-migration/BASELINE.md`、`specs/rust-migration/FEATURE-PARITY.md`、
  `specs/rust-migration/PROGRESS.md`、`specs/rust-migration/screenshots/{dashboard,app,settings,empty,light}.png`；
  修改 `specs/PROGRESS.md`（加入迁移索引小节）。**未**修改任何业务代码。
- 验证命令、时间与退出码（2026-11-06T12:28Z–12:35Z UTC）：
  - `git status --short --branch` → `## main...origin/main [ahead 1]`，唯一未跟踪文件是本计划文档
  - `git rev-parse HEAD` → `02acedb290a7edad463a0497deff097e16bd4244`
  - `git fetch origin` → exit 0；`git ls-remote --heads origin` 只返回 `refs/heads/main` = `0cd4a2e…`，确认远程是 `neotu23/NeoHub`
  - `pnpm typecheck` → exit 0
  - `pnpm lint` → exit 0（无警告）
  - `pnpm test` → exit 0，28 files / **202 tests** 全通过（5.74s）
  - `pnpm build` → exit 0（main 276.45 kB / preload 2.88 kB / renderer 1,510.93 kB + 50.76 kB CSS）
- 人工验证及截图：`screencapture` 在本会话不可用（无屏幕录制权限）；改用仓库自带 Electron
  加载 `out/renderer` + 合成数据 preload + `capturePage()` 生成 5 张真实渲染截图，存
  `specs/rust-migration/screenshots/`。**真实旧版进程级闭环未验证**：旧版无条件
  `app.setPath('userData', …)`，`--user-data-dir` 与 `HOME` 均无法隔离，而真实
  `~/Library/Application Support/devharbor` 内有 13 MB 用户数据，按"不读写生产数据库"的约束主动不启动。
  原因与复现细节写在 BASELINE.md §5/§6。
- 与旧版不同之处：无（本阶段未触碰产品代码）。
- 未解决问题及所属阶段：
  1. 旧版运行期资源基线（footprint/RSS/CPU/启动时间）缺失 → 阶段 16（需先定隔离方案）
  2. 旧版 GUI 进程级闭环对照缺失 → 阶段 16
  3. `xcode-select` 指向 CommandLineTools，`xcrun --find metal` 失败；已验证
     `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer` 可拿到 Xcode 27.1 的 metal
     → 阶段 2 用它构建并在 DEPENDENCIES.md 记录，仍失败再申请 `sudo xcode-select -s`
- 下一项具体动作：进入阶段 1A —— 在 `02acedb` + 本阶段提交后的新 HEAD 上创建
  `legacy/electron` 分支与 annotated tag `electron-final-20261106`，推送并核对 `^{}` 解引用，
  再创建离线 bundle 与 worktree。
- 完成情况：已全部执行，证据见 `specs/rust-migration/BACKUP.md`。

---

### 阶段 1：远程代码备份、数据备份、独立工作区
- 状态：**已验证**
- 工作分支与目录：`main` @ `/Users/neil/Code/NeoHub`（备份引用）→ Rust 工作区
  `/Users/neil/Code/NeoHub-Rust` @ `rewrite/rust-gpui`
- 最近提交：`docs: record migration backups and create Rust worktree`
- 实际修改文件：新增 `specs/rust-migration/BACKUP.md`；修改 `.gitignore`（`.worktrees/`、`native/target/`，
  单独提交 `58f88de`）；更新本文件。**未**修改 `src/` 或任何产品代码。
- 验证命令、时间与退出码（2026-11-06T12:34Z–12:40Z UTC）：
  - `git push --atomic origin refs/heads/legacy/electron refs/tags/electron-final-20261106` → exit 0（两个引用均为 new，无 force push）
  - `git ls-remote origin refs/heads/legacy/electron 'refs/tags/electron-final-20261106*'` → branch `58f88de…`；tag `^{}` = `58f88de…`
  - `git bundle create … --all` + `git bundle verify` → `The bundle records a complete history`（9 refs）；SHA-256 `d11ce859…`；临时目录 clone 后 `electron-final-20261106^{commit}` = `58f88de…`
  - 用户数据副本 `PRAGMA integrity_check` = `ok`，`PRAGMA foreign_key_check` 空，7 条迁移，行数 apps 4 / tasks 4 / env_vars 0 / run_history 9 / settings 7
  - `git worktree add -b rewrite/rust-gpui /Users/neil/Code/NeoHub-Rust 58f88de` → 成功，`git worktree list` 显示两个 worktree
- 人工验证及截图：不需要截图；恢复演练（bundle clone + `fsck`）与哈希核对已记录在 BACKUP.md。
- 与旧版不同之处：无（本阶段未触碰产品代码；旧版 `main` 保持不变）。
- 未解决问题及所属阶段：
  1. bundle 与 userData 备份目录只存在本机 `~/Backups/…`，没有第二份异地副本 → 交付前若需要，请用户决定是否复制到外置盘
  2. Keychain 加密的 secret 无法随文件备份迁移（当前 `env_vars` 为 0 行，暂无实际影响）→ 阶段 13
- 下一项具体动作：进入阶段 2 —— 在 `/Users/neil/Code/NeoHub-Rust/native/` 建立 Cargo workspace，
  先查 GPUI 与 Longbridge 组件库当前官方示例，锁定版本组合（`DEVELOPER_DIR` 指向 Xcode 27.1 以获得 metal 工具链），
  用最小窗口验证后写 `native/docs/DEPENDENCIES.md` 与 `interfaces.md`。

---

### 阶段 2：锁定依赖并验证原生宿主
- 状态：**已验证**（宿主链路），视觉/交互确认**受限**（见下）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`，代码在 `native/`
- 最近提交：`build: establish native GPUI workspace`
- 实际修改文件：`native/Cargo.toml`、`native/Cargo.lock`、`native/rust-toolchain.toml`、
  `native/crates/{ui,desktop}/{Cargo.toml,src/*}`、`native/crates/ui/src/theme.rs`、
  `native/crates/desktop/src/main.rs`、`native/docs/{DEPENDENCIES.md,interfaces.md}`、
  本文件与 `FEATURE-PARITY.md` 的 Rust 状态列。
- 验证命令、时间与退出码（2026-11-06T12:41Z–12:47Z UTC）：
  - `cargo check --workspace --locked` → exit 0（28.64s）
  - `cargo build -p neohub-desktop --release --locked` → exit 0（1m25s，产物 19 MB，strip 后）
  - `cargo tree -d` → 71 组上游重复依赖，已记录，不 patch
  - `cargo test -p neohub-ui --locked` → 2 passed（调色板/模式解析）
  - `cargo run -p neohub-desktop` → 进程驻留；`CGWindowList` 实测窗口 1280×853 @ (160,103)、
    `layer=0`，与 `WindowOptions` 一致
- 人工验证及截图：**像素/交互验证未完成**。`screencapture` 无屏幕录制权限、`osascript`
  无辅助功能权限（-25211）、GPUI 的 `render_to_image` 仅 `test-support` 下提供且 macOS 未实现。
  中文输入法组合/选择/剪贴板/Tab 焦点/暗色主题需在用户桌面会话人工确认，或由阶段 9 的无头
  视觉测试补上。已写入 `native/docs/DEPENDENCIES.md` §6，不当作已完成。
- 与旧版不同之处：
  1. 工具链要求 Rust **1.99.1**（1.97 stable 编译 `gpui-pre-macos` 报 E0658），已固定
  2. 构建需 `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`（CLT 里没有 metal）
  3. `gpui-kit 0.9.0` 钉住 `gpui-pre 0.3.11`（Zed 的 `gpui` crate 发布不规律）；只允许这一条依赖链
- 未解决问题及所属阶段：
  1. 视觉/交互人工确认（中文输入法、剪贴板、焦点）→ 用户会话或阶段 9 无头视觉测试
  2. macOS 11 真实机器运行未验证（只核对了 `LC_BUILD_VERSION` minos 11.0）→ 阶段 15 打包时复核
  3. 71 组重复依赖只记录不优化 → 阶段 16 评估
- 下一项具体动作：阶段 3 —— 在 `native/crates/domain` + `native/crates/storage` 落地数据模型与
  SQLite（原样保留 `0001`—`0007` 迁移与 `_schema_migrations` 版本名），先写 `legacy_import` /
  `catalog` 失败测试，再实现 `Storage::open` 与 `Catalog::create_app`。

---

### 阶段 3：数据模型、数据库与项目管理
- 状态：**已验证**
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`，代码在 `native/`
- 最近提交：`feat: port catalog and SQLite persistence`
- 实际修改文件：新增 `native/crates/domain/src/{lib,ids,models,settings,error}.rs`、
  `native/crates/storage/src/{lib,db,migrations,catalog,env,history,settings}.rs`、
  `native/crates/storage/migrations/legacy/0001…0007.sql`（与旧版逐字节相同）、
  `native/crates/storage/tests/{legacy_import,catalog,persistence}.rs`、
  `native/crates/core/src/{lib.rs,catalog/{mod,graph}.rs}`、`native/crates/core/tests/catalog.rs`；
  更新 `native/Cargo.toml`（加入 core/domain/storage 成员）、`FEATURE-PARITY.md`、本文件。
- 验证命令、时间与退出码（2026-11-06T13:05Z–13:30Z UTC）：
  - `cargo test -p neohub-storage --test legacy_import --locked` → exit 0，**7 passed**
    （7 次迁移/版本名/幂等/补数/三层 env 同键/级联/历史 task_id 可空/老库补列）
  - `cargo test -p neohub-storage --test catalog --locked` → exit 0，5 passed
  - `cargo test -p neohub-storage --test persistence --locked` → exit 0，2 passed
    （重启后项目/任务/分组/设置全在；损坏的设置单元格只回落该字段）
  - `cargo test -p neohub-core --test catalog --locked` → exit 0，**9 passed**
    （原子创建/回滚/路径不存在/重复路径/symlink 去重/依赖环/未知依赖/自依赖/
      one-shot 需 exit/删除有依赖者/重排全覆盖/清空文件夹不动磁盘）
  - `cargo test --workspace --locked` → exit 0，**44 passed / 0 failed**
  - `cargo clippy -p neohub-domain -p neohub-storage -p neohub-core --all-targets --locked` → 0 warnings
  - `cargo fmt --all` 已执行
- 人工验证及截图：不需要（纯数据层）。合成数据等价物是 `neohub-synth`（`demo-web`/`demo-worker`），
  在测试里以临时目录内的假项目目录体现，不执行任何真实用户项目。
- 与旧版不同之处：
  1. 原始缓冲/日志等尚未涉及；本阶段差异集中在数据层：`Storage::open(DataDir)` 取代模块级单例，
     数据目录默认 `~/Library/Application Support/NeoHub-Rust-Dev`，支持 `--data-dir`（旧版硬编码 userData）
  2. 事务边界显式化：`EnvStore` 检测 `is_autocommit()`，在外部事务内直接写，保证"创建 app + 任务 + env"
     是一个事务（旧版为 renderer 侧多次 IPC + 补偿删除）
  3. 时间戳统一 i64 毫秒（同旧版 `Date.now()`）
- 未解决问题及所属阶段：
  1. 旧库真实数据导入（含 `enc1:` 密文与 `_schema_migrations` 对照）→ 阶段 13
  2. `db:export` / `db:reset` / `db:path` 等价命令 → 阶段 9/10 接到命令层
  3. 任务删除/更新的 UI 错误定位（`DomainError.field`）→ 阶段 10
- 下一项具体动作：阶段 4 —— `native/crates/core/src/environment/`（detect / node_resolver /
  path_probe / dotenv / env_builder）+ `storage/src/secrets.rs` + `platform-macos/src/keychain.rs`，
  先建 fixture（空格/中文路径、软链、损坏 JSON、monorepo、缺 Node 版本、不同包管理器）再实现。

---

### 阶段 4：Node 环境、项目检测、环境变量和密钥接口
- 状态：**已验证**（本阶段 6/6 子项完成；接口接入启动流程属于阶段 5）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`，代码在 `native/`
- 最近提交：`feat: port dotenv parsing and env layering`
- 已完成并验证：
  - `native/crates/core/src/environment/dotenv.rs`：`.env` 解析（`export` 前缀、跨行双引号、
    转义展开、单引号字面、行内注释、非法键跳过、未闭合引号退化为单行），`is_secret_key` 启发式
  - `native/crates/core/src/environment/env_builder.rs`：五层覆盖（白名单 OS → 计算 PATH →
    项目 `.env` 文件 → global/app/task → 强制 `FORCE_COLOR`/`TERM`），进程控制键保护，
    disabled 跳过、空字符串保留
  - `native/crates/core/src/environment/path_probe.rs`：登录 shell 探测（`-l -i`）、哨兵提取、
    3 s 超时、成功缓存/失败可重试
- 验证命令与结果（2026-11-06T13:40Z–14:05Z UTC）：
  - `cargo test --workspace --locked` → exit 0，**64 passed / 0 failed**
    （其中 core 单元 25：dotenv 7、env_builder 7、path_probe 5、graph 5、migrations 等）
  - `cargo clippy -p neohub-core --all-targets --locked` → 0 warnings
  - `cargo fmt --all` 已执行
- 未完成（本阶段剩余）：`node_resolver`（nvm/fnm/volta/asdf/system + `.nvmrc`/`.node-version`/
  `engines.node` + semver 选择）、`detect`（package.json scripts、PM 探测、monorepo workspaces）、
  `storage/src/secrets.rs` + `platform-macos/src/keychain.rs`（假 SecretStore 正常/锁定/损坏三态 +
  `SecretUnavailable`），以及 `core/tests/{environment,detection}.rs` 的 fixture 套件。
- 第二批完成（2026-11-06T14:10Z–14:40Z UTC）：`node_resolver.rs`（五来源扫描、`.nvmrc`/
  `.node-version`/`engines.node` 选择、npm 风格 range 归一化、OR 组显式拒绝、3 s 版本探测超时）、
  `detect.rs`（PM/scripts/env 文件/engines/workspaces 展开、损坏 JSON 与越界 glob 防护）、
  `storage/src/secrets.rs`（`SecretStore` trait、`nhk1:` 新前缀、`enc1:` 明确拒绝、
  假实现正常/锁定/损坏三态）、`core/tests/{detection,environment}.rs`。
  验证：`cargo test --workspace --locked` → **106 passed / 0 failed**；
  `cargo clippy -p neohub-{domain,storage,core} --all-targets` → 0 warnings；`cargo fmt --all` 已执行。
- 第三批完成（2026-11-06T14:45Z–15:05Z UTC）：新增 `native/crates/platform-macos/`
  （`keychain.rs`：`security-framework 3.7` + `aes-gcm 0.10`；master key 存 login Keychain，
  单条 generic-password 项，密文为自包含 `nhk1:` base64(nonce‖AES-256-GCM)；解密**不会**创建新
  master key；`keychain_round_trip` 为 opt-in 测试，默认套件不碰钥匙串）。
  验证：`cargo test -p neohub-platform-macos -- --include-ignored` → **3 passed**
  （真实 Keychain 写入→解密→删除→再解密失败，且事后 `security find-generic-password` 返回
  errSecItemNotFound，无残留）；`cargo test --workspace --locked` → **108 passed / 0 failed**；
  `cargo clippy --workspace --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行。
- 与旧版不同之处（本阶段新增）：
  1. `engines.node` 的 npm 风格写法（空格连接的比较符）会被归一化；`||` OR 组**显式报错**
     （`InvalidRequirement`），不静默改变语义
  2. 密钥格式：新密文统一 `nhk1:` 前缀，`enc1:` 明确拒绝（错误而非把密文当密码），
     master key 缺失时解密报错而不是悄悄新建
  3. 版本扫描结果带缓存，可显式刷新；system Node 通过 PATH 直接执行 `node --version`（不经 shell）
- 未解决问题及所属阶段：
  1. resolver/env/secrets 尚未接入 spawn 流程 → 阶段 5
  2. "从 Finder 启动的原生包能发现预置测试 Node" 需要打包后的 `.app` → 阶段 15 验证
  3. Keychain 仅在当前用户会话验证过 round-trip；锁定/拒绝授权、签名包内 keychain-access-groups
     → 阶段 15/16
- 与旧版不同之处：环境构建改为纯函数（注入 `os_env`/`login_path`/`project_dir`/三层变量），
  便于在不启动进程的情况下断言；语义与旧版逐层一致（含控制键保护与空值保留）。
- 下一项具体动作：实现 `node_resolver.rs`（先用假 home 目录构造 nvm/fnm/volta/asdf 布局的 fixture，
  写 `.nvmrc`/`.node-version`/`engines.node` 失败测试，再实现扫描与选择），然后 `detect.rs`，
  最后 secrets/keychain + `tests/{environment,detection}.rs`。

---

### 阶段 5：PTY 运行、进程树停止与退出历史
- 状态：**已验证**（PTY runner、进程树终止、CoreHandle 闭环、运行历史与 Shutdown 顺序全部完成并测试）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: implement PTY task lifecycle and process tree shutdown`
- 已完成并验证（2026-11-06T15:10Z–15:50Z UTC）：
  - `native/test-fixtures/process/`：5 个合成 fixture（固定日志、延迟启动、派生两个孙进程、
    忽略 SIGTERM、指定退出码）。**不执行任何用户真实项目**
  - `crates/core/src/process/spawn_spec.rs`：`SpawnSpec`（program/args/cwd/env/初始 120×30/display）。
    script 任务 → `<pm> run <script>` 参数数组；只有显式 `custom` 才经 shell；工作目录覆盖相对 app 路径；
    缺失 script/command/包管理器/目录各自报错；grid 下限 2×2
  - `crates/core/src/process/process_tree.rs`：按**进程组**终止（TERM → grace → KILL），
    `ps -o pid=,pgid=` 一次批量枚举成员，拒绝 pid 0/1 与 NeoHub 自身进程组
  - `crates/core/src/process/runner.rs`：每任务一个操作锁串行化 start/stop；PTY 读取与 `wait`
    都在独立线程；日志按 5 MiB 有界环缓冲 + 事件；退出按 generation 只结算一次；
    shutdown 置位后拒绝新启动、立即回收"晚到"的子进程
- 验证命令与结果：
  - `cargo test -p neohub-core --test process_lifecycle --locked` → **13 passed / 0 failed**，含计划要求的
    四个竞态：`same_task_concurrent_start_spawns_once`、`stop_during_start_leaves_no_orphan`、
    `start_during_exit_waits_for_teardown`、`quit_during_spawn_reaps_late_child`；另有
    整棵进程树消失（父 + 两个孙进程）、忽略 TERM 后升级 KILL（实测耗时 ≥ grace）、
    崩溃记录退出码 5、正常退出、用户停止归因、resize 下限、3 任务 shutdown、自身组安全
  - 测试中对每个用例都断言 `group_members(pgid)` 为空，且运行时 `pgrep -f test-fixtures/process` 无残留
  - `cargo test --workspace --locked` → **132 passed / 0 failed**
  - `cargo clippy -p neohub-core --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行
- 与旧版不同之处：
  1. 停止作用于**进程组**而非递归 pid 列表（旧版 `tree-kill` 逐 pid），少一次系统调用竞态；
     portable-pty 的 `setsid` 已在测试中确认（子进程 pid == pgid，孙进程同组）
  2. 日志环按字节（5 MiB），不是按 JS 字符串长度
  3. 退出结算带 generation 校验，陈旧 waiter 无法改写新 run 的状态
- 未解决问题及所属阶段：
  1. `CoreCommand`/`CoreSnapshot`/`CoreEvent` 与 `CoreHandle` 的 actor 尚未落地 → 本阶段剩余
  2. 运行历史写入（start 记 row、退出 finish + prune）→ 与 service 层一起补
  3. channel 满时的背压/resync 策略（当前用 broadcast，慢订阅者收到 Lagged）→ 阶段 6 与日志一起收紧
  4. `Quit during spawn` 的 service 级顺序（拒绝启动 → 等在途 → 反向停止 → 写历史 → 关库）→ 本阶段剩余

- 第二批完成（2026-11-06T15:55Z–16:25Z UTC）：`commands.rs`/`events.rs`/`snapshot.rs`/`service.rs`
  - `CoreCommand`：GetSnapshot / StartTask / StopTask / ResizeTask / GetLogs / StopApp / Shutdown
  - `CoreService`：单一 DB 持有者 + `TaskRunner` + 事件泵；start 时解析 Node（`spawn_blocking`）、
    构建 env、解密检查、spawn、**spawn 成功后再写运行历史**（写失败只记日志，进程仍被回收）
  - 退出事件 → `finish_run`（ended_at/退出码/信号/是否用户停止）；慢订阅者超过 2048 事件收到
    `ResyncRequired`；`Shutdown` 顺序为：拒绝新启动 → 停全部 → 关闭残留 run 行 + prune →
    `wal_checkpoint` → 丢弃连接
  - 验证：`cargo test -p neohub-core --test core_service --locked` → **5 passed**
    （start→快照→日志→stop→历史行；崩溃退出码 7；shutdown 停止全部并拒绝新启动；
    订阅者按序看到 running→terminal；`enc1:` 密文阻止启动并报 SecretUnavailable）
  - `cargo test --workspace --locked` → **137 passed / 0 failed**；
    `cargo clippy -p neohub-core --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行
  - 残留进程检查：`pgrep -f test-fixtures/process` 为空

---

### 阶段 6：有界日志、终端状态与订阅恢复
- 状态：**已验证**（日志存储/终端状态/订阅引导/搜索全部完成；逐帧合批与 UI 渲染在阶段 11）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: add bounded logs and recoverable terminal streams`
- 实际修改文件：新增 `native/crates/core/src/logs/{mod,buffer,terminal_state,subscription,search}.rs`、
  `native/crates/core/tests/log_stream.rs`、`native/test-fixtures/process/numbered-output.sh`；
  修改 `process/runner.rs`（日志后端换成 `LogStore`，新增 bootstrap/search/clear/eviction/usage 接口）、
  `commands.rs`（`BootstrapLogs`/`SearchLogs`/`ClearLogs`）、`service.rs`（转发 + 全局搜索）、`Cargo.toml`
- 验证命令与结果（2026-11-06T16:40Z–17:20Z UTC）：
  - `cargo test -p neohub-core --test log_stream --locked` → **6 passed**
    （1000 条编号输出"先订阅后挂载"：无重复、无缺口；旧 run 游标返回 `ResetRequired`；
    丢弃订阅后 PTY 继续采集；退出任务仍可搜索；清空日志不影响运行中的进程；
    全局/单任务预算与截断上报）
  - `cargo test -p neohub-core --lib logs` → **27 passed**（终端状态 10：跨 chunk UTF-8/ANSI、
    宽字符、CR 覆写、清行/清屏、光标移动、超长行换行、scrollback 上限与截断；缓冲区 11；搜索 6）
  - `cargo test --workspace --locked` → **170 passed / 0 failed**
  - `cargo clippy -p neohub-core --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行
  - 残留进程检查：`pgrep -f test-fixtures/process` 为空
- 与旧版不同之处：
  1. ANSI 解析使用维护中的 `vte` 0.15（Alacritty/Zed 同款解析器），网格与 scrollback 自实现；
     **没有**用正则剥 ANSI。计划中的候选 `alacritty_terminal` 需要整棵 Term/Grid/Handler 栈，
     体积与 API 成本更高——此处选择 vte 并在依赖文档记录（阶段 16 复核）
  2. 预算按真实 UTF-8 字节计算（旧版按 JS 字符串长度近似），差异已按计划记入兼容说明
  3. 终端快照携带 `parsed_bytes`（解析位置），重开窗口只取快照，绝不重放截断的 ANSI 片段
  4. 每个事件 ≤10 KiB，序列号按 run 递增；run 变化时序号重置并向旧游标返回 `ResetRequired`
- 未解决问题及所属阶段：
  1. 界面刷新合批（约 33 ms / 16 KiB）与"仅可见任务推送"门控 → 阶段 11（需要 UI 订阅模型）
  2. 日志导出命令（原始文件 vs 可读文本的区别）→ 阶段 11
  3. 全局 100 MiB 预算在真实长跑任务上的高水位验证 → 阶段 16 压测

---

### 阶段 7：任务依赖、就绪探测、失败回滚
- 状态：**已验证**
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: port task orchestration and readiness`
- 实际修改文件：新增 `native/crates/core/src/orchestrator/{mod,topology,state,readiness}.rs`、
  `native/crates/core/tests/orchestration.rs`、`native/test-fixtures/process/{listens-on-port,http-ready}.sh`；
  修改 `service.rs`（`start_app`/`stop_app`/`restart_app` + 回滚 + 取消 + readiness 记录）、
  `commands.rs`（`StartApp`/`RestartApp`）、`domain/error.rs`（`ReadinessTimeout`）、`Cargo.toml`（reqwest/futures）
- 验证命令与结果（2026-11-06T17:30Z–18:15Z UTC）：
  - `cargo test -p neohub-core --test orchestration --locked` → **7 passed**
    （三层链路全部启动；readiness 失败只回滚本次启动、用户先前手动启动的任务保持运行；
    disabled 任务保持 idle；`none` 记为 unverified；restart 产生新 run；真实 port 探测；
    真实 http 探测穿过 503→200 预热）
  - `cargo test -p neohub-core --lib orchestrator` → **24 passed**
    （拓扑分层/环/自依赖/未知依赖/disabled/反向停止 9；app 状态与 sticky 6；readiness 单元 9）
  - `cargo test --workspace --locked` → **201 passed / 0 failed**
  - `cargo clippy -p neohub-core --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行
  - 残留进程检查：`pgrep -f test-fixtures/process` 为空
- 与旧版不同之处：
  1. `engines`/readiness 的 JS 正则差异被显式处理：只有 `i` flag 有等价物，其它 flags
     返回 `Failed("…not supported…")`；非法正则同样明确失败，不静默改成字面量匹配
  2. http readiness 的 TLS 跳过**仅限 loopback**（`danger_accept_invalid_certs` 只在
     127.0.0.1/localhost/::1 时开启），非 loopback 正常校验证书
  3. 回滚精确到"本次 start 新增的任务"集合，而不是按 app 全量停止
  4. `none` readiness 有独立结果 `Unverified`，快照里记 `(kind, verified=false)`
- 未解决问题及所属阶段：
  1. app `autoStart`（登录/启动时自动拉起）→ 阶段 12/15（需要 AppHost 生命周期）
  2. 每层 readiness 等待是串行等待同层全部完成（旧版亦然），更细的并发控制留待实测
  3. http 探测使用 reqwest+rustls；私有 CA 场景尚未验证 → 阶段 16

---

### 阶段 8：端口、资源、文件监听与通知事件
- 状态：**已验证**（monitor/watch 模块、后台采样循环与 stats/ports 事件全部完成）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: port process monitoring and file watchers`
- 实际修改文件：新增 `native/crates/core/src/monitor/{mod,ports,stats}.rs`、
  `native/crates/core/src/watch/{mod,environment,restart}.rs`、
  `native/crates/core/tests/monitor_watch.rs`、`native/test-fixtures/process/spawn-listener.sh`；
  修改 `Cargo.toml`（notify 8 / glob 0.3）
- 验证命令与结果（2026-11-06T18:30Z–19:15Z UTC）：
  - `cargo test -p neohub-core --test monitor_watch --locked` → **5 passed**
    （孙进程监听端口被归到任务；confirmed 与 hint 分离且 hint 三次淘汰；退出竞态采样为 0；
    `.env` 变化上报；源码变化上报且 node_modules/非匹配文件被过滤）
  - `cargo test -p neohub-core --lib monitor` → **11 passed**；`--lib watch` → **8 passed**
  - `cargo test --workspace --locked` → **225 passed / 0 failed**
  - `cargo clippy -p neohub-core --all-targets --locked` → **0 warnings**；`cargo fmt --all` 已执行
  - 残留进程检查：`pgrep -f test-fixtures/process` 为空
- 与旧版不同之处：
  1. 采样口径分离：`cpu_percent`/`mem_mb` 保留旧 `cpu`/`memMB` 语义（根进程），
     另给 `tree_cpu_percent`/`tree_mem_mb`/`process_count`（整棵受管树），不静默改含义
  2. 端口探测一次批量 `lsof`（旧版每任务一次），并按进程组归属判祖先链
  3. glob 语义：`glob` crate 的 `**/` 需要至少一层目录，已补上 picomatch 的"零层"折叠形式
  4. 目录级 FSEvents 事件（无法匹配文件 glob）视为"该目录有变化"，避免漏重启；生成目录仍被忽略
- 未解决问题及所属阶段：
  1. 后台采样循环与 stats/ports 事件 → **已完成**（见下）
  2. 崩溃/就绪通知、通知点击跳转 → 阶段 15
  3. `.env` 变化触发的 UI 横幅与"重启 watcher 复用启动锁"（避免重启风暴）→ 阶段 10/12

---

- 第二批完成（2026-11-06T19:20Z–19:45Z UTC）：`CoreEventKind::{TaskStats, TaskPorts}`、
  `CoreSnapshot::{task_stats, task_ports}`；`CoreService::monitor_tick()` + `spawn_monitor()`
  - 空闲时**不轮询**：无运行任务时循环停在 `Notify` 上（真实 await，不是 sleep 轮询），
    启动任务时被唤醒；`monitor_sweeps` 计数器可验证"空闲零采样"
  - 每次 sweep：一次 `ps` 读全表 → 每任务 own/tree 口径；一次 `lsof` 批量取 LISTEN →
    按进程组归属；日志提示只取尾部 4 KiB，避免每秒钟扫描 5 MiB 缓冲
  - 任务停止后清理该任务的采样与端口缓存；事件带 revision，快照与事件同源
  - 顺手修掉：`RunningTaskInfo.app_id` 之前被填成空串（runner 未保存 app），现在保留真实 app id
  - 验证：`cargo test -p neohub-core --test monitor_loop` → **3 passed**
    （stats/ports 事件到达订阅者且快照含同样缓存；空闲 700 ms 零采样、启动后开始采样、停止后停止；
    端口 confirmed 与 hinted 分别上报）；`cargo test --workspace --locked` → **228 passed / 0 failed**；
    clippy **0 warnings**；`cargo fmt --all`；`pgrep -f test-fixtures/process` 为空

### 阶段 9：原生主界面、侧栏、仪表盘、导入
- 状态：**已验证**（外壳/侧栏/仪表盘 + 扫描导入 + 空态；命令面板与右键菜单留给阶段 10）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: build native dashboard and project navigation`
- 实际修改文件：新增 `native/crates/ui/src/state/{mod,session,store}.rs`、
  `native/crates/ui/src/views/{mod,shell}.rs`、`native/crates/ui/tests/navigation.rs`；
  重写 `native/crates/desktop/src/main.rs`（真实宿主装配）；更新 `ui/Cargo.toml`、`desktop/Cargo.toml`、
  `ui/src/lib.rs`、`FEATURE-PARITY.md`、本文件
- 验证命令与结果（2026-11-06T19:50Z–20:40Z UTC）：
  - `cargo test -p neohub-ui --locked` → **17 passed**（session 6 + store 11）
  - `cargo test -p neohub-ui --test navigation --locked` → **4 passed**
    （选择在窗口销毁后恢复、被删除的 app 回退仪表盘、100 个项目下的分组/筛选/排序、
    仪表盘端口与 CPU/内存聚合口径）
  - `cargo test --workspace --locked` → **247 passed / 0 failed**
  - `cargo clippy -p neohub-ui -p neohub-desktop --all-targets --locked` → **0 warnings**；`cargo fmt --all`
  - 实机：`./target/debug/neohub-desktop --data-dir /tmp/neohub-ui-probe` 启动成功，
    GPUI 自报窗口 `1280×853 @ (160,103)`，`CGWindowList` 看到 `owner=neohub-desktop, layer=0, onscreen=true`
- 与旧版不同之处：
  1. 界面状态与渲染分离：`UiStore` 是纯投影（快照+事件 → 侧栏/卡片/汇总），无 GPUI、无 IO，
     因此导航与聚合逻辑可以在没有窗口的情况下测试
  2. 导航会话写在数据目录的 `session.json`（原子写：临时文件 + rename），不是 Chromium localStorage
  3. 渲染线程不做阻塞调用：命令通过 `tokio` 运行时下发，事件泵每 50 ms 取一次并 `notify()`
- 未解决问题及所属阶段：
  1. **窗口尺寸观测不一致**：GPUI 自报 1280×853，`CGWindowList` 报告 150×129（极简视图同样如此，
     说明与本视图内容无关，是本会话窗口服务器的观测限制）。像素级验证仍需用户桌面会话或阶段 16
  2. 添加项目 / 扫描导入 / Node 选择 / 空态 / 错误提示 / 加载态 / 命令面板 ⌘K /
     右键菜单 / 虚拟列表 / 快捷键 → 本阶段剩余
  3. "导入合成项目 → 启动 → 卡片显示运行 → 端口 → 停止" 的端到端人工闭环依赖上面的导入界面；
     core 侧链路已由阶段 5–8 的测试覆盖
- 下一项具体动作：补 `views/{sidebar,dashboard,add_app,import_projects}.rs` 的交互（添加入口、
  扫描导入列表、空态、命令面板），再完成端到端人工闭环。

---

- 第二批完成（2026-11-06T20:45Z–21:20Z UTC）：
  - core：`environment::scan_folder`（浅层扫描，只列直接子目录含 `package.json`，跳过
    node_modules/隐藏目录，200 上限）、`CoreCommand::{ScanFolder, CreateApp}`、
    `CoreReply::{ImportCandidates, App}`，`ImportCandidate.already_registered` 由 service 填充
  - ui：`UiStore` 增加 `ImportState`（open/dir/busy/error/candidates + `addable()` 过滤已注册）；
    `HostBridge` 把命令回复并入同一通道（`HostUpdate`），扫描/导入结果回到界面；
    视图加入顶栏「+ 导入项目」、导入面板（候选列表、已导入灰显、错误条、扫描中状态）、
    首次运行空态（欢迎 + 扫描入口），导入成功后自动刷新快照
  - 验证：`cargo test -p neohub-core --test import_flow` → **2 passed**
    （scan → create → rescan 显示已导入 → StartApp → 快照 app_state=running →
    `TaskPorts` 事件含确认端口 → StopApp → 非 live；重复导入被拒绝）
    `cargo test -p neohub-ui` → **19 passed**；`cargo test --workspace --locked` → **253 passed / 0 failed**；
    `cargo clippy -p {core,ui,desktop} --all-targets` → **0 warnings**；`cargo fmt --all`
  - 实机：`NEOHUB_IMPORT_DIR=/tmp/neohub-synth-repos ./neohub-desktop --data-dir /tmp/neohub-ui-probe2`
    启动 8 秒无 stderr、数据目录正确创建 `devharbor.db(-wal/-shm)`
  - 与旧版不同之处：导入目录由 `NEOHUB_IMPORT_DIR`（缺省 `~/Code`）决定，扫描结果带
    `already_registered` 标记；旧的"选择文件夹"对话框尚未接入（GPUI 文件对话框 API 待验证）
  - 未解决问题：
    1. 文件选择对话框（`prompt_for_paths`）与命令面板 ⌘K / 右键菜单 / 虚拟列表 → 阶段 10 一并处理
    2. 完整人工点击闭环（导入→启动→卡片运行→点端口→停止）需要用户桌面会话操作；
       上述 `import_flow` 测试从命令层验证了同一链路

### 阶段 10：项目详情、任务编辑、环境变量与设置
- 状态：**开发中**（详情页、任务编辑器、设置面板、环境变量编辑器（模型+命令+视图）完成；行内编辑与 db danger zone 待做）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: add edit drafts, save barrier and locales`
- 实际修改文件：新增 `native/crates/ui/src/state/edit_session.rs`、
  `native/crates/ui/src/i18n.rs`、`native/crates/ui/locales/{en,zh-CN}.json`、
  `native/crates/ui/tests/editor_save.rs`；更新 `ui/Cargo.toml`（dev-deps）、`state/mod.rs`、`lib.rs`
- 验证命令与结果（2026-11-06T21:25Z–21:55Z UTC）：
  - `cargo test -p neohub-ui --test editor_save --locked` → **4 passed**，即计划点名的四个：
    `close_flushes_pending_edits`、`save_failure_keeps_window_and_input`、
    `mcp_refresh_preserves_active_edit`、`stale_save_does_not_overwrite_newer_revision`
    （最后两个用真实 core/SQLite：MCP 侧改名后本地草稿保留并发起冲突；陈旧 revision 的 ack
    不会清掉更新的编辑，后续 flush 发送新值）
  - `cargo test -p neohub-ui --lib edit_session` → **7 passed**；`--lib i18n` → **5 passed**
  - `cargo test --workspace --locked` → **269 passed / 0 failed**
  - `cargo clippy -p neohub-ui --all-targets --locked` → **0 warnings**；`cargo fmt --all`
- 与旧版不同之处：
  1. 修订号语义明确：`Draft::ack(saved, revision)` 只在 revision 等于当前时才清 dirty；
     否则服务端值成为新 baseline、本地保持 dirty，**不会回退 revision**（初版回退了，被测试抓出）
  2. 冲突值单独保存（`conflict`），用户可显式 `take_conflict()` 采用对方版本，而不是弹窗覆盖
  3. 语言包内嵌（`include_str!`）+ 缺 key 回退链，并由测试断言两语言 key 完全对齐
- 未解决问题及所属阶段：
  1. 详情页/任务编辑器/环境变量编辑器/设置面板的 GPUI 表单视图 → 本阶段剩余
     （文本输入需要 `gpui-component` 的 Input，尚未验证其 API）
  2. 设置面板要覆盖全部 17 项 + db 导出/重置/MCP/登录项/通知/菜单栏 → 依赖阶段 15 的系统集成
  3. 运行历史视图与崩溃日志入口 → 与详情页一起做
- 下一项具体动作：先验证 `gpui_component::input` 的输入控件 API（编译一个最小表单），
  再做 `views/{app_detail,task_editor,settings}.rs`，把 `Draft`/`SaveBarrier` 接进视图。

---

- 第二批完成（2026-11-06T22:00Z–22:25Z UTC）：
  - `ui/state/text_input.rs`：单行文本字段模型（插入/退格/删除/左右/Home/End/Esc/Enter，
    UTF-8 边界安全，控制字符拒绝），6 个单测
  - 侧栏接入真实筛选输入框（点击聚焦、键入过滤、Esc 清空），`render` 用字段值驱动 `UiStore::set_filter`
  - **组件库输入控件的尝试与结论**：`gpui_component::input` 的 `InputState`/`Input` 需要其内部
    状态实体与焦点管线（`InputState::new()` 签名、`Input::new` 参数与文档不符），两轮编译都未对上；
    为避免把时间花在猜测 UI 库 API 上，改用自绘字段 + `KeyDownEvent`，逻辑因此可单测。
    这一点已记入下方未解决问题，阶段 11/后续再评估是否切回组件库（需要读其 `input/state.rs` 源码）
  - 验证：`cargo test --workspace --locked` → **275 passed / 0 failed**；clippy **0 warnings**；
    `cargo fmt --all`；实机启动 7 s 无 stderr（带筛选框的界面不崩）
  - 未解决问题：
    1. 详情页/任务编辑器/设置面板视图仍未做（下一步），依赖文本字段（已具备）与多行日志区
    2. 组件库输入控件若要用，需要按其源码写一个最小可编译示例并固化 API 版本

- 第三批完成（2026-11-06T22:30Z–23:05Z UTC）：
  - core 命令面补齐：`AddTask`/`UpdateTask`/`RemoveTask`/`ReorderTasks`/`ListRunHistory`，
    以及**补回阶段 6 遗漏的** `BootstrapLogs`/`SearchLogs`/`ClearLogs`（见下"记录更正"）
  - `LogStore::ensure_run`：任务启动即登记日志条目，未输出时可 bootstrap（原来会报 NotFound）
  - ui：`UiStore` 增加运行历史缓存；`HostBridge::load_history`；**详情页**渲染任务 tabs、
    每任务启停、项目级启停/重启、日志尾 20 行、最近 5 条运行历史（含退出码与"用户停止"归因）
  - 验证：`cargo test -p neohub-core --test app_detail` → **4 passed**
    （每任务启停与项目级启停/重启一致；命令层编辑校验并定位字段；重排/删除生效；
    停止后历史行闭合且标记用户停止；重开详情可 bootstrap 日志）
    `cargo test --workspace --locked` → **279 passed / 0 failed**；clippy **0 warnings**；实机启动无 stderr
- **记录更正**：阶段 6 的进度条目曾写明已加 `BootstrapLogs`/`SearchLogs`/`ClearLogs` 命令，
  实际代码里只有 runner/service 的底层方法，命令枚举与分发缺失（本轮测试暴露）。现已补齐，
  阶段 6 的"已验证"结论只对底层能力成立，命令面在阶段 10 才接上。
- 未解决问题：
  1. 任务编辑器视图（表单：命令/工作目录/依赖/readiness/oneShot/enabled）与设置面板 → 本阶段剩余
  2. Open In、崩溃日志钉住、`.env` 变化横幅 → 阶段 12/15

- 第四批完成（2026-11-06T23:10Z–23:35Z UTC）：
  - `ui/state/task_form.rs`：`TaskForm`（名称/命令/工作目录/readiness 参数 + 六种 readiness /
    oneShot / enabled），Tab 循环焦点，`to_update_input()` 产出 `UpdateTaskInput`，
    本地校验与**后端错误映射到具体字段**（`FieldError`），`apply_external` 在脏状态下保留本地文本
  - oneShot 勾选自动切换为 exit readiness（避免保存时必然失败）
  - `HostBridge::save_task` + `HostUpdate::{TaskSaved,TaskError}` + `UiStore::upsert_task`，
    保存结果回到界面并刷新任务 tab
  - 测试：`cargo test -p neohub-ui --lib task_form` → **9 passed**
    （输入→脏→载荷；Tab 循环；空名/端口 0 定位字段；oneShot→exit；六种 readiness 往返；
    后端错误归因；脏状态外部刷新保留；干净状态跟随刷新；ack 清脏）
  - `cargo test --workspace --locked` → **288 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
  - 未解决问题：编辑面板的**视图渲染**（点击"编辑任务"展开表单、字段聚焦/键入、保存按钮）
    仍是本阶段剩余；表单模型与保存通道已就绪，接线不需要新的核心能力

- 第五批完成（2026-11-06T23:40Z–23:55Z UTC）：
  - 编辑面板渲染进详情页：「编辑任务」按钮创建 `TaskForm` 并聚焦；四个字段行可点击聚焦，
    当前焦点字段显示光标；readiness 六个选项可点选；保存/取消按钮；错误显示为
    "字段名：消息"；Esc 关闭面板；Tab 切换字段
  - 保存路径：`to_update_input()` → `HostBridge::save_task` → `HostBridge::take_save_outcome`
    在下一帧回收结果：成功则 `ack` 并关闭面板、任务 tab 显示新名称；失败则把错误映射回字段
  - 验证：编译/clippy **0 warnings**；`cargo test --workspace --locked` → **288 passed / 0 failed**；
    实机启动 7 s 无 stderr（带编辑面板的界面不崩）
  - 未解决问题：
    1. 编辑面板的字段键入目前依赖一次性 focus（点击字段设置焦点）；实机上需要人工确认
       IME/退格/光标移动，本会话无法做点击验证
    2. 设置面板（17 项 + db 导出/重置/登录项/通知/菜单栏）与环境变量编辑器仍是本阶段剩余
    3. 任务依赖编辑与重排 UI 尚未提供（命令层已具备）

- 第六批完成（2026-11-07T00:00Z–00:25Z UTC）：
  - core：`GetSettings`/`UpdateSettings` 命令；`CoreEventKind::SettingsChanged`；
    `CoreService::{settings, update_settings}`（应用 patch → 落库 → 广播）
  - ui：`SettingsForm`（17 项全覆盖）：`patch()` 只携带改动字段；数值项经 domain patch 的同一套
    clamp（刷新下限 200 ms、就绪超时下限 1000 ms、MCP 端口 <1024 直接拒绝且不改动草稿）；
    脏状态下忽略外部刷新；`pending_native_fields()` 明确列出"已存储但需阶段 12/14/15 才生效"的项
    （登录启动 / 菜单栏 / 通知 / MCP），不假装它们已经工作
  - 验证：`cargo test -p neohub-ui --lib settings_form` → **8 passed**；
    `cargo test -p neohub-core --test settings_cmd` → **3 passed**
    （命令往返 + 落库 + 重启后仍生效 + `SettingsChanged` 事件；越界值由 core 兜底 clamp；
    未知字段 `serde` 解析即报错）；`cargo test --workspace --locked` → **299 passed / 0 failed**；
    clippy **0 warnings**
  - 未解决问题：
    1. 设置面板的**视图**尚未渲染（模型与命令已就绪）
    2. 依赖系统集成的四项（登录启动/菜单栏/通知/MCP）只存储与展示，实际生效在阶段 12/14/15
    3. 环境变量编辑器（三层 + 秘密遮挡 + 掩码不回写）仍待做

- 第七批完成（2026-11-07T00:30Z–00:50Z UTC）：
  - 设置面板渲染进窗口：侧栏「设置」开关；主题（深色/浅色/跟随系统）与语言（系统/中文/English）
    三选一；8 个开关（自动更新/崩溃通知/就绪通知/登录启动/菜单栏/MCP 启用/要求 Token/HTTPS）；
    6 个数值项用 −/+ 步进（范围由 domain clamp 兜底，不需要自由输入，避免非法中间状态）；
    「待接入」项以警示条单列；保存后回收结果并 `ack` 清脏
  - `HostBridge::save_settings` + `HostUpdate::{SettingsSaved,SettingsError}` + `UiStore::{set_settings,settings}`
  - 验证：`cargo test --workspace --locked` → **299 passed / 0 failed**；clippy **0 warnings**；
    `cargo fmt --all`；实机启动 7 s 无 stderr（含设置面板的界面不崩）
  - 未解决问题：
    1. 设置面板的点击路径（开关/步进/保存）在实机上仍需人工确认一次（本会话无辅助功能权限）
    2. db danger zone（导出/重置）需要平台对话框与 WAL 安全导出 → 阶段 15
    3. 环境变量编辑器（Global/App/Task 三层 + 秘密遮挡 + 掩码不回写）仍待做

- 第八批完成（2026-11-07T00:55Z–01:20Z UTC）：
  - core：`ListEnvVars`/`SetEnvVars` 命令 + `EnvScope`/`EnvVarView`/`EnvVarInput`
    - 读取时**秘密只回报"已设置"**（`value: None`），核心从不把秘密文本发给视图
    - 写入时 `value: None` 表示"保留原值"，因此 UI 不可能把掩码字符当成密码写回
    - 秘密写入经 `SecretStore.encrypt`（`nhk1:` 前缀），数据库里没有明文
  - ui：`EnvEditor`（三层作用域、掩码/明文/新秘密三态、启用开关、备注、粘贴 `.env` 合并、
    `effective()` 覆盖来源预览、`scope_for(tab)`）
  - 验证：`cargo test -p neohub-ui --lib env_editor` → **9 passed**（含掩码不回写回归测试）；
    `cargo test -p neohub-core --test env_cmd` → **3 passed**
    （库内为 `nhk1:` 密文且不含明文；掩码重存不改动密文；三层各自独立）；
    `cargo test --workspace --locked` → **311 passed / 0 failed**；clippy **0 warnings**
  - 未解决问题：
    1. 环境变量编辑器的**视图**尚未渲染（模型与命令已就绪）
    2. 已存储的旧 `enc1:` 密文仍会在启动时阻止任务（阶段 13 迁移）
    3. db danger zone（导出/重置）→ 阶段 15

- 第九批完成（2026-11-07T01:25Z–01:45Z UTC）：
  - 环境变量面板渲染进详情页：三层 tab（全局/应用/任务）切换即加载对应作用域；
    行显示 key、值或 `••••••` 掩码、启用状态；每行三个小按钮（启用/停用、明文/秘密、删除）；
    「新增变量」「应用粘贴」（把 `KEY=VALUE …` 粘进列表，掩码/秘密语义保持）、保存、关闭
  - `HostBridge::{load_env_vars, save_env_vars}` + `HostUpdate::{EnvVars, EnvError}`，
    结果按作用域回填到打开的编辑器（`EnvEditor::ack`）
  - 验证：`cargo test --workspace --locked` → **311 passed / 0 failed**；clippy **0 warnings**；
    `cargo fmt --all`；实机启动 7 s 无 stderr
  - 未解决问题：
    1. **行内值/key 编辑**尚未接（点击单元格进入编辑需要一个额外的焦点状态机；
       当前只能通过「应用粘贴」或新增空行再粘贴来改值）
    2. 覆盖来源预览 `effective()` 已在模型里可用，但面板上还没渲染「哪一层生效」列表
    3. db danger zone（导出/重置）→ 阶段 15

### 阶段 11：原生终端、日志搜索与导出
- 状态：**已验证**（终端视图/选择/搜索、彩色渲染、导出、全局日志搜索面板全部完成；等宽字体与刷新合批为已知差异）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: add the native terminal view, selection and search`
- 实际修改文件：新增 `native/crates/ui/src/terminal/{mod,view,selection,search}.rs`、
  `native/crates/ui/tests/terminal.rs`；更新 `ui/Cargo.toml`（unicode-width）、`ui/src/lib.rs`
- 验证命令与结果（2026-11-07T01:50Z–02:15Z UTC）：
  - `cargo test -p neohub-ui --lib terminal` → **17 passed**（视图 6：跟随/滚动/scrollback/裁剪/空/导出；
    选择 6：单行/双向/宽字符/emoji/越界/空；搜索 5：字面量/上下环绕/正则与非法/无命中/limit）
  - `cargo test -p neohub-ui --test terminal` → **6 passed**
    （两个不同速率任务输出互不混入；重建视图不重复历史；退出任务仍可搜索；
    UTF-8 跨 chunk 的中文/emoji 从 PTY 字节到显示行保持一致；滚动后恢复跟随；resize 不丢内容）
  - `cargo test --workspace --locked` → **334 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
- 与旧版不同之处：
  1. 终端渲染读的是核心解析后的网格 + scrollback，不是 xterm.js；选择按**显示格**定位并按字符边界
     切分，宽字符/emoji 不会被切成半个
  2. 搜索复用核心的匹配器，所以终端内搜索与全局日志搜索的语义一致（非法正则同样报错）
  3. 滚动是视图状态：向上滚暂停跟随、回到底部自动恢复；导出用 `TerminalView::text()`
- 未解决问题及所属阶段：
  1. 详情页日志区仍显示原始文本（阶段 9 的实现），尚未改为渲染 `TerminalView`（含 ANSI 颜色/字号）
  2. 日志导出命令（写文件 + 与"可读文本"的区别说明）→ 本阶段剩余
  3. 专用日志搜索视图（`views/log_search.rs`，全局跨任务定位）→ 本阶段剩余
  4. 人工截图对比旧版 xterm 与本实现 → 需要用户桌面会话

---

- 第二批完成（2026-11-07T02:20Z–02:45Z UTC）：
  - core：`GetTerminalText`（已解析文本，含 scrollback）与 `ExportLogs { path, raw }`
    （`raw=true` 保留 ANSI 原始字节，`raw=false` 写可读文本；写文件在 `spawn_blocking` 里）
  - ui：详情页日志区改为渲染 `TerminalView`（默认 16 行可视区）；工具条含「跟随」「↑」「↓」
    「导出可读」「导出原始」；选中任务变化或行数变化时重建视图并请求文本
  - 验证：`cargo test -p neohub-core --test log_export` → **2 passed**
    （可读文本无转义且 CR 覆写只保留最后一帧；原始导出含 ANSI；未知任务导出为空文件而非报错）
    `cargo test --workspace --locked` → **336 passed / 0 failed**；clippy **0 warnings**；
    实机启动 7 s 无 stderr
  - 未解决问题：
    1. 终端渲染目前是纯文本行（未按 ANSI 上色、未用等宽字体）；颜色需要把 SGR 状态带出解析器
    2. 日志区刷新依赖"行数变化"触发，尚未做 33 ms/16 KiB 合批与可见性门控（阶段 9 记录的剩余项）
    3. 全局日志搜索视图（跨任务定位 + 点击跳转）仍待做

- 第三批完成（2026-11-07T02:50Z–03:15Z UTC）：
  - core：`CellStyle`/`AnsiColor`/`TextRun`；`TerminalState` 逐格记录样式，SGR 解析支持
    重置(0)、粗体(1/22)、默认前景(39)、经典 8 色(30-37)、亮色(90-97)、`38;5;n`(256 色)、
    `38;2;r;g;b`(真彩)；擦除单元格同时重置样式；`TerminalSnapshot.styled` 输出同样式 run
  - ui：`terminal/color.rs`（16 色表 + 216 色立方 + 24 级灰阶 + 真彩直传）、`DisplayRow.spans`，
    详情页日志区按 run 上色渲染（默认色回落到主题 `fg_muted`）
  - 验证：`cargo test -p neohub-core --lib terminal_state` → **14 passed**（含颜色 3 例：
    颜色 run 划分与文本不受影响、256/真彩解析、擦行重置样式）；
    `cargo test --workspace --locked` → **343 passed / 0 failed**；clippy **0 warnings**；
    实机启动 7 s 无 stderr
  - 未解决问题：
    1. 等宽字体尚未指定（GPUI 字体族设置待验证）
    2. 全局日志搜索视图（跨任务定位 + 点击跳转）仍待做
    3. 33 ms/16 KiB 合批与可见性门控（阶段 9 起的遗留项）

- 第四批完成（2026-11-07T03:20Z–03:45Z UTC）：
  - `UiStore::SearchState`（open/query/busy/error/results）+ `locate_task`/`labels_for`
  - `HostBridge::search_logs`（全局 `SearchLogs { task_id: None }`）→ `HostUpdate::LogMatches`
  - 顶栏「搜索日志」打开面板：输入框（回车即搜、Esc 关闭）、结果按
    `应用 / 任务 · 第 N 行` + 命中行显示，点击结果**跳转到该项目与任务 tab**并关闭面板
  - 验证：`cargo test --workspace --locked` → **343 passed / 0 failed**；clippy **0 warnings**；
    `cargo fmt --all`；实机启动 7 s 无 stderr
  - 未解决问题（已知差异，非阻塞）：
    1. 等宽字体尚未指定
    2. 日志刷新合批（33 ms / 16 KiB）与"仅可见任务推送"门控仍未做
    3. 快捷键 ⌘⇧F/⌘K 的键位绑定待阶段 12 与菜单一起接

### 阶段 12：窗口销毁、菜单栏、Dock、单实例
- 状态：**开发中**（生命周期、单实例、窗口重建、bounds 持久化、Dock 桥接、托盘菜单模型完成；NSStatusItem 绑定待做）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: add the window lifecycle and single-instance lock`
- 实际修改文件：新增 `native/crates/desktop/src/{lib,session_lock,window_session}.rs`、
  `native/crates/desktop/tests/window_lifecycle.rs`；更新 `desktop/Cargo.toml`（lib target、libc、tempfile）
- 验证命令与结果（2026-11-07T03:50Z–04:20Z UTC）：
  - `cargo test -p neohub-desktop --test window_lifecycle` → **8 passed**
    （在途启动时关窗任务继续运行；保存失败保留窗口；无窗口时日志继续增长；
    20 次关闭/重开窗口代数 = 21 且没有任务被启动；quit 停止全部并拒绝新启动；
    tray 开关决定 Dock 隐藏；关窗后快照仍报告运行任务；重开恢复选择且不跑 autoStart）
  - `cargo test -p neohub-desktop --lib session_lock` → **3 passed**
    （首个实例拿到锁、第二个拿到 AlreadyRunning 并写激活文件、drop 后锁释放）
  - `cargo test --workspace --locked` → **354 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
- 与旧版不同之处：
  1. 单实例锁用 `flock`（进程被杀也会释放），不再依赖陈旧 pid 文件；第二实例通过
     `<data>/neohub.activate` 的 mtime 通知主实例
  2. 关窗语义显式建模为 `CloseDecision::{HideWindow, FlushFirst, KeepWindow(errors)}`，
     保存失败的分支在状态机里而不是散落在 UI 回调中
  3. `reopen()` 明确返回 `run_auto_start: false`，从类型上杜绝"重开窗口又启动一遍项目"
- 未解决问题及所属阶段：
  1. GPUI 真实窗口的销毁/重建（`open_window` 第二次调用、关闭时销毁实体）与 Cmd+W 绑定 → 本阶段剩余
  2. 托盘（AppKit/`tray-icon`）与 Dock 隐藏的实机调用、逐项目启停菜单 → 本阶段剩余
  3. 在打包 `.app` 上实测 Dock/Finder 激活/最后窗口关闭后事件循环不退出 → 阶段 15 打包后
  4. 第二实例把激活信号送达主实例（当前只写文件，主实例轮询读取的逻辑待接）

---

- 第二批完成（2026-11-07T04:25Z–04:50Z UTC）：
  - `desktop/src/app_host.rs`：`open_main_window` / `reopen_or_focus` / `ActivationWatcher`
    - 关窗拦截用 GPUI 的 `window.on_window_should_close`：返回 false 时窗口保持（保存失败分支），
      否则捕获 bounds → `AppHost::finish_close` → 允许销毁
    - 窗口可**再次** `open_window` 创建（同一进程、同一核心），`reopen_or_focus` 在窗口已存在时只激活
    - `ActivationWatcher` 轮询激活文件（每 500 ms，一次 stat，无 shell、无数据库访问）
  - `main.rs`：单实例锁 → tokio runtime → CoreService → AppHost/SharedHost → 打开窗口 → 轮询激活
  - 实机验证：先启动一个实例，再启动第二个 → 第二实例**立即退出**并打印激活文件路径；
    `/tmp/.../neohub.activate` 被创建；第一个实例仍在运行
  - `cargo test --workspace --locked` → **354 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
  - 未解决问题：
    1. 托盘（NSStatusItem/tray-icon）与 Dock 隐藏/恢复的实机调用、逐项目启停菜单 → 本阶段剩余
    2. Cmd+W 的键位绑定（当前只有窗口关闭按钮路径）
    3. 把 `window-state.json` 落盘（现在 bounds 只在进程内捕获）
    4. 打包 `.app` 上验证 Dock/Finder 激活与"最后窗口关闭后事件循环不退出" → 阶段 15

- 第三批完成（2026-11-07T04:55Z–05:20Z UTC）：
  - `platform-macos/src/dock.rs`：`set_dock_visible` / `current_dock_visible` / `sync_dock`，
    通过 `NSApplication.setActivationPolicy(Regular|Accessory)` 实现；**强制主线程**（非主线程
    直接拒绝并回报当前状态，不静默失败）；返回值是**读回**的真实策略，供调用方验证
  - `platform-macos/src/tray.rs`：`build_menu(&CoreSnapshot) -> TrayMenu`
    - 状态栏标题为运行中任务数（空闲为空串）
    - 逐项目一行：运行中显示「停止 X」、否则「启动 X」，**按名称排序**
    - 「全部停止」仅在确有任务运行时启用；另有「打开 NeoHub」「退出 NeoHub」
    - 全部从快照生成 —— **不要求窗口存在**（这正是托盘的意义）
  - 验证：`cargo test -p neohub-platform-macos --lib tray` → **5 passed**
    （标题计数、逐项目动词与排序、Stop all 启用条件、空注册表仍给 Open/Quit、无窗口可用）
    `cargo test --workspace --locked` → **359 passed / 0 failed**；clippy **0 warnings**；实机启动无 stderr
  - 未解决问题（本阶段剩余）：
    1. **NSStatusItem 绑定**：需要 `declare_class!` 定义 target-action 对象并在 GPUI 主线程创建；
       尚未实现（风险与成本都高，且必须实机点击验证）
    2. Dock 隐藏/恢复尚未接到 `AppHost.dock_hidden` 的变化上（函数已就绪，调用点待接）
    3. Cmd+W 绑定、`window-state.json` 落盘
    4. 打包 `.app` 上验证 Finder/Dock 激活与事件循环不退出的行为 → 阶段 15

- 第四批完成（2026-11-07T05:25Z–05:45Z UTC）：
  - `AppHost` 增加 `window-state.json` 持久化：关窗时写入 `{x,y,width,height}`，
    启动时读取；损坏文件视为"无存储 frame"→ 用默认尺寸（不 panic、不猜）
  - 验证：`cargo test -p neohub-desktop --test window_lifecycle` → **10 passed**
    （新增两条：frame 在下次启动被恢复；损坏的 window-state.json 回退默认）；
    `cargo test --workspace --locked` → **361 passed / 0 failed**；clippy **0 warnings**
  - 未解决问题（本阶段剩余，逐项明确）：
    1. **NSStatusItem 绑定**：需要在 GPUI 主线程用 objc2 `declare_class!` 定义 target-action
       对象并创建 `NSStatusItem`/`NSMenu`。本轮未实现——该绑定必须实机点击验证，
       而本会话无法进行 GUI 点击；已在功能表标注为"托盘 UI 未完成"
    2. Dock 桥接函数已就绪，但尚未挂到 `AppHost.dock_hidden` 的变化点（调用点待接）
    3. Cmd+W 键位绑定
    4. 打包 `.app` 的 Finder/Dock 激活与事件循环验证 → 阶段 15

### 阶段 13：旧版真实数据与密钥迁移
- 状态：**开发中**（只读巡检、导入、原子发布、幂等/不覆盖已验证；UI 迁移向导与 `enc1:` 重录流程待做）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: import legacy databases safely`
- 实际修改文件：新增 `native/crates/storage/src/import/{mod,legacy_db,manifest,legacy_import}.rs`、
  `native/crates/storage/tests/migration_atomicity.rs`、`native/docs/MIGRATION.md`；
  更新 `storage/Cargo.toml`（sha2）、`storage/src/{lib,db}.rs`
- 验证命令与结果（2026-11-07T05:50Z–06:25Z UTC）：
  - `cargo test -p neohub-storage --lib import` → **7 passed**
    （巡检报告版本/行数/未知设置；未知未来迁移版本被记录；WAL 一起复制且源不变；
    目录发现优先级；清单往返与重复来源识别；清单损坏不致致命）
  - `cargo test -p neohub-storage --test migration_atomicity` → **6 passed**
    （首次导入发布整库并暂停自动化；同源重复导入零新增且记为 repeat；**导入后原生改名不被重复导入覆盖**；
    损坏 JSON 列降级为空而非崩溃；发布失败不破坏既有文件；源库导入前后字节不变）
  - `cargo test --workspace --locked` → **374 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
- 与旧版不同之处：
  1. 导入是"巡检 → 预览 → 导入"三步，预览只含**数量与名称**，不含项目路径/密钥
  2. 首次导入在 `.import-staging/` 建库，`integrity_check` + `foreign_key_check` 通过后
     以**文件级 rename** 发布（目录级 rename 在目标目录已存在时会失败，已修正）
  3. 已存在 id 一律跳过；设置项用 `INSERT OR REPLACE`（单值，旧版意图应生效）
  4. 导入即强制 `auto_start=0`、`mcp_enabled=0`、`launch_at_login=0`，并在报告中标记
- 未解决问题及所属阶段：
  1. **未对真实用户数据执行导入**：计划要求启用前停止旧版、检查端口/锁占用；这属于需要用户
     明确同意的动作，本次只交付工具与步骤（`native/docs/MIGRATION.md`）
  2. `enc1:` 密文目前只能"原样保留 + 计数"，重录/旧版 opt-in 导出入口待做
  3. UI 迁移向导（`ui/src/views/migration.rs`）待做
  4. 迁移后 `migration.json` 清单尚未接入 UI 展示

### 阶段 14：MCP 全量迁移与协议兼容
- 状态：**基本完成**（22 工具的旧版参数与返回形状全部对齐，含 `workspaceTasks` monorepo；TLS/SSE 与错误文案为已知差异）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: port the MCP gate, redaction and tool catalogue`
- 实际修改文件：新增 `native/crates/mcp/{Cargo.toml,src/{lib,auth,redaction,tools}.rs}`；更新 workspace 成员
- 验证命令与结果（2026-11-07T06:30Z–07:00Z UTC）：
  - `cargo test -p neohub-mcp` → **17 passed**
    - `auth`（6）：Host 只接受精确 loopback（`localhost.evil.com` 被拒）、Origin 规则
      （缺失通过 / `null` 拒绝 / 非 loopback 拒绝）、Bearer 精确匹配、**Token 轮换后旧 Token 立即失效**、
      Host 去端口保留 IPv6 括号
    - `redaction`（6）：已知 secret 字面量、`enc1:`/`nhk1:` 密文、`Bearer` 头、JSON 递归、
      长 secret 优先（避免残留片段）、过短值不登记
    - `tools`（5）：22 个工具各一条 spec、wire 名与旧版一致、全部是 object schema、
      关键工具的 required 字段、名字可反查
  - `cargo test --workspace --locked` → **391 passed / 0 failed**；clippy **0 warnings**；`cargo fmt --all`
- 与旧版不同之处：
  1. **Host 校验更严格**：旧版 `hostWithoutPort` 把 `[::1].evil.com` 归约成 `[::1]` 从而放行；
     现在方括号 host 必须在 `]` 结束（后面只允许 `:port`）
  2. Bearer 比较同样是"先 sha256 再常量时间比较"，但用 Rust 实现（无 `timingSafeEqual` 的长度陷阱）
  3. 工具 schema 用 `additionalProperties: false` 明确拒绝多余参数（旧版 zod 也是严格模式）
- 未解决问题及所属阶段：
  1. **rmcp Streamable HTTP 服务未接入**（默认端口 6872、Bearer Token、可选自签 TLS）→ 本阶段剩余
  2. **22 个工具的 dispatch 未实现**：需要先在 core 补 `UpdateApp`/`RemoveApp`/`ListNodeVersions`
     命令，再逐个把工具接到 `CoreHandle`
  3. 兼容性 golden cases（对照旧版返回 JSON）→ 随 dispatch 一起做
  4. 真实 MCP 客户端验收（initialize → tools/list → tools/call）→ 随传输层一起做

- 第二批完成（2026-11-07T07:05Z–07:40Z UTC）：
  - core 补齐 `UpdateApp` / `RemoveApp` / `ListNodeVersions` 命令
    （`RemoveApp` 在任一任务运行时拒绝，避免工具调用把运行中的栈变成孤儿）
  - `mcp/src/dispatch.rs`：22 个工具全部接到 `CoreHandle`，与 UI 走**同一条命令路径**；
    参数从不可信 JSON 严格读取（必填/枚举/类型），错误返回 `{field, message}` 便于客户端定位
  - 关键语义：`list_env_vars` 对秘密只回 `value: null`；`set_env_var` 读-改-写保证掩码行不被覆盖；
    `get_ports` 区分 `confirmed`/`hinted`；`set_readiness` 不改命令即可更新探测
  - 验证：`cargo test -p neohub-mcp --test compatibility` → **5 passed**
    （add→list→get；秘密不回传文本；日志/端口来自运行中的任务且运行中拒绝 remove_app；
    日志搜索与 readiness 更新；非法参数返回字段错误）
    `cargo test -p neohub-mcp` → **22 passed**；`cargo test --workspace --locked` → **396 passed / 0 failed**；
    clippy **0 warnings**
  - 修掉的一个真实缺陷：`get_ports` 的过滤条件写反（给了 task_id 反而把该任务排除），
    测试直接抓到
  - 未解决问题：
    1. **rmcp Streamable HTTP 传输层**（默认 6872、Bearer、可选自签 TLS）仍未接入
    2. 与旧版返回 JSON 的 golden 对照用例
    3. 真实 MCP 客户端验收（initialize → tools/list → tools/call）与 Token 轮换实测
    4. MCP 修改后窗口/菜单栏事件（`CatalogChanged`）尚未在 dispatch 后发出

- 第三批完成（2026-11-07T07:45Z–08:20Z UTC）：
  - `mcp/src/server.rs`：`POST /mcp` 上的 JSON-RPC 2.0 端点
    （`initialize` / `tools/list` / `tools/call` / `ping` / `notifications/*`）
  - gate 在解析 body **之前**生效：Host → Origin → Bearer，全部失败即 403/401
  - `require_auth` 但未配置 Token 时返回 **401**（不会静默变成开放服务）
  - 工具错误以 MCP 规范的 `result.isError = true` 返回，内容为 `{field,message}` JSON
  - 出站内容统一过 `Redactor`
  - 验证：`cargo test -p neohub-mcp --test protocol` → **6 passed**
    - initialize → tools/list（22 个，全部 object schema）→ tools/call `list_apps` 读到真实应用
    - gate：`Host: localhost.evil.com` → 403、非 loopback Origin → 403、缺/错 Token → 401、
      loopback Origin（Inspector）→ 200
    - 未知方法 → `-32601`；未知工具 → `-32602`；参数缺失 → `isError` 且点名 `path`
    - 通知 → 202 且响应体为空
    - **Token 轮换后旧 Token 立即 401**，新 Token 200
    - 要求认证但无 Token → 401
  - `cargo test --workspace --locked` → **402 passed / 0 failed**；clippy **0 warnings**
- 关于 rmcp：`rmcp 3.9` 的 Streamable HTTP 传输以 tower/axum Service 形式装配，
  会把第二套 HTTP 栈带进桌面应用；本阶段改用**协议兼容的最小实现**（上述端点），
  工具层与传输层已解耦（`dispatch` 不依赖 HTTP），换回官方 SDK 是局部替换。
  差异：暂不支持 SSE 流式响应与 `initialize` 后的会话 id（会话在单进程内隐式存在）。
- 未解决问题：可选自签 TLS、SSE、工具变更后的 `CatalogChanged` 广播、与旧版返回 JSON 的 golden 对照。

- 第四批完成（2026-11-07T08:25Z–09:00Z UTC）：
  - **修掉一个真实缺陷**：`CoreEventKind::CatalogChanged` 从阶段 7 起只有定义、从未发出，
    窗口因此只能靠自己发起命令后手动更新。现在 `CoreHandle::request` 在**变更类命令成功**后
    统一广播（`CoreCommand::mutates_catalog()`），MCP 与 UI 共用这条路径，
    所以工具修改后窗口/菜单栏立即刷新
  - **字段与参数对齐旧版 `tools.ts`**（此前用 snake_case，客户端会读不到）：
    - `get_overview` → `{devharborVersion, appCount, running, crashed, activePorts, apps:[{name,state}]}`
    - `list_apps` → `{count, apps}`，brief 用 camelCase（`defaultScript`/`packageManager`/`autoStart`/`lastStartedAt`/`ports`/`state`），
      新增可选 `state` 过滤
    - `get_app` → 参数改为 `app`（id / 精确名称 / 绝对路径），保留 `app_id`/`path` 兼容旧客户端
  - 验证：`cargo test -p neohub-mcp` → **31 passed**
    - 新增：变更类工具广播 `CatalogChanged` 且 revision 递增；只读工具**不广播**
    - 新增：`read_tools_return_the_legacy_field_names`（camelCase 字段、`{count,apps}`、`state` 过滤、
      `get_app` 三种解析方式）
  - `cargo test --workspace --locked` → **405 passed / 0 failed**；clippy **0 warnings**
- 第五批完成（2026-11-07T09:05Z–09:45Z UTC）：按 `tools.ts` 继续对齐读取类工具
  - `list_running` → `{count, running:[{app, appId, taskId, pid, state, ready, listening, uptimeSec, cpu, memMB, ports}]}`
  - `search_logs` → 参数 `{query, regex, app?, limit}`；返回 `{interpretedAs, count, matches:[{app, task, line}]}`，
    `app` 支持 id/名称/路径并过滤
  - `list_env_vars` → 参数 `{app, task?}`（`scope` 保留兼容）；返回 `{app, scope, vars:[{key, value, isSecret, enabled}]}`，
    秘密值恒为 `null`
  - `scan_folder` → `{count, candidates}`；`list_node_versions` → `{installations}`
  - **新增 `CoreSnapshot.task_ready`**：`ready` 在旧版是"该 run 有真实探测才给出的结论"，
    现在由 `ready_flags` 导出；无探测时为 `null`，不会把"起来了"说成"就绪"
  - 验证：`cargo test -p neohub-mcp` → **31 passed**；`cargo test --workspace --locked` → **405 passed**；clippy 0
- 第六批完成（2026-11-07T09:50Z–10:20Z UTC）：变更类工具按 `tools.ts` 对齐
  - `update_app` → 参数 `app` + `name/defaultScript/autoStart/autoRestartOnChange/watchGlobs/tags/folder/portHint/nodeVersion`，
    返回 `{app: appBrief}`；`nodeVersion` 支持 `auto`/`system`/显式版本
  - `remove_app` → 参数 `app` + **强制 `confirm: true`**（否则返回字段错误），返回 `{removed: 名称, note}`
  - `add_task` → 参数 `app/name/script/command/workingDir/dependsOn/oneShot/enabled`；
    强制 **`script` 与 `command` 二选一**；`dependsOn` 可按任务名解析；`oneShot` 默认 readiness = 退出码 0
  - snake_case 全部保留为别名（`opt_field` 帮助函数），旧客户端与新客户端都能用
  - 验证：`cargo test -p neohub-mcp` → **32 passed**；`cargo test --workspace --locked` → **406 passed**；clippy 0
- 第七批完成（2026-11-07T10:25Z–11:00Z UTC）：读取类详情工具收口
  - `get_app` → 直接返回 `appBrief` + `state`/`ports`/`nodeVersionPref`/`autoRestartOnChange`/`watchGlobs`/
    `portHint`/`tasks`/`envFiles`/`recentRuns`，支持 `runs_limit`（默认 5）；
    `envFiles` 只列**文件名**（`find_env_files`），不读内容
  - `get_logs` → 参数 `{app, task?, lines}`（`task_id` 保留兼容）；返回 `{header:{app, linesPerTask, tasks:[{task,live,state,hasOutput}]}, text}`，
    多任务时每段带 `=== 名称 (状态) ===` 头，单任务不带
  - `get_ports` → `{apps:[{app, ports, portDetails:[{port, pid, process, task}]}]}`，或传 `port` 得到
    `{port, owner, task, process, note}`（优先真实 socket，不被日志猜测盖过）
    —— `pid`/`process` 明确为 `null`：NeoHub 只报**哪个任务**占用，不去猜进程名
  - 验证：`cargo test -p neohub-mcp` → **33 passed**
    （新增：`get_logs` 分节 + 单任务无节头 + `get_ports` 按应用分组）
  - `cargo test --workspace --locked` → **407 passed / 0 failed**；clippy **0 warnings**
- 第八批完成（2026-11-07T11:05Z–11:40Z UTC）：`add_app` 收口
  - 参数对齐：`path/name/script/command/workspaceTasks/autoStart`（`tasks`/`env` 数组保留为扩展）
  - 任务选择优先级与旧版一致：**workspaceTasks → 显式 `tasks` 数组 → `script` → `command` → 探测到的默认脚本**
  - `workspaceTasks: true`：用 core 的 `detect_workspaces`（npm/yarn `workspaces` + `pnpm-workspace.yaml`）
    为每个可运行包建一个任务，`working_dir_override = rel_path`，包名重复时追加相对路径；
    **重新做一遍越界检查**（拒绝绝对路径与 `..`）
  - `autoStart` 在创建后通过 `UpdateApp` 应用，并**回读应用**使返回值反映实际状态
  - 返回 shape：`appBrief` + `state`/`tasks`/`scripts`/`workspaces`/`note`
  - 验证：`cargo test -p neohub-mcp` → **35 passed**
    （新增：`add_app` 的 script/command/autoStart/默认脚本；monorepo 一包一任务且工作目录正确）
  - `cargo test --workspace --locked` → **409 passed / 0 failed**；clippy **0 warnings**
- 顺手修掉的两个真实缺陷（都是测试抓到的）：
  1. 显式 `tasks` 数组与默认任务同时创建会**重名冲突**导致整个创建失败 → 改为数组优先
  2. `task_json` 漏了 `working_dir_override`，monorepo 任务的工作目录在返回里看不到
- 第九批完成（2026-11-07T11:45Z–12:15Z UTC）：`set_readiness` 收口，阶段 14 基本完成
  - 参数对齐：`app` + `task`（id 或名称，可按应用作用域）+ `kind`，`task_id` 保留兼容
  - 按 kind 校验必填字段（`port`/`regex`/`url`/`ms`）并在缺失时报出**字段名**
  - `log` 的正则**当场编译校验**（写坏的 pattern 会成为永不匹配的静默黑洞），`flags` 默认 `"i"`
  - **`http` 的 url 必须 loopback**：NeoHub 自己轮询这个地址，非 loopback 会把就绪探测
    变成出网原语（SSRF）。这条是旧版有、我先前遗漏的安全约束，已补上并测试
  - 返回 `{app, task, readiness, note}`；`exit` 作为扩展保留（`oneShot` 任务需要）
  - 验证：`cargo test -p neohub-mcp` → **36 passed**
    （新增：按 kind 的必填校验、坏正则、非 loopback url 被拒、loopback 通过、flags 默认 i）
  - `cargo test --workspace --locked` → **410 passed / 0 failed**；clippy **0 warnings**
- 阶段 14 剩余已知差异（不影响默认安全姿态，随阶段 16 一并评估）：
  1. 可选自签 TLS 与 SSE 流式响应未做（默认 loopback + Bearer + 默认关闭已满足计划的安全要求）
  2. 错误消息文案未逐条照抄旧版（字段层面一致；`interpretedAs`、`note` 等为中文）
  3. `get_ports` 的 `pid`/`process` 为 `null`（只报占用任务，不猜进程名）

### 阶段 15：应用包与分发
- 状态：**完成**（.app + .dmg 可构建、可启动；签名走 ad-hoc，公证流程已写明）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: package NeoHub.app and a dmg`
- 实际修改文件：新增 `native/scripts/package.sh`、`native/docs/BUILD.md`；更新 `.gitignore`
- 验证命令与结果（2026-11-07T12:20Z–13:00Z UTC）：
  - `bash scripts/package.sh` → 成功
    - `NeoHub.app` **29 MB**，`lipo -archs` → `arm64`
    - `Info.plist` 由 `plutil -lint` 校验通过；`CFBundleIdentifier=app.neohub.desktop`、
      版本 `1.4.0`、`LSMinimumSystemVersion=13.0`
    - 无 Developer ID → **ad-hoc 签名**，`codesign --verify --strict` 通过
    - `NeoHub-1.4.0.dmg` **13 MB**（UDZO，含 `/Applications` 链接）
  - 通过 **LaunchServices** 启动（`open -a dist/NeoHub.app --args --data-dir <tmp>`）：
    - `lsappinfo` 报告 `bundleID="app.neohub.desktop"`、`bundle path=…/NeoHub.app`、
      `Version="1.4.0"`、`Arch=ARM64`、`type="Foreground"`
    - 窗口存在且 onscreen；数据库在指定 `--data-dir` 内初始化（`devharbor.db` + WAL + 锁文件）
    - `SIGTERM` 后正常退出，无残留进程
- 与旧版不同之处：
  1. 包结构手写组装（`scripts/package.sh`），不依赖 electron-builder；信息更透明，也少一层工具链
  2. 语言文件放在 `Contents/Resources/locales/*.json`，运行时读取，缺失时回退英文
  3. 图标沿用旧版 `build/icon.icns`
- 未解决问题及所属阶段：
  1. **公证未做**：没有 Developer ID 证书，只能 ad-hoc；分发到其他 Mac 会被 Gatekeeper 拒绝。
     流程写在 `native/docs/BUILD.md`，需要证书与 Apple 账号
  2. **窗口尺寸无法自动断言**：本机 `CGWindowListCopyWindowInfo` 对所有应用都返回失真尺寸
     （微信也是 144×146），因此改用"进程存活 + 窗口 onscreen + 数据目录初始化"作为启动证据；
     视觉细节需人眼确认
  3. 只有 arm64（未做 universal binary 与 x86_64 交叉构建）
  4. 菜单栏 NSStatusItem 的 target/action 绑定与 Dock 桥接（阶段 12 遗留）仍未完成

### 阶段 16：全量回归与性能对比
- 状态：**部分完成**（体积与 Rust 侧性能已实测；Electron 侧的启动/内存因数据隔离问题未测）
- 工作分支与目录：`rewrite/rust-gpui` @ `/Users/neil/Code/NeoHub-Rust`
- 最近提交：`feat: measure the packaged app and compare sizes`
- 实际修改文件：新增 `native/scripts/bench.sh`、`specs/rust-migration/PERFORMANCE.md`
- 验证命令与结果（2026-11-07T13:05Z–13:30Z UTC）：
  - `cargo test --workspace --locked` → **410 passed / 0 failed**；clippy 0 warnings
  - `bash scripts/bench.sh`（5 次，临时 `--data-dir`）→ 启动到数据库就绪
    **82 ms**（79–84），常驻 RSS **135 MB**（单进程）
  - 体积实测：Rust `.app` **29 MB** / `.dmg` **13 MB**；Electron `.app` **265 MB** / `.dmg` **104 MB**
    → 包小 **89%**，镜像小 **87%**
- 与旧版不同之处：Rust 版单进程；`--data-dir` 让运行时完全不碰真实数据
- 未解决问题及所属阶段：
  1. **Electron 侧启动/内存未测**：打包版的 `--user-data-dir` 被应用内部固定路径覆盖，
     任何启动都会操作真实数据库（详见下面的安全事件）。要做公平对比，需先让 Electron 版的
     userData 路径可被参数覆盖
  2. 首帧时间未测（需要屏幕录制权限）
  3. 大项目负载下的内存峰值未测

### ⚠️ 安全事件记录（需要知道）

在尝试测量 Electron 版启动时间时，我用 `--user-data-dir=<tmp>` 启动了一次
`dist/mac-arm64/NeoHub.app`，以为可以隔离数据。**实际上该参数被应用内部路径覆盖**，
进程以 `~/Library/Application Support/DevHarbor` 为 userData 启动（macOS 大小写不敏感，
与 `devharbor` 是同一目录），**指向了真实用户数据**。

发现后立即：

1. `pkill` 终止全部相关进程（含 GPU/network 子进程）
2. 校验真实数据库：
   - `shasum -a 256 devharbor.db` = `eadcd3d941210e0f128f443da9cb205fe126a9bee7cd30d8f57f77238009cbab`
     —— 与阶段 1 冷备份**逐字节一致**
   - `PRAGMA integrity_check` → `ok`
   - 行数一致：`4 apps / 4 tasks / 7 settings / 9 runs`
3. 结论：数据库**未被写入**；目录里新增的是两个空的 SQLite 附属文件
   （`devharbor.db-wal` 0 字节、`devharbor.db-shm`）以及 Chromium 自身的缓存文件，
   `devharbor.db` 本身字节未变（mtime 仍是 Nov 5）

后续规则（已写入本文件顶部约束）：**不再启动任何 Electron 打包版**，除非先确认其数据目录可隔离。

### 阶段 17：交付文档
- 状态：**进行中**（交付状态文档完成；功能缺口清单已明确，尚未闭环）
- 最近提交：`docs: record what the Rust build delivers`
- 实际修改文件：新增 `specs/rust-migration/DELIVERY.md`；**修正** `FEATURE-PARITY.md` 中 10 条过期状态
- 关键更正（文档诚实性）：对照代码后发现对照表落后于实现——`P6`/`P7`（应用级与任务级启停）、
  `E1`（三层环境变量）、`U4`/`U5`（导入与扫描面板）、`L3`（事件合批）等已实现却仍标"未开始/部分"，
  已逐条核对源码后更新。当前真实分布：**已验证 46 / 部分 32 / 未开始 11 / 说明 5**（共 94 项）
- 交付结论（写入 `DELIVERY.md`）：**核心可用、可打包、可本机日常使用；功能面约 8 成，
  还有 10 项旧版功能 + 菜单栏绑定未完成**，因此**不宣布"可直接替换旧版"**
- 未完成清单（按建议优先级）：
  1. NSStatusItem target/action 绑定（菜单栏闭环）
  2. S9 登录启动 + 每应用 autoStart
  3. U7 应用配置抽屉、S5 菜单快捷键
  4. S8 通知、S7 Open In、S6 深链接、S10 自动更新、S11 崩溃日志、B4 db 危险区
  5. 真实数据迁移（待用户确认）

### 阶段 12 遗留补齐：菜单栏 NSStatusItem 绑定
- 状态：**完成（代码与启动验证）**；点击行为需人工确认
- 最近提交：`feat: install a real menu bar item`
- 实际修改文件：新增 `native/crates/platform-macos/src/tray_native.rs`；
  更新 `platform-macos/Cargo.toml`（tray-icon 0.26 + muda 0.21）、`desktop/Cargo.toml`、`desktop/src/main.rs`
- 做法：用 `tray-icon` + `muda` 而不是手写 AppKit。状态项需要一个被 retain 的 target
  对象和一组带回调的菜单项，所有权写错表现为**点击时崩溃**而不是编译错误；这两个 crate
  正好封装了该模式。菜单**规则**仍留在 `tray.rs`（纯函数、可测），`tray_native.rs` 只做映射。
- 关键修正：`muda::MenuEvent::set_event_handler` 是**进程级单例**，最初在每个菜单项里设置
  （等价于只有最后一项能点），改为安装一次并查表分发
- 接入：`main.rs` 在 `run` 内、AppKit 启动后创建；动作在主循环的同一 tick 上 drain，
  `Open` 走 `reopen_or_focus`，启停/StopAll/Quit 走 `CoreHandle::request`（与 UI 同一条命令路径）
- 验证：`cargo test -p neohub-platform-macos --lib` → **9 passed**（含菜单与动作映射一致性、
  无运行时 StopAll 置灰）；`cargo test --workspace --locked` → **412 passed / 0 failed**；clippy 0
  重新打包（30 MB）并用临时数据目录启动：进程存活、日志无 error/panic
- 未解决问题：
  1. **菜单只在启动时构建一次**，应用状态变化后标题与逐项启用状态不会刷新（需按快照变更重建）
  2. 点击行为无法自动化验证（需要辅助功能/人工点击）
  3. `launch_at_login` 的 LaunchAgent 注册尚未实现（S9 只完成了每应用 autoStart）

### 菜单栏：随快照刷新（补上一批的遗留）
- 状态：**完成（构建与启动验证）**；点击行为仍需人工确认
- 最近提交：`feat: keep the menu bar in step with the core`
- 实际修改文件：`platform-macos/src/tray_native.rs`（抽出 `build_native_menu`/`install_event_router`、
  新增 `TrayHandle::refresh`）、`desktop/src/main.rs`（主循环按快照刷新）
- 要点：
  1. `refresh` 只在**行或动作**变化时重建菜单，纯标题变化（运行数）只 `set_title`，避免菜单闪
  2. 重建时**重新安装事件路由**——`muda` 的 handler 是进程级单例，菜单换了 id 也就换了
  3. `install`/`refresh` 共用同一个 `build_native_menu`，避免两条路径漂移
- 验证：`cargo test -p neohub-platform-macos --locked` → **10 passed**；
  `cargo test --workspace --locked` → **413 passed / 0 failed**；clippy 0；重新打包并启动，日志无 error/panic
- 学到的限制（已写入代码注释与测试名）：`muda` 在 macOS 上**不能在非主线程构建菜单**
  （会 panic），所以菜单映射只能靠纯模型测试覆盖，平台部分由启动验证兜底
- 未解决问题：`launch_at_login` 的 LaunchAgent 注册仍未实现（S9 只剩这一项）

### 登录启动 `launch_at_login`（S9 收尾）
- 状态：**完成**
- 最近提交：`feat: honour launch at login with a LaunchAgent`
- 实际修改文件：新增 `platform-macos/src/login_item.rs`；更新 `platform-macos/{lib.rs,Cargo.toml}`、
  `desktop/src/main.rs`
- 为什么不用 `SMAppService`：它要求签名并（macOS 13+ 上）公证的 bundle；当前只有 ad-hoc 签名，
  `SMAppService` 会失败。`osascript` 方案需要自动化权限。自写 LaunchAgent plist 是唯一对
  未签名开发构建也可靠的方式。
- 实现要点：
  1. plist 每次从**当前可执行文件**重新生成，bundle 移动后不会留旧路径
  2. `KeepAlive=false`：launchd 不该与应用的单实例锁打架；崩溃处理是应用自己的事
  3. `LimitLoadToSessionType=Aqua`，只在图形会话加载
  4. XML 转义：路径里有 `&`/`<` 会让 `launchctl` 拒绝解析，且失败表现是"登录项悄悄不见"
  5. 主循环每 tick 校正：设置与已安装状态不一致就重写，所以手动 `launchctl bootout`
     或移动 bundle 之后会自愈
- 验证：
  - `cargo test -p neohub-platform-macos --locked` → **14 passed / 1 ignored**
  - **真实 launchd 往返**（`--ignored` 测试，用 `/usr/bin/true` 而非应用本体，
    避免 `RunAtLoad` 在测试期间启动第二个实例）：install → `launchctl print`
    能查到 job → uninstall → job 消失
  - 验证后系统状态：`~/Library/LaunchAgents/app.neohub.desktop.plist` **不存在**、
    `launchctl print gui/501/app.neohub.desktop` → `Could not find service`（无残留）
  - `cargo test --workspace --locked` → **417 passed / 0 failed**；clippy 0
- 未解决问题：无（S9 完成）。设置界面上该开关已可切换，但**切换后到下一次 tick 才生效**（≤500 ms）。

### U7 应用配置编辑（第一批：模型）
- 状态：**模型完成**；面板视图未接线
- 最近提交：`feat: model the app configuration form`
- 实际修改文件：新增 `native/crates/ui/src/state/app_form.rs`；更新 `ui/src/state/mod.rs`
- 设计要点：
  1. **只提交改动过的字段**：未触碰的字段不进 patch，因此保存不会覆盖 MCP 或另一窗口刚改的字段
  2. 空值语义显式区分：「清空文件夹」是 `Some(None)`，与「没改」不同
  3. 列表用文本编辑：标签逗号分隔（**重复即报错**，不静默合并）、watch globs 每行一条
     （**拒绝绝对路径**，它们相对应用根目录）
  4. `nodeVersion` 接受 `auto`/`system`/具体版本，`v20.11.0` 会归一化为 `20.11.0`
  5. 保存被拒时保留用户输入（`apply_error` 只加错误，不动字段），成功后 `commit` 更新基线
- 验证：`cargo test -p neohub-ui --lib app_form` → **10 passed**
  （初始填充、空 patch、只提交改动字段、清空写 null、列表解析、重复标签、绝对 globs 被拒、
  版本三种形式与非法值、字段级错误、提交/拒绝语义）
  `cargo test --workspace --locked` → **427 passed / 0 failed**；clippy 0
- 未解决问题：`shell.rs` 的配置面板渲染与 `UpdateApp` 提交接线（下一批）

### U7 应用配置编辑（第二批：面板接线）
- 状态：**可用**（有一个明确的输入方式限制）
- 最近提交：`feat: edit app configuration in the window`
- 实际修改文件：`ui/src/views/shell.rs`（面板渲染、入口按钮、保存通道）、`ui/src/state/app_form.rs`（不变）
- 接线内容：
  1. 详情页新增「应用配置」按钮 → 用当前应用构造 `AppForm`
  2. 面板渲染 9 个字段 + 保存/取消；字段校验失败时**就地显示该字段的错误**（红色边框 + 文案），
     不关闭面板
  3. 保存走 `HostBridge::save_app_config` → `CoreCommand::UpdateApp`（与 MCP 同一条命令路径）
  4. 结果经 `last_app_save` 通道回传，`render` 里消费：成功则用**服务端返回值**重建表单基线，
     失败则把错误贴到面板上并保留输入
  5. 校验在**保存时**统一执行，错误一次性全部呈现（不是逐字段拦截）
- 验证：`cargo test --workspace --locked` → **427 passed / 0 failed**；clippy 0；
  重新打包（30 MB / dmg 13 MB）并用临时数据目录启动，日志无 error/panic
- **明确的限制（重要）**：面板没有行内文本输入，字段值是**点击循环取样**
  （如 Node 版本在 auto/system/20.11.0/22.0.0 之间切换，标签在预设组合间切换）。
  这是为了先打通"读到 → 改到 → 存回 → 报错"的完整链路；真正的文本编辑需要
  与任务编辑器同款的行内 `TextField` 接线（下一步）
- 未解决问题：
  1. 行内文本输入（当前只能循环取样）
  2. 面板未复用 `TextField` 的焦点处理，Tab 键不切换字段

### U7 应用配置编辑（第三批：行内文本输入）
- 状态：**完成**
- 最近提交：`feat: type into the app configuration fields`
- 实际修改文件：`ui/src/state/app_form.rs`（`TextField` + `apply_key` + `focus_field` + Tab）、
  `ui/src/views/shell.rs`（面板键盘处理、光标标记、点击聚焦；删除取样函数）
- 实现要点：
  1. 与任务编辑器同款：**一个** text buffer + 焦点字段索引，Tab/Shift-Tab 切换时把目标字段的
     当前值载入缓冲（切换不丢改动）
  2. 每次按键即写回模型并**清除该字段的错误**——用户正在修它，错误不该还挂着
  3. `Esc` 关闭面板（`KeyOutcome::Escape`），`Enter` 报 `Submit` 但**不**逐字段保存（保存走按钮）
  4. 焦点字段显示 `▌` 光标标记，未聚焦字段显示当前值或占位符
  5. 删掉了上一批的"点击循环取样"实现（已被真实输入取代）
- 验证：`cargo test -p neohub-ui --lib app_form` → **14 passed**
  （新增：输入落到焦点字段、Tab 切换带值且可回绕、编辑只清该字段错误、Esc/Enter 不被吞）
  `cargo test --workspace --locked` → **431 passed / 0 failed**；clippy 0；
  重新打包（30 MB / dmg 13 MB）并启动，日志无 error/panic
- 未解决问题：Shift-Tab 反向切换尚未实现（`KeyOutcome` 只给到 `tab`，需要从 keystroke 读 modifiers）；
  多行字段（watch globs）在单行缓冲里用换行较别扭

### S5 快捷键（第一批）
- 状态：**部分**（快捷键可用；AppKit 顶部菜单未建）
- 最近提交：`feat: add the window shortcuts`
- 实际修改文件：`ui/src/views/shell.rs`（actions、`bind_shortcuts`、`on_shortcut`、根元素 `on_action`）
- 实现的键位（与旧版一致的部分）：
  | 键 | 动作 |
  |---|---|
  | ⌘, | 打开设置面板 |
  | ⌘N | 打开导入面板并扫描默认目录 |
  | ⌘P | 聚焦侧栏筛选框 |
  | ⌘⇧F | 打开全局日志搜索 |
  | ⌘R | 重启当前选中的应用 |
- 设计点：**快捷键处理与按钮走同一条代码路径**（`on_shortcut` 调用与按钮相同的状态变更/命令），
  所以不会出现"按钮能做、快捷键做不到"的分叉
- 验证：`cargo test --workspace --locked` → **431 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：
  1. **AppKit 顶部菜单栏（NSMenu：关于/设置/退出/编辑菜单）未建**，所以这些动作只由快捷键触发，
     菜单里看不到
  2. 快捷键的实际按键行为需人工确认（自动化无法按键）
  3. `⌘P` 目前只"重新设置焦点句柄"，未真正把焦点移入筛选框（GPUI 的焦点需要在标签上用
     `track_focus`）；本轮先保证动作通路，焦点落在下一轮修正

### S5 焦点修正 + S7 Open In（模型与启动）
- 状态：S5 的 ⌘P 焦点**已修正**；S7 模型与启动**完成**，详情页接线待做
- 最近提交：`feat: open a project in another app`
- 实际修改文件：`ui/src/views/shell.rs`（`on_shortcut` 改为接收 `&mut Window`，Filter 分支真正
  `window.focus(&search_focus)`）、新增 `platform-macos/src/open_in.rs`、`platform-macos/src/lib.rs`
- S5 修正说明：上一版只重置了焦点句柄、没有把焦点交给窗口，等于**按了没反应**。现在调用
  `window.focus`，而筛选框本来就 `track_focus(&self.search_focus)`，链路完整
- S7 实现：
  1. 五个目标（Finder/Terminal/VS Code/Cursor/Sublime），一律走 `/usr/bin/open`：
     Finder 用 `open -R`（在父目录中**显示**），其余用 `open -a <app> <dir>`
  2. 可用性靠**目录探测**（`/Applications` 与 `~/Applications`），不猜注册表；未安装的目标不出现在菜单
  3. `launch` 会先检查目录存在，再以分离方式 spawn（不接管子进程输出）
- 验证：`cargo test -p neohub-platform-macos --lib open_in` → **7 passed**
  （Finder 用 `-R`、其他用 `-a`+app 名、含空格路径作为单个 argv、探测与文件系统一致、
  顺序稳定、缺目录被拒、真实 `open -R` 可达）
  `cargo test --workspace --locked` → **438 passed / 0 failed**；clippy 0
  本机实际安装：Visual Studio Code 有，Cursor / Sublime 无 —— 探测结果因此只给三项菜单
- 未解决问题：
  1. 详情页/右键菜单还没挂 Open In 按钮（模型与启动已就绪）
  2. AppKit 顶部菜单栏（NSMenu）仍未建

### S7 Open In（第二批：详情页接线）
- 状态：**完成**
- 最近提交：`feat: open a project from the detail pane`
- 实际修改文件：`ui/src/views/shell.rs`（详情页按可用目标生成按钮）、`ui/Cargo.toml`
  （`neohub-platform-macos` 从 dev 依赖提升为正式依赖）
- 要点：按钮组由 `available_targets()` 驱动，**本机没装的目标不会出现**（不是灰掉，是不显示），
  点击直接 `open_in::launch`，失败只打日志不弹窗（外部应用打不开不该打断工作）
- 验证：`cargo test --workspace --locked` → **438 passed / 0 failed**；clippy 0；
  重新打包（30 MB / dmg 13 MB）并启动，日志无 error/panic
  本机探测：Finder + Terminal + VS Code 三个按钮，Cursor/Sublime 不显示
- 未解决问题：无（S7 完成）。顶部菜单栏（NSMenu）仍缺，属 S5

### B4 数据库导出 / 重置 / 路径
- 状态：**后端完成**；UI 危险区按钮待接
- 最近提交：`feat: export and reset the database safely`
- 实际修改文件：新增 `core/src/database.rs`、`core/tests/database_ops.rs`；更新
  `core/src/{lib,service}.rs`（`storage_slot`、`running_tasks`、`with_storage` 放宽到 `pub(crate)`）、
  `core/Cargo.toml`（dev 依赖 rusqlite 0.37，与 storage 同版本）
- 语义（沿用旧版 `db:*` 的两个关键决定，它们各自避免一种**静默数据丢失**）：
  1. **导出用 `VACUUM INTO`，不是文件复制**：WAL 模式下最新写入在 `-wal` 里，
     只拷 `.db` 会得到一个能打开但少了最近若干分钟的文件。目标已存在时**拒绝**（不覆盖）
  2. **重置先关连接**：checkpoint 把 WAL 折进主文件，然后把 `db`/`-wal`/`-shm`
     **三件一起**改名为 `.reset-<ts>.bak`；留下 `-wal` 会让下次启动把旧行恢复进"新"库
  3. 有任务在运行时**拒绝重置**（否则 UI 会描述一个连历史都写不进去的任务）
- 验证：`cargo test -p neohub-core --test database_ops` → **4 passed**
  （路径就是活动库文件；导出副本有 app/task 且 `integrity_check=ok`；覆盖已存在文件被拒且
  原文件字节不变；重置后备份存在、备份里仍有原数据、新库为空且可再写）
  `cargo test --workspace --locked` → **442 passed / 0 failed**；clippy 0
- 未解决问题：
  1. 设置面板的「危险区」按钮（导出对话框 / 重置确认）未接
  2. 导出走的是路径参数，没有原生 `NSSavePanel`；UI 接线时需要一个选路径的入口

### S11 应用日志文件与 panic 捕获
- 状态：**基础设施完成**；设置面板入口待接
- 最近提交：`feat: write the host's own log file`
- 实际修改文件：新增 `platform-macos/src/log_file.rs`；更新 `platform-macos/{lib.rs,Cargo.toml}`
  （chrono、dev 依赖 tempfile）、`desktop/src/main.rs`（启动即初始化 + 记录数据目录 + 第二实例分支记录）
- 为什么需要：UI 里看到的是**项目的**日志。NeoHub 自己出问题（host panic、窗口打不开）时，
  进程可能在屏幕上留下任何东西之前就死了，而 Console.app 不索引应用自写的日志。
  一个放在 `~/Library/Logs` 下的普通文件是用户能直接附到 bug 报告里的东西
- 实现要点：
  1. 路径 `~/Library/Logs/NeoHub/neohub.log`（与旧版同目录、不同文件名，不覆盖旧版日志）
  2. 行格式 `2026-11-06T15:37:23Z [host] message`（UTC，ISO-8601）
  3. **写入时轮转**：超过 5 MiB 就改名为 `.1`（只保留一份）。轮转失败不影响当前这行写入
  4. **panic hook 先写日志再调默认 hook**：原 stderr 行为不变，只是多了一条落盘证据
  5. `init` 在 `main` 最开头调用，所以之后的任何 panic 都能落盘
- 验证：`cargo test -p neohub-platform-macos --lib log_file` → **6 passed**
  （路径、行格式与 UTC 时间戳、追加顺序、超限轮转且只保留一份、panic 内容落盘、
  sink 未初始化时静默）
  `cargo test --workspace --locked` → **448 passed / 0 failed**；clippy 0
  打包后真实启动：`~/Library/Logs/NeoHub/neohub.log` 出现
  `[host] NeoHub starting` 与 `[host] data dir: …`
- 未解决问题：
  1. 设置面板没有「打开日志目录」按钮（`log_file::reveal` 已实现，等 UI 接线）
  2. 只有 host 写日志；runner/UI 的 warn/error 尚未接入同一 sink

### B4 + S11 的 UI 收口（设置面板危险区）
- 状态：**B4、S11 完成**
- 最近提交：`feat: add the settings danger zone`
- 实际修改文件：`core/src/{commands,service}.rs`（数据库命令）、
  `ui/src/views/shell.rs`（桥接方法、危险区渲染、重置二次确认、通知通道）、
  `core/tests/database_ops.rs`（命令面测试）
- 内容：
  1. 命令面补齐：`DatabasePath` / `ExportDatabase { path }` / `ResetDatabase`，
     `ResetDatabase` 计入 `mutates_catalog`（重置后广播 `CatalogChanged`，窗口自动刷新）
  2. 设置面板底部「诊断与危险区」：
     - **打开日志目录**（`log_file::reveal`，先建目录再 reveal）
     - **导出数据库…** → `<data-dir>/exports/neohub-<stamp>.db`（自动取名，不需要原生保存对话框）
     - **重置数据库…** → **两次点击**：第一次按钮变红并提示"再点一次确认重置（旧库会改名保存）"，
       第二次才执行
  3. 操作结果显示在面板里（成功路径含具体文件/备份数量，失败显示错误原文）
- 验证：`cargo test -p neohub-core --test database_ops` → **5 passed**（新增命令面往返：
  路径 → 导出 → 重置，且重置后快照为空）
  `cargo test -p neohub-ui --locked` → **98 + 4 + 4 + 6 passed**
  `cargo test --workspace --locked` → **449 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：
  1. 导出没有原生保存对话框（固定落在数据目录的 `exports/`），用户不能选位置
  2. 重置后不自动重启：当前会话已用新库继续运行（旧库在磁盘上，重启也可）

### S8 桌面通知（崩溃）
- 状态：**崩溃通知可用**；`notify_on_ready` 未接
- 最近提交：`feat: notify on a crash`
- 实际修改文件：新增 `platform-macos/src/notify.rs`；更新 `platform-macos/src/lib.rs`、
  `ui/src/views/shell.rs`（事件处理里按开关发通知）
- **为什么不用 `UNUserNotificationCenter`**：它要求正确签名（macOS 13+ 还要公证）的 bundle，
  ad-hoc 构建下通知**根本不投递**，且不给用户可处理的错误；`NSUserNotification` 已废弃。
  走 AppleScript 的 `display notification` 是唯一在未签名开发构建下真正会出现的路径，
  而且仍然经过系统通知中心（专注模式、勿扰、通知中心都照常生效）。
  代价是横幅来源显示为脚本宿主而非 NeoHub —— 换来"通知真的能到"，我认为值得。
  以后换 `UNUserNotificationCenter` 只需改这一个模块。
- 语义：**只对意外退出发通知**。用户自己点的停止不是新闻，把用户刚做的事再通知一遍
  正是通知被关掉的原因（这条规则写成 `for_task_exit`，有测试）
- 防注错：标题/正文按 AppleScript 字面量转义（反斜杠 → 双写、双引号 → `\"`、换行 → 空格）。
  未转义的引号会让脚本在运行时失败，**静默丢失通知**
- 发送在 store 锁**之外**执行：`osascript` 是进程启动，持锁会卡住所有其他更新
- 验证：`cargo test -p neohub-platform-macos --lib notify` → **6 passed**
  （脚本形状、引号/反斜杠转义、换行折叠、空通知先拒、只对意外退出通知、崩溃文案含应用与任务名）
  `cargo test --workspace --locked` → **455 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：
  1. `notify_on_ready`（任务就绪通知）未接
  2. 通知实际弹出需人工确认（自动化不能点授权/看横幅）
  3. 用户在"系统设置 → 通知"里若禁用脚本宿主，通知不会出现

### S10 更新检查
- 状态：**检查可用**；下载/安装明确不做
- 最近提交：`feat: check for a newer release`
- 实际修改文件：新增 `core/src/update.rs`；更新 `core/{lib.rs,Cargo.toml}`（reqwest blocking+json）、
  `ui/src/views/shell.rs`（桥接 `check_for_updates`、设置面板「检查更新」/「打开下载页」/状态文案）
- 范围决定（写进模块文档）：**只检查与跳转下载**，不自动安装。安全地自动安装需要签名+公证的
  bundle 与安装步骤；一个能给未签名构建装更新的通道是**安全回退**，不是功能。旧版 `electron-updater`
  同样没有接 live feed（PROGRESS Phase 4 备注），所以这是"对齐 + 真实检查"，不是隐藏缺口。
- 实现要点：
  1. `normalise_version` 处理 `v1.4.0`/`1.4.0`/`release-1.4.0`，**部分版本（`v1.4`）不算版本**
  2. 用 `semver` 比较：`1.10.0 > 1.9.9`（字符串比较会得出相反结论）
  3. 任一侧无法解析 → 报"已是最新"并显示当前版本，**不猜有更新**
  4. 缺 `html_url` 时回落到 releases 页面
  5. 网络失败只在面板显示错误，不弹窗、不打断
- 验证：
  - `cargo test -p neohub-core --lib update` → **6 passed**
  - **真实网络往返**（`--ignored`）：`check_blocking("0.0.1")` 从 api.github.com 拿到最新发布并判定
    "有更新" —— 已实跑通过
  - `cargo test --workspace --locked` → **461 passed / 0 failed**；clippy 0
  - 重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：
  1. 没有自动安装（见上，属有意不做）
  2. 没有"忽略此版本"记忆；每次打开面板点检查都会重新提示

### S5 收尾：AppKit 顶部菜单
- 状态：**完成**（菜单项点击需人工确认）
- 最近提交：`feat: install the application menu`
- 实际修改文件：新增 `platform-macos/src/{app_menu,menu_router}.rs`；更新
  `platform-macos/src/{lib,tray,tray_native}.rs`、`desktop/src/main.rs`
- **先修的一个真实缺陷**：`muda` 全进程只有一个 `MenuEvent` handler。上一批托盘是"谁最后安装谁生效"，
  再加一个应用菜单就会让**托盘点击静默失效**。现在抽出 `menu_router`：单一 handler + 按来源
  （`Tray`/`AppMenu`）登记路由，重建某个菜单只替换它自己的行。这条有测试（托盘重建不影响应用菜单）
- 菜单结构（只放真能做的事，不做点不动的行）：
  - **NeoHub**：设置…（⌘,）、打开日志目录、退出（⌘Q）
  - **文件**：打开窗口（⌘O）、检查更新…
- 动作处理：打开窗口走 `reopen_or_focus`；打开日志目录走 `log_file::reveal`；
  检查更新写进应用日志（窗口可能是关的）；退出走 core shutdown + `cx.quit()`
- 验证：`cargo test -p neohub-platform-macos --locked` → **37 passed / 2 ignored**（新增菜单模型、
  加速键可解析、行都必须有真实动作、路由按来源替换）
  `cargo test --workspace --locked` → **465 passed / 0 failed**；clippy 0
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：
  1. 菜单点击与顶部菜单的实际显示需人工确认（辅助功能权限不可用，`System Events` 拒绝查询）
  2. 「设置…」目前只把窗口带到前台，未直接把设置面板打开（需要向视图派发 GPUI action）
  3. 没有「编辑」菜单（文本框自己处理按键，标准编辑项会与之冲突）

### S6 深链接（解析与注册）
- 状态：**部分**（解析 + scheme 注册 + argv 路径可用；Apple Event 接收未接）
- 最近提交：`feat: parse neohub:// links`
- 实际修改文件：新增 `platform-macos/src/deep_link.rs`；更新 `platform-macos/src/lib.rs`、
  `scripts/package.sh`（`CFBundleURLTypes`）、`desktop/src/main.rs`、`ui/src/views/shell.rs`
  （`apply_deep_link`）、`ui/src/state/store.rs`（`apps()`）
- 已实现：
  1. `neohub://open?id=<id>` / `neohub://open?path=<绝对路径>` / `neohub://start?id=<id>`
     解析，含百分号解码与 `+` → 空格
  2. **把外来输入当敌意输入**：id 只允许字母数字和连字符（`../../etc/passwd`、含空格的 id 一律丢弃）；
     path 必须绝对且不含 `..` —— 链接不能把人指向项目目录之外
  3. `Info.plist` 注册 `neohub` scheme（已用 `plutil -p` 确认写进包）
  4. 通过 argv 传入的链接（`open -a NeoHub --args neohub://…`）会被解析、记日志并用来**选中**应用
- **明确未做**：`kAEGetURL` Apple Event 接收（需要 `NSAppleEventManager` handler 与 ObjC target）。
  所以从浏览器点 `neohub://` 只会激活应用，**不会跳到具体项目**。这条写在模块文档与本文里，
  不让链接看起来能用。
- 安全决定：链接只能**选中**，不能启动。来自浏览器的 URL 不构成"跑某人的 dev server"的授权。
- 验证：`cargo test -p neohub-platform-macos --lib deep_link` → **10 passed**
  （id/路径/组合、旧 scheme 忽略、未知 host、非法 id、路径穿越与相对路径被拒、空格与中文路径、
  argv 取第一个链接、无链接时无动作）
  `cargo test --workspace --locked` → **475 passed / 0 failed**；clippy 0
  打包后 `plutil -p` 确认 `CFBundleURLSchemes = [neohub]`；带
  `neohub://open?id=demo` 启动，日志出现
  `deep link requested: Open { app_id: Some("demo"), path: None }`
- 未解决问题：
  1. Apple Event 接收未接（上面那条）
  2. 选中发生在第一帧快照之前，若快照还没到则本次不生效（不排队重试）

### 阶段 17 收尾：完成度校正 + 补上真正的 autoStart
- 状态：**交付文档与代码一致**
- 最近提交：`fix: actually start auto_start apps at boot`
- **又抓到一处自己的错报**：`FEATURE-PARITY` 的 S9 写着"每应用 `autoStart` 已在服务启动流程接线"，
  但代码里 `auto_start` 从未被读取（`reopen()` 固定返回 `run_auto_start: false`，也没有遍历逻辑）。
  已实现 `CoreService::start_auto_start_apps`：按依赖顺序、跳过已在运行的、单个失败不影响其余、
  结果（成功/失败名单）写入应用日志；`desktop` 在窗口打开后异步拉起，不阻塞首帧
- 校正对照表：逐条回源码核实后，把 10 行过期状态更新到与代码一致
  （S9/R4/U3/U12/U9/D5/A5/P1/E7/U8/U6/L8/U14）
  → 当前 **64 已验证 / 24 部分 / 1 未开始 / 5 说明**（共 94 项）
- `DELIVERY.md` 同步：结论从"约 8 成、10 项未做"改为"主要功能面已对齐，剩 3 项且有明确原因"
- 验证：`cargo test -p neohub-core --test database_ops auto_start` → **1 passed**
  （只有 `auto_start` 的应用被尝试、二次调用不重复启动）
  `cargo test --workspace --locked` → **476 passed / 0 failed**；clippy 0

### U11 命令面板（第一批：模型）
- 状态：**模型完成**；⌘K 与面板渲染未接（按你的选择，这是下一步的重点）
- 最近提交：`feat: model the command palette`
- 实际修改文件：新增 `ui/src/state/command_palette.rs`；更新 `ui/src/state/mod.rs`、
  `ui/src/state/app_form.rs`（顺手修掉一处同类缺陷，见下）
- 模型要点：
  1. 两组：**应用**（打开/启动/停止/重启，按当前状态过滤）+ **操作**（设置/导入/全局搜索）
  2. 会"什么都做不到"的行**不出现**（空闲应用没有"停止"，运行中的应用没有"启动"）——
     否则用户会误触到无效行
  3. 排序稳定：组内按名称（再按 id）排，与侧栏一致，避免每次打开位置乱跳
  4. 匹配是**子序列**（`ovr` 能命中 `overview`），这是面板"跟手"的关键
  5. 方向键环绕、Enter 执行、Esc 关闭、空结果时 Enter 不是崩溃
- **顺手修的同类缺陷**：`TextField::new(x)` 的参数是**初始文本**不是 id。命令面板最初传 `"palette"`
  导致查询里带着这个词（测试立刻抓到）；`AppForm` 里也有同样的误用（`new("app_form")`），
  因为随后 `set_text` 覆盖才没暴露出来 —— 一并改正
- 验证：`cargo test -p neohub-ui --lib command_palette` → **9 passed**
  （两组行、按状态过滤、输入缩小范围并重置光标、子序列匹配、方向键环绕、Enter 取当前行、
  Esc/未知键、空快照仍有全局动作、无匹配时不崩）
  `cargo test --workspace --locked` → **485 passed / 0 failed**；clippy 0
- 未解决问题：⌘K 快捷键与面板渲染（下一批）

### U11 命令面板（第二批：⌘K 与面板）
- 状态：**完成**
- 最近提交：`feat: open the command palette with cmd-K`
- 实际修改文件：`ui/src/views/shell.rs`（⌘K 动作与绑定、面板渲染、行点击、动作执行、
  `palette`/`palette_focus` 状态）、`ui/src/state/store.rs`（`snapshot_for_palette`）
- 接线内容：
  1. ⌘K 打开，查询框 + 行列表，每行右侧显示状态或对应快捷键
  2. 键盘：输入过滤、↑↓ 环绕、Enter 执行、Esc 关闭；鼠标点击任意行也能执行
  3. 动作**复用已有路径**：打开应用走会话导航，启停/重启走 `CoreCommand`，
     设置/导入/搜索走 `on_shortcut`（与按钮、快捷键完全同一条路）
- 实现中踩到并修掉的坑：局部变量 `palette` 遮蔽了调色板 `palette`，导致一片"找不到函数"的
  编译错误 —— 改名为 `palette_state` 并加注释说明谁是谁
- 验证：`cargo test --workspace --locked` → **485 passed / 0 failed**；
  命令面板模型 9 项测试全绿；clippy 0；重新打包（30 MB）并启动，日志无 error/panic
- 对照表现在是 **65 已验证 / 24 部分 / 0 未开始**（共 94 项），`DELIVERY.md` 同步更新
- 未解决问题：键盘/鼠标的实际交互需人工确认（无法自动化按键）

### U2 CPU sparkline（UI 细节第一批）
- 状态：**完成**
- 最近提交：`feat: draw a CPU sparkline on each dashboard card`
- 实际修改文件：新增 `ui/src/state/sparkline.rs`；更新 `ui/src/state/{mod,store}.rs`、
  `ui/src/views/shell.rs`（卡片渲染）
- 为什么用块字符而不是画路径：不占 GPU 状态、任何字体都渲染正确、在卡片这个尺寸上最好读。
  将来换真图表时，只需要删掉这一个模块。
- 两条让图"不说谎"的规则：
  1. **每张卡按自己的近期峰值缩放** —— 用绝对百分比会让安静的项目挨着繁忙项目时看起来是死的
  2. **空闲画平线**（全零时画最低块），不画上升趋势；缺失采样不能被画成增长
- 采样时机：**应用快照时**采样，不是渲染时 —— 卡片一次快照会重绘很多次，按渲染采样会让
  曲线取决于重绘频率而非时间。应用被删除后历史一并清理，长会话不会无限增长
- 验证：`cargo test -p neohub-ui --lib sparkline` → **8 passed**（窗口保留最新、空序列、
  峰值/零块、平线、常量序列、宽度裁剪、NaN/负数/∞ 不污染刻度、容量 0 仍有 1 个）
  `cargo test -p neohub-ui --lib sparkline_sampling` → **2 passed**（每快照一采样、删应用后清理）
  `cargo test --workspace --locked` → **495 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：卡片空态文案（没有应用时的提示）仍未做

### U1/U2 侧栏折叠与空态（UI 细节第二批）
- 状态：**完成**
- 最近提交：`feat: collapse sidebar sections and guide an empty dashboard`
- 实际修改文件：`ui/src/state/store.rs`（`SidebarSection.collapsed`、`toggle_section`、`has_no_apps`）、
  `ui/src/views/shell.rs`（分组头可点击、空态面板）
- 内容：
  1. **分组折叠**：点分组标题折叠/展开，标题前用 `▸`/`▾` 指示方向（不悬停也能看懂点击会怎样）。
     折叠只影响显示，行仍在模型里 —— 快照刷新不会把状态弄丢
  2. 折叠状态**只存内存**：折叠一个文件夹是随手动作，不值得写进 `session.json`
  3. **空态引导**：没有项目时显示"还没有项目 / 扫描一个目录…"与「导入项目…」按钮，
     而不是一片空白网格
- 验证：`cargo test -p neohub-ui --lib section_tests` → **3 passed**
  （折叠只影响被点的分组、再次点击展开、兜底分组也可折叠、无应用时明确报告）
  `cargo test --workspace --locked` → **498 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：⋮ 菜单与拖拽分组（U1 剩余部分）

### U13 Toast 队列（UI 细节第三批）
- 状态：**部分完成**（统一队列与浮层完成；ErrorBoundary 未做）
- 最近提交：`feat: show transient messages as toasts`
- 实际修改文件：新增 `ui/src/state/toast.rs`；更新 `ui/src/state/{mod,store}.rs`、
  `ui/src/views/shell.rs`（浮层渲染、点击关闭、每帧过期、数据库结果接入）
- 决定这个模块形状的规则：**错误不过期**。自动消失的错误等于用户没读到，而会报错的恰恰是
  "保存失败""重置数据库失败"这类必须知道的事。信息/成功类 4 秒后过期，因为它们只是确认
  刚刚看得见地发生过的事
- 其它要点：
  1. 队列满时**优先淘汰会过期的**消息 —— 一串"已导出"不能把一条错误顶出去
  2. 若屏幕上全是错误且队列满，仍淘汰最旧的一条，否则新消息永远看不见
  3. 错误用 `danger_bg` 着色，点击任意 toast 关闭
  4. 数据库操作结果（导出/重置/失败）现在既留在设置面板，也作为 toast 出现 —— 因为操作完成时
     面板可能已经关了
- 验证：`cargo test -p neohub-ui --lib toast` → **7 passed**
  （顺序与类型、错误不过期、确认类爆发不挤掉错误、全错误时淘汰最旧、按 id 关闭、id 唯一、
  容量 0 仍保留一条）
  `cargo test --workspace --locked` → **505 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：`ErrorBoundary`（渲染期 panic 的兜底 UI）未做；其余错误（保存任务、环境变量）
  目前仍是行内提示，未全部改走 toast

### U10 运行历史重跑
- 状态：**完成**
- 最近提交：`feat: rerun a task from its history row`
- 实际修改文件：`ui/src/views/shell.rs`（历史行加重跑按钮）
- 语义决定：重跑**用历史记录里的任务 id 重新启动同一个任务**，不是拿历史里的脚本另建一个。
  那条历史确实是这个任务按当前配置跑出来的；如果任务已被删除，就用错误 toast 说明"没有关联任务，
  无法重跑"，**不擅自重建**一个用户没要求的任务
- 反馈：点击后立即给一条 info toast（"已重新启动 …"），失败路径给 error toast
- 验证：`cargo test --workspace --locked` → **505 passed / 0 failed**；clippy 0；
  重新打包（30 MB）并启动，日志无 error/panic
- 未解决问题：重跑按钮的实际点击需人工确认；历史列表仍只取最近 5 条，没有独立子视图

### 打包自检：产物不得落后于源码
- 状态：**完成**
- 最近提交：`build: fail the package step when the bundle is stale`
- 背景：Rerun 那一轮我按"打包 → 启动验证 → 修 clippy 警告 → 提交"的顺序做完，最后一次改动后
  **没有重新打包**。产物能启动、跑的是上一版代码 —— 这种"看起来成功"的失败最难发现
- 现在 `scripts/package.sh` 结束前自检：若有任何 `crates/**/*.rs` 比包内二进制新，
  **打印这些文件并以退出码 1 失败**
- 验证：
  - 正常路径：重新打包后输出 `up to date with …/crates`，落后文件数 0
  - 失败路径：`touch crates/ui/src/lib.rs` 后单独运行该检查 → 打印该文件并 `exit=1`
  - 之后重新打包已恢复一致（`touch` 只改 mtime，源码内容未变）

### U5 批量导入
- 状态：**完成**
- 最近提交：`feat: import several projects at once`
- 实际修改文件：`ui/src/state/store.rs`（`ImportState.selected`、勾选/全选/标记已导入）、
  `ui/src/views/shell.rs`（候选行勾选框、全选与批量按钮、`HostBridge::import_selected`、
  `HostUpdate::ImportedOne`/`BatchImported`）
- 设计要点：
  1. **逐个顺序导入**：每次导入是 app+tasks+env 的一个事务；并行会让失败报告取决于调度顺序
  2. **成功即标记**：部分失败后重试只会重新导入剩下的（`mark_candidate_registered` 同时取消勾选）
  3. **已注册的候选不能勾选**（显示 `—`）：给出一个服务端会拒绝的选项是假承诺
  4. 新扫描会**清空上次的勾选**，避免对着旧列表点"导入选中"
  5. 结果汇总为一条 toast：全成功给数量，部分失败给"导入 N 个，M 个失败：…"
- 验证：`cargo test -p neohub-ui --lib batch_import` → **5 passed**
  （勾选只影响可选候选、全选覆盖可选且再点清空、新扫描清空选择、按扫描顺序输出、
  标记已导入后取消勾选）
  `cargo test --workspace --locked` → **510 passed / 0 failed**；clippy 0
  打包自检通过（`up to date with …/crates`），启动日志无 error/panic
- 未解决问题：批量导入的实际点击需人工确认

### U1 行内 ⋮ 菜单
- 状态：**完成**（U1 只剩拖拽分组未做）
- 最近提交：`feat: add a per-row action menu`
- 实际修改文件：`ui/src/state/store.rs`（`open_row_menu`、`toggle_row_menu`、`close_row_menu`）、
  `ui/src/views/shell.rs`（⋮ 按钮、内联菜单渲染、`row_menu_actions`、`run_row_action`）
- 设计要点：
  1. **内联展开而非浮层**：侧栏很窄，浮层会盖住它要操作的那些行
  2. 同一时刻**只开一个**菜单；再点同一行会关闭（与菜单栏行为一致）
  3. 动作按状态过滤（与命令面板同一条规则）：运行中才有"停止/重启"，否则只有"启动"
  4. 每个动作走**与按钮相同的路径**：启停/重启走 `CoreCommand`，"应用配置"打开同一个
     `AppForm` 面板，"在 Finder 中显示"复用 `open_in`
- 验证：`cargo test -p neohub-ui --lib row_menu` → **4 passed**
  （同时只开一个、再点关闭、close 清空、初始未打开）
  `cargo test --workspace --locked` → **514 passed / 0 failed**；clippy 0；
  打包自检通过，启动日志无 error/panic
- 未解决问题：拖拽分组（U1 最后一项）；菜单点击的实际效果需人工确认

### 状态核实 + L2 日志可见性门控
- 状态：**完成**
- 最近提交：`feat: push log events only for the visible tasks`
- **先做了一件必要的事：核实过期状态**。逐条查源码后修正 5 行：
  - `N3`（登录 shell PATH 探测）→ 已验证：`path_probe.get()` 确实在启动流程的 env 构建里被调用
  - `N4`（node 解析）→ 已验证：`resolver.resolve` 在 `start_task` 路径里
  - `P4`（PTY resize）→ 仍是部分：`CoreCommand::ResizeTask` 有处理，但 **UI 从不发送**
  - `N1`（node 版本）→ 仍是部分：MCP 有 `list_node_versions`，UI 没有版本下拉
  - `D6`（IPC 契约）→ 改为**不适用**：Rust 版是同进程调用，不存在 IPC/preload 边界
- L2 实现：
  1. 新命令 `SetLogFocus { tasks }`：`None` = 推送全部
  2. 事件泵发 `Log` 事件前查焦点集合，不在其中就**跳过推送**（`revision` 都不 bump）
  3. **字节永不丢弃**：环形缓冲照常写入，切到该任务后 `GetTerminalText`/bootstrap 仍拿到完整输出
  4. UI 只在**可见任务集合变化时**发送该命令（每帧比较，避免每帧发命令）；搜索面板打开或
     未选应用时发送 `None`（需要全部）
- 验证：`cargo test -p neohub-core --test log_focus` → **2 passed**
  （焦点只放行被关注任务的事件，**而且被门控任务的输出仍可读**；清空焦点后全部推送）
  `cargo test --workspace --locked` → **516 passed / 0 failed**；clippy 0；打包自检通过
- 未解决问题：门控的实际收益需在多个后台任务运行时观察；`L3` 的 16 KiB 分块上限仍未做

### E7/U8 环境变量行内值编辑
- 状态：**完成**
- 最近提交：`feat: edit environment values inline`
- 实际修改文件：`ui/src/views/shell.rs`（值单元格可点击进入编辑、键盘处理、Tab 跳行）、
  `ui/src/state/env_editor.rs`（新增 4 个语义测试）
- 行为：
  1. 点某一行的值 → 该行进入编辑态，缓冲**预填当前值**（不是空）
  2. 编辑时该行显示**缓冲内容**（带 `▌`），不是原值 —— 否则光标会走在"不是最终保存内容"的文本上
  3. `Enter` 提交到模型，`Tab` 提交并跳到下一行（环绕），`Esc` 取消不再改动
  4. 秘密行编辑后**仍然保持 secret 标记**
- **关键安全语义（有测试）**：未编辑的秘密行提交时 `value = None`（保留原值）；
  如果把掩码当成值提交回去，就会把真实密钥覆盖成 `••••••`
- 验证：`cargo test -p neohub-ui --lib inline_edit` → **4 passed**
  （编辑后保存带上新值、秘密编辑后仍是秘密、未编辑的秘密行不回传值、掩码显示不是 "null"）
  `cargo test --workspace --locked` → **520 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：键、备注、启用状态的**键内编辑**仍靠按钮；行内编辑的实际点击需人工确认

### E6 密钥不可用：让失败可见
- 状态：**完成**
- 最近提交：`feat: say what failed when a command errors`
- 发现的问题：`HostBridge::send` 是 `let _ = handle.request(...)` —— **所有命令错误被静默丢弃**。
  所以"点了启动没反应"是真的没有任何提示，而密钥不可用正是其中一种
- 修改：
  1. `send` 在失败时回传 `HostUpdate::CommandFailed { label, message }`
  2. 新增 `ui/src/state/command_labels.rs`：把命令翻译成"启动/停止/重启/保存环境变量…"，
     文案短（≤8 字），因为它在 toast 里是"启动失败：…"
  3. drain 里统一转成 **error toast**（不自动过期）；对密钥错误追加
     "（该项目的密钥无法用它当前可用的密钥材料解密，请到环境变量里重新录入）"
- 后端本来已正确：`SecretUnavailable` 会**阻止启动**（`core_service.rs` 有测试），
  缺的只是把它说出来
- 副作用记录：这个模块的 `#[test]` 放进 `shell.rs` 会触发 `recursion limit reached while
  expanding #[test]`（文件太大）。因此把纯函数移到独立模块 —— 顺便更符合"小模块可测"
- 验证：`cargo test -p neohub-ui --lib command_labels` → **3 passed**
  （启停标签、未映射命令有兜底标签、标签长度 ≤8）
  `cargo test --workspace --locked` → **523 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：错误只以 toast 呈现，详情页没有"上次失败原因"的常驻位置

### L9 崩溃钉住
- 状态：**完成**
- 最近提交：`feat: pin the tail of a crash`
- 实际修改文件：新增 `ui/src/state/crash_pin.rs`；更新 `ui/src/state/{mod,store}.rs`、
  `ui/src/views/shell.rs`（事件里钉住、详情页崩溃卡片）
- 为什么是"快照"而不是"接着看实时日志"：缓冲对已退出任务有 TTL 会过期，而用户可能几分钟后
  才回到窗口。钉住的是**崩溃那一刻的尾部**；如果它继续更新，那就不是现场记录，只是换了标题的
  当前日志
- 内容：
  1. 崩溃事件到达时冻结最后 **200 行**（`terminal_text_of`）
  2. 详情页顶部卡片：标题含退出码、尾部输出、"重试"（`StartTask`）、"忽略"
  3. **端口冲突识别**：尾部出现 `EADDRINUSE` / `address already in use` 时指出端口
     （优先用应用的 `port_hint`，否则从文本里找），并给出下一步（停掉占用进程或改端口）
  4. 同一个任务重复崩溃只保留一张卡（避免堆叠）；最多保留 5 张
- 验证：`cargo test -p neohub-ui --lib crash_pin` → **9 passed**
  （只保留尾部且最新在末尾、短日志完整保留、空日志不报错、标题含退出码、端口冲突两种拼写、
  无关崩溃不误报端口、只有崩溃才钉住、limit 0）
  `cargo test --workspace --locked` → **532 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：崩溃卡片的实际渲染需人工确认（无法自动化视觉）

### S8 就绪通知
- 状态：**完成**（崩溃 + 就绪两类通知）
- 最近提交：`feat: notify when a task becomes ready`
- 实际修改文件：`core/src/events.rs`（新事件）、`core/src/service.rs`（`set_ready` 发事件、
  `app_of_task`、测试钩子）、`platform-macos/src/notify.rs`（`for_task_ready`）、
  `ui/src/state/store.rs`（处理事件）、`ui/src/views/shell.rs`（按开关发送）
- 关键设计：**只在 ready 首次变 true 时发事件**。探测是周期性运行的，若每次成功都发事件，
  事件流和通知中心都会被刷屏 —— 测试直接断言"同一次运行只发一次"
- 就绪通知默认**关闭**（`notify_on_ready` 默认 false，与旧版一致）：它只在窗口不在前台时才有意义，
  而大多数时候用户正看着那个任务
- 验证：`cargo test -p neohub-platform-macos --lib notify` → **7 passed**（含设置关闭时不通知）
  `cargo test -p neohub-core --test log_focus readiness` → **1 passed**（一次运行只发一次事件）
  `cargo test --workspace --locked` → **534 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：通知实际弹出需人工确认（自动化不能看横幅）

### S10 忽略此版本
- 状态：**完成**
- 最近提交：`feat: let the user skip an update version`
- 实际修改文件：`domain/src/settings.rs`（`skipped_update_version: Option<String>` + patch 的
  `Option<Option<String>>`）、`storage/src/settings.rs`（读写该键）、`core/src/update.rs`
  （`respect_skip`）、`ui/src/views/shell.rs`（应用跳过 + 「忽略 X」按钮）
- 语义要点：
  1. **跳过一个版本 ≠ 关掉更新检查**：跳过 1.5.0 之后 1.6.0 仍然会提示（有测试）
  2. 跳过值存**归一化后的版本号**，`v1.5.0` 与 `1.5.0` 视为同一个
  3. 无法解析的跳过值**不会**静默隐藏更新（有测试）
  4. `Option<Option<String>>` 让"清除跳过"与"没提到该字段"区分开（设置表单保存时不碰它）
- 顺带修的结构问题：`write_settings` 原本是固定长度数组，加键时改成了 `Vec`
- 验证：`cargo test -p neohub-domain --lib settings` → **6 passed**（含跳过/保留/清除三级语义）
  `cargo test -p neohub-core --lib update` → **9 passed**（含跳过不再提示、更新版本仍提示、坏值被忽略）
  `cargo test --workspace --locked` → **538 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：「忽略 X」按钮的实际点击需人工确认

### N1 Node 版本选择接入配置面板
- 状态：**完成**
- 最近提交：`feat: offer installed node versions in the config panel`
- 实际修改文件：`ui/src/views/shell.rs`（`load_node_versions`、版本列表通道、版本字段下的可选 chips）
- 内容：
  1. 打开「应用配置」时加载 `ListNodeVersions`（与 MCP 同一个命令）
  2. 在 Node 版本字段下方列出本机已装版本（`20.11.0 (nvm)` 形式），点击填入**裸版本号**
  3. 仍可手输 `auto` / `system` / 任意版本 —— 下拉是便利，不是限制
- 实现注意：drain 方法在 `HostBridge` 上、读列表的是 `ShellView`，所以列表经 `last_node_versions`
  共享通道传递（与 `last_update` 同一模式），而不是硬塞进视图
- 验证：`cargo test --workspace --locked` → **538 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：版本 chips 的实际点击需人工确认；本机只有 system node（`~/.local/bin/node`），
  所以列表里只会出现一个来源

### P4 PTY resize 接线
- 状态：**完成**
- 最近提交：`feat: keep the pty in step with the window`
- 实际修改文件：新增 `ui/src/state/pty_size.rs`；更新 `ui/src/state/mod.rs`、
  `ui/src/views/shell.rs`（render 读取窗口宽度、尺寸变化时发 `ResizeTask`）
- 为什么之前是"部分"：PTY 启动在 120×30 之后**再没变过** —— 窗口拉宽后终端仍在原宽度换行
- 实现：
  1. 纯函数 `pty_size(window_width, font_size, rows)`：扣掉侧栏与内边距，按 0.6 em 字宽估算列数
  2. 结果**双向夹紧**（cols 20–400、rows 4–200）：估算偏差无害，但荒谬值会让游标定位的
     转义序列错乱
  3. 只有**尺寸变化时**才发命令（每帧一个 ioctl 是纯噪音），按任务记录上次发送值
- 验证：`cargo test -p neohub-ui --lib pty_size` → **7 passed**
  （宽窗列数更多、扣掉侧栏与内边距、极小窗夹到最小值、极大窗夹到最大值、NaN/0/∞ 回退最小值、
  rows 双向夹紧、1280×12px 时列数落在 100–140 与初始 120×30 同一量级）
  `cargo test --workspace --locked` → **545 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：真实拖拽窗口后终端重排的观感需人工确认；字符宽度是估算（0.6 em），
  若换非等宽字体会偏

### U4 导入面板的批次默认值
- 状态：**完成**
- 最近提交：`feat: choose node and package manager for an import batch`
- 实际修改文件：`ui/src/state/store.rs`（`ImportState.node_version`/`package_manager` + setter）、
  `ui/src/views/shell.rs`（选择行、`import_selected` 应用）
- 内容：
  1. 面板顶部两行选择：包管理器（自动/npm/pnpm/yarn/bun）、Node（自动/system/本机已装）
  2. 选择**只作用于本批导入**：重新扫描保留选择（那是关于"怎么导入"，不是关于目录），
     关闭面板则清空
  3. 未选择时**不写死**：`node_version_pref = None`（即 auto），包管理器用探测结果 —— 不把
     "今天这台机器恰好有的版本"固化进应用配置
  4. 显式版本会去掉 `v` 前缀后写入 `NodeVersionPref::Explicit`
- 验证：`cargo test -p neohub-ui --lib import_defaults` → **2 passed**
  （默认空、可设置与清除、重新扫描保留、关闭面板清空）
  `cargo test --workspace --locked` → **547 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：选择行的实际点击需人工确认

### U16 焦点陷阱（Shift-Tab 与焦点语义）
- 状态：**完成**
- 最近提交：`feat: walk the form fields backwards with shift-tab`
- 实际修改文件：`ui/src/state/app_form.rs`（`shift-tab` 反向、测试）、
  `ui/src/views/shell.rs`（把 Shift+Tab 翻译成表单认识的反向键）
- 澄清一处此前的模糊表述：这个 UI 是**自绘**的，只有一个 `FocusHandle`，字段焦点由表单自己
  管理（单缓冲 + 索引）。因此**不存在"焦点逃出抽屉"的系统焦点链问题** —— 旧版 WebView 里需要
  焦点陷阱，正是因为浏览器有真实的 tab 链。真正缺的只是反向切换
- 内容：
  1. `Tab` 前进、`Shift-Tab` 后退，**双向都环绕**，切换时把目标字段当前值载入缓冲（不丢改动）
  2. `Esc` 关闭面板（已有）；视图层把 `tab + shift` 翻译成 `"shift-tab"`，模型不依赖 modifiers
- 验证：`cargo test -p neohub-ui --lib app_form` → **15 passed**
  （新增：Shift-Tab 从第一项绕到最后一项、反向取值正确、再按 Tab 回到原位）
  `cargo test --workspace --locked` → **548 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：按键实际效果需人工确认

### L7 等宽字体
- 状态：**完成**
- 最近提交：`feat: use a monospace face for the terminal panes`
- 实际修改文件：`ui/src/theme.rs`（`MONO_FAMILY` / `MONO_FAMILY_FALLBACK` + 测试）、
  `ui/src/views/shell.rs`（终端与日志面板 `font_family`）
- 为什么值得单独做：**终端网格在比例字体里是没有意义的** —— ANSI 列与光标定位只有在每个字形
  等宽时才对得上。之前终端用的是界面默认字体，输出对齐是碰运气
- 选择：`SF Mono`（随 macOS 提供），回退 `Menlo`（10.6 起就有）。本机实测只有 `Menlo.ttc`，
  所以实际走回退路径 —— 这也说明回退不是装饰
- 验证：`cargo test -p neohub-ui --lib font_tests` → **2 passed**
  （等宽族已声明、回退族与主族不同、回退是 Menlo）
  `cargo test --workspace --locked` → **550 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：字体渲染的观感需人工确认；若用户系统两者都没有，会退回系统默认（比例字体），
  此时列对齐仍会偏 —— GPUI 侧暂无字体回退链的显式 API

### L3 日志推送分块上限（核实 + 锁定）
- 状态：**完成**（实现早已满足，本轮补上核实与防回归）
- 最近提交：`test: pin the log push chunk limit`
- **核实结果**：`CHUNK_LIMIT = 10 KiB`，`LogStore::append` 按它切块，事件泵逐块发送 —— 已经
  低于旧版 ~16 KiB 的上限。之前标"部分"是表过期，不是实现缺失
- 本轮补的：
  1. `the_push_chunk_limit_stays_under_the_legacy_cap`：常量若被调大就报错
  2. `one_big_write_becomes_several_small_events`：64 KiB 一次写入 → ≥4 条事件、每条 ≤16 KiB、
     seq 严格递增
  3. `the_split_keeps_every_byte`：切分后拼回去与原文**逐字节相同**（顺序也一致）
- 验证：`cargo test -p neohub-core --lib push_chunking` → **3 passed**
  `cargo test --workspace --locked` → **553 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 备注：clippy 提示"常量断言恒为真"，改成对运行时副本断言（保留了检查意图，也不引入
  `const` 断言的编译期噪音）

### 交付收尾：文档对齐 + 最终验证
- 状态：**完成**
- 最近提交：`docs: bring the delivery report up to date`
- 做了什么：
  1. `DELIVERY.md` 数字全部按实测重写：**553 测试 / 0 clippy / 82 提交 / 125 文件 / 3.86 万行**
  2. 功能对照表结论改为 **84 已验证 / 4 部分 / 1 不适用 / 5 说明**
  3. 剩下的四项各写清"为什么留着 + 需要什么前提"，而不是含糊地留在"部分"
  4. 补上"本轮之前的验证快照"与"建议的下一步"（真实数据迁移 → 人眼确认 → 之后才谈
     L6/U13/S6/B6）
- 最终验证（全部实跑）：
  - `cargo test --workspace --locked` → **553 passed / 0 failed**
  - `cargo clippy --workspace --all-targets --locked` → **0 warnings**
  - `bash scripts/package.sh` → 31 MB app / 13 MB dmg，**打包自检通过**（产物不落后于源码）
  - 启动（临时 `--data-dir`）→ 进程存活、窗口 onscreen、无 error/panic
- 依据你的三个决定保持：**不合入 main**、**真实数据由你按 MIGRATION.md 迁移**、
  **⌘K 优先**（已完成）

### U13 渲染守卫（ErrorBoundary 的可行形态）
- 状态：**完成**（按"先验证再动手"的方式落地，而不是照抄 WebView 的 ErrorBoundary）
- 最近提交：`feat: keep one bad render from taking the window down`
- **先做的判断**：WebView 里的 ErrorBoundary 是在组件树里捕获异常渲染；GPUI 里做到等价物需要
  包住框架自己的绘制帧，那不是我们能安全 unwind 的地方。所以只包**喂给视图的计算**
  ——这部分是我们的代码、返回普通数据，`catch_unwind` 在这里是可靠的
- 实现：`ui/src/state/render_guard.rs` 的 `guard(what, f) -> Option<T>`：捕获 panic、
  写入 `~/Library/Logs/NeoHub/neohub.log`（`[ui] render guard caught a panic in …`）、
  返回 `None` 让该区域渲染为空
- 已套用的两处（最容易因外部数据出问题）：
  1. 仪表盘卡片（`dashboard_cards`）
  2. 终端行（`visible`，来自转义序列状态）
- **明确不做的**：不包裹 GPUI 的绘制帧 —— 那里 unwind 不是我们该管的，硬包可能把框架状态留在
  半更新状态，比崩溃更难查。这条写进了模块文档
- 验证：`cargo test -p neohub-ui --lib render_guard` → **4 passed**
  （正常计算透传、panic 变成 None 且调用方存活、`&str` 与 `String` 两种 payload 都能读、
  一次被捕获的 panic 不影响下一次调用）
  `cargo test --workspace --locked` → **557 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：真正的 GPUI 绘制期 panic 仍会结束进程（这是有意的选择）；渲染守卫的日志路径
  需在真出问题时才会被验证

### L6 日志虚拟化与高亮
- 状态：**完成**
- 最近提交：`feat: highlight the search term inside the terminal`
- **核实**：虚拟化早已成立 —— `TerminalView::visible(height, follow)` 只返回窗口高度那么多行
  （16 行），渲染量不随缓冲增长。之前标"部分"是因为缺高亮
- 本轮补的高亮：
  1. `highlight_ranges(text, needle)`：**按字符**定位（不是字节），大小写不敏感；
     字节偏移会在多字节字符中间切开并 panic，而带中文/emoji 的日志是常态
  2. `split_run_for_highlight(run, offset, ranges)`：把一个带颜色的 run 按匹配切成片段，
     `offset` 让整行的匹配范围能套到每个 run 上
  3. 终端面板在搜索面板打开且有查询时，把匹配片段用 `warn` 色底着色
- 边界处理（都有测试）：空 needle 不高亮、needle 比行长、**重叠命中不重复绘制**、
  小写化后长度可能变化时退回大小写敏感扫描而不是错位高亮
- 验证：`cargo test -p neohub-ui --lib highlight` → **9 passed**
  `cargo test --workspace --locked` → **566 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：高亮的实际观感需人工确认；高亮只跟随全局搜索词，详情页尚无独立搜索框

### B6 `enc1:` 密钥重录
- 状态：**完成**（重录路径可用；自动解密仍需要旧版配合，但不再阻塞用户）
- 最近提交：`feat: flag legacy ciphertext so it can be re-entered`
- 为什么这样收口：`enc1:` 只有旧版进程能解。与其等一个需要旧版改动的自动导出，不如把
  **用户本来就必须做的那一步**做顺：在环境变量面板里明确指出哪一行需要重录，并允许就地编辑
  （行内编辑上一轮已完成）。改完即写成 `nhk1:` 新密文
- 实现：
  1. `EnvVarView.needs_reentry`：core 只看前缀是否为 `enc1:`，**密文本身仍不出 core**
  2. `EnvRow.needs_reentry` 传递到面板，行上显示 `需重录`（`warn` 色）
  3. 用户就地改值 → `SetEnvVars` 用新密钥重新加密
- 验证：`cargo test -p neohub-core --test core_service a_legacy…` → **1 passed**
  （`enc1:` 行被标记、其文本仍为 `None`；普通行不受影响）
  `cargo test --workspace --locked` → **567 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic
- 未解决问题：没有"自动批量重录"（需要旧版导出明文）；启动流程对 `enc1:` 的拦截仍会让任务
  起不来（这是正确的，错误已能说明原因并可引导重录）

### S6 深链接 Apple Event：探针结论（本轮未实现，已止损）
- 状态：**保持"部分"**，但阻塞点已查清并记录
- 本轮做了什么：查了 `objc2` 生态能提供什么，而不是凭猜测改代码
- **确切结论**：
  - `objc2-foundation 0.3` 有 `NSAppleEventManager` 与 `NSAppleEventDescriptor`，但
    `setEventHandler:andSelector:forEventClass:andEventID:` **只在启用 `objc2-core-services`
    时存在**（它需要 `AEEventClass` / `AEEventID` / `kAEGetURL` / `AEKeyword` 这些类型）
  - 因此要做成，需要同时具备：`objc2-core-services` 依赖 + `objc2::define_class!` 定义一个
    NSObject 子类并实现 `handleGetURLEvent:withReplyEvent:` + 从 `AEKeyword` 取
    `keyDirectObject` 再读 `stringValue`
  - 这三件事都是 unsafe ObjC 交互，且**失败形态是"点击链接后崩溃"**，只能在真机上验证
- 为什么本轮停手：`Cargo.toml` 里加了 `NSAppleEventManager` feature 之后我判断，在剩余精力内
  完成这套绑定的把握不足；更糟的结果是交付一个"看起来能用、点了会崩"的功能。所以回滚了那处
  feature 改动，工作区保持干净，并把上面的路径写清楚
- 现状（未变）：`Info.plist` 已注册 `neohub` scheme，`deep_link::parse` 已完整实现并有 10 个
  测试，argv 传入的链接会被解析并选中项目；**只有"运行中通过 Apple Event 送进来"这一环缺失**
- 接手方式：按上面三步做，先写一个只打印 URL 的探针（`#[ignore]` 测试或示例程序），
  确认不崩再接入 `ActivationWatcher` 那条轮询链路

### S6 深链接 Apple Event（完成 —— 上轮的"止损"被推翻了）
- 状态：**完成**
- 最近提交：`feat: receive neohub:// links through an Apple Event handler`
- 上轮我判断"把握不足"并回滚；本轮按自己写下的三步做完了，**结论是不难，只是需要查对 API**
- 实际需要的三件事（上轮的推测全部被证实，另外多查出一处）：
  1. `objc2-core-services` 依赖，且要开 `AE` feature（`AEEventClass`/`AEEventID`/`keyDirectObject` 在这里）
  2. `objc2-foundation` 要开 `NSAppleEventManager`、`NSAppleEventDescriptor`、**`objc2-core-services`**
     三个 feature —— 最后一个才是 `setEventHandler:…` 存在的前提，容易漏
  3. `kAEGetURL` / `kInternetEventClass` **不在生成的绑定里**（它们是 C 宏），需要自己写
     FourCharCode：两者都是 `'GURL'`
- 实现：
  1. `define_class!` 定义 `NSObject` 子类 `UrlEventHandler`，实现 `handleGetURLEvent:withReplyEvent:`，
     从 `keyDirectObject` 读 `stringValue` 存入全局队列
  2. `install()` 注册到 `sharedAppleEventManager`，**返回 Retained 对象**（丢了会留悬挂 target，
     正是这类绑定的经典崩溃点）
  3. `desktop` 在主线程注册并在轮询 tick 里 `drain()`，逐个 `deep_link::parse` → 选中项目 + 带窗口到前台
- 验证：`cargo test -p neohub-platform-macos --lib deep_link_event` → **3 passed**
  （handler 类能在运行时分配、drain 交出并清空、**真实注册 `install()` 不崩**）
  `cargo test --workspace --locked` → **570 passed / 0 failed**；clippy 0；打包自检通过；
  启动日志无 error/panic、无 handler 注册失败
- 未解决问题：**"浏览器点击链接"这一步只能人工确认** —— 我验证的是注册不崩、接收逻辑正确；
  系统真的回调我们，需要在真机上点一次

### 最终交付验证（0 部分状态）
- 状态：**完成**
- 最近提交：`docs: final delivery verification`
- 本轮做的：
  1. `DELIVERY.md` 结论改为 **88 已验证 / 0 部分 / 1 不适用 / 5 说明**，并列出最后关闭的四项
     （`L6`/`U13`/`B6`/`S6`）各自的结论
  2. 在仓库根的 `specs/PROGRESS.md` 补上 Rust 版条目 —— 计划要求两处进度文件都更新，此前只写了
     `specs/rust-migration/PROGRESS.md`
  3. 全部数字按实测重写：570 测试、0 clippy、88 提交
- 最终验证（实跑）：
  - `cargo test --workspace --locked` → **570 passed / 0 failed**
  - `cargo clippy --workspace --all-targets --locked` → **0 warnings**
  - `bash scripts/package.sh` → 31 MB app / 13 MB dmg，**产物不落后于源码**自检通过
  - 启动（临时 `--data-dir`）→ 进程存活、无 error/panic、无 URL handler 注册失败
- 仍需要人做的事（不是我能替代的）：
  1. **真实数据迁移** —— 按 `native/docs/MIGRATION.md` 的"预览 → 确认 → 导入"
  2. **人眼确认** —— 菜单栏图标与点击、⌘K、行内编辑、通知横幅、拖拽窗口后的终端重排、
     浏览器点击 `neohub://`
  3. 决定是否合入 `main`、是否替换稳定版（当前按你的决定保持分支隔离）

### 真实数据迁移：CLI + 执行（并修掉一个真实 bug）
- 状态：**完成**（已对真实旧库执行，旧库字节未变）
- 最近提交：`feat: migrate legacy data with one command`
- 为什么需要新代码：此前只有库内 API，没有可执行入口，用户/我都没法按 `MIGRATION.md` 跑。
  新增 `neohub-desktop --migrate-legacy [--dry-run] [--data-dir] [--legacy-dir]`，
  在 GPUI 启动前分流、打印纯文报告后退出 —— 这样 SSH 里也能用，且不会被窗口半途打断
- **执行中发现并修掉一个真实 bug**：`import_legacy` 的内部发布目录原本用 `.import-staging`，
  与调用方（CLI）复制源库所用的目录**同名**。于是发布步骤先把调用者的副本删掉，再去读它 →
  `unable to open database file`。改为 `.publish-staging`，并加了防回归测试
  （测试形状与 CLI 完全一致：复制到 `<target>/.import-staging` → inspect → import）
- 真实迁移结果（`~/Library/Application Support/DevHarbor` → `…/NeoHub-Rust-Dev`）：
  - 预览：7 个 schema 版本全部已知；**4 apps / 4 tasks / 0 env vars / 9 run history / 7 settings**；
    源库有 WAL；`mcp_token` 属未知设置（留在旧库，未应用）
  - 导入：`应用 4 · 任务 4 · 环境变量 0 · 历史 9 · 跳过 0 · 未解密 0`
  - `PRAGMA integrity_check` → `ok`；`auto_start=0`、`mcp_enabled=0`（**按设计暂停自动化**）
  - **旧库 sha256 仍是 `eadcd3d9…09cbab`**，与冷备份一致 —— 源库只读这条守住了
  - 用导入后的数据启动打包应用：进程存活、**0 errors**
  - `migration.json` 记录了源 sha256、schema 版本与逐表计数
- 验证：`cargo test -p neohub-desktop --lib migrate` → **4 passed**（flag 解析、缺 `--data-dir` 报错、
  目标=源被拒）；`cargo test -p neohub-storage --test migration_atomicity` → **7 passed**（新增回归）
  `cargo test --workspace --locked` → **575 passed / 0 failed**；clippy 0
- 未解决问题：`enc1:` 密文本次为 0 条（旧库没有秘密变量），若有需按面板「需重录」处理

