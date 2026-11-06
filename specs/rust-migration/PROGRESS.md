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
