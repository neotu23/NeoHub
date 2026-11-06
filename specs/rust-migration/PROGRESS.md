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
- 最近提交：`docs: define Rust GPUI migration baseline`（见下条目的实际 SHA）
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

---
