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
- 状态：**开发中**（22 工具 + dispatch + gate + 脱敏 + HTTP 端点 + 变更广播 + 读写工具参数/形状基本对齐；`get_logs`/`get_ports`/`add_app` 与 TLS/SSE 待对齐）
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
- 仍存在的兼容差异（需继续对齐 `tools.ts`）：
  1. `get_app` 尚未返回 `.env` 发现结果与 `recentRuns`（run history）、`runs_limit` 参数
  2. `get_logs` 仍是 `{task_id, lines}` + 单段文本；旧版是 `{app, task?, lines}` + 分任务段落 + `{app, linesPerTask, tasks}`
  3. `get_ports` 仍是 `{task_id, confirmed, hinted}` 数组；旧版是 `{apps:[{app, ports, portDetails}]}` 或按 `port` 查所有者
  4. `add_app` 参数集未对齐（旧版 `path/name/script/command/workspaceTasks/autoStart`）；`workspaceTasks`（monorepo 一包一任务）尚未实现
  5. `set_readiness` 的旧版参数集未核对
  6. 错误消息文案未逐条对齐旧版（只在字段层面一致）



