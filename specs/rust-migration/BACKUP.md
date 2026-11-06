# 备份记录（阶段 1）

> 采集时间：2026-11-06T12:34Z（UTC）。
> **本文只记录脱敏路径、哈希、时间与恢复方法。** 数据库、Chromium 存储、bundle、
> 加密/明文迁移包与截图以外的一切本机数据都不进 Git。报告里不出现任何变量值、Token、
> 项目名或完整项目路径。

## 1. Git 备份（1A）

| 项目 | 值 |
|---|---|
| 备份提交 | `58f88defd9df25e3b9c2556f2c20e20472d36ae8` |
| 该提交内容 | `02acedb`（1.4.0 稳定版）+ 阶段 0 文档提交 `66d8519` + 忽略规则提交 `58f88de` |
| 旧版分支 | `legacy/electron` → `58f88de…` |
| 不可移动标签 | annotated tag `electron-final-20261106`（tag 对象 `cf790a6b4701358f6f8ea5ef59a42107e31132c3`，解引用提交 `58f88de…`） |
| 推送 | `git push --atomic origin refs/heads/legacy/electron refs/tags/electron-final-20261106`，exit 0，两个引用均为 new |
| 远端核对 | `git ls-remote origin refs/heads/legacy/electron 'refs/tags/electron-final-20261106*'`：branch = `58f88de…`，tag 对象 = `cf790a6…`，`^{}` = `58f88de…`（与备份提交一致，未误把 tag 对象 SHA 当成提交 SHA） |
| `main` | **未被覆盖、未被 force push**；远端 `main` 仍是 `0cd4a2e`，本地 `main` 在其基础上多了阶段 0/1 的三个提交 |

离线 bundle：

| 项目 | 值 |
|---|---|
| 路径 | `~/Backups/NeoHub-Migration-20261106/neohub-electron-20261106.bundle`（仓外、非 `/tmp`） |
| 大小 | 7.4 MB |
| SHA-256 | `d11ce859b3c8f6edebe88f198b252a1d8dd4f9d8ca3687a6389064461f4e16a1` |
| `git bundle verify` | 通过：9 个 refs，含 `refs/heads/{main,legacy/electron}`、`refs/tags/{electron-final-20261106,v1.3.0,v1.4.0}`，输出 `The bundle records a complete history.` |
| 恢复演练 | 在 `/tmp` 临时目录 `git clone --no-local` bundle 后：`git rev-parse electron-final-20261106^{commit}` = `58f88de…`、`HEAD` = `58f88de…`，`git fsck` 无输出（干净）；验证后已删除临时目录 |

> bundle 只含 Git 追踪对象，**不是**用户数据库或忽略文件的备份。

## 2. 用户数据备份（1B）

- 数据目录确认方式：按 `src/main/services/AppIdentity.ts` 的三个历史候选
  （`NeoHub` → `DevHarbor` → `devharbor`，判据是目录内存在 `devharbor.db`）逐个检查，
  实际命中 `~/Library/Application Support/devharbor`；**没有**只按名字猜。
- 采集前置条件：`pgrep -fl "NeoHub|DevHarbor"` 无匹配（旧版完全退出，不是"关窗留菜单栏"）；
  源目录内未发现 `-wal` / `-shm` 文件，说明上次是干净关闭，冷复制即可（未对活动数据库裸拷）。
- 备份位置：`~/Backups/NeoHub-Migration-20261106/userdata-devharbor-20261106/`
  （`chmod 700`，用 `cp -a` 保留时间戳与权限，含数据库、Chromium 存储、`window-state.json`
  等全部内容，共 13 MB）。

副本校验（**只对副本执行**，源目录只读）：

| 检查 | 结果 |
|---|---|
| `PRAGMA integrity_check` | `ok` |
| `PRAGMA foreign_key_check` | 无输出（无违反） |
| `PRAGMA journal_mode` | `wal` |
| `_schema_migrations` | `0001_init`、`0002_tasks`、`0003_run_history_tasks`、`0004_env_task_scope`、`0005_app_folders`、`0006_env_scope_unique`、`0007_app_autostart` 共 7 条，版本名不含 `.sql` |
| 行数 | `apps` 4、`tasks` 4、`env_vars` 0、`run_history` 9、`settings` 7 |
| `devharbor.db` SHA-256 | `eadcd3d941210e0f128f443da9cb205fe126a9bee7cd30d8f57f77238009cbab` |

**外部依赖**：`env_vars` 目前为 0 行，因此本副本不含密文字段；但旧版用 Electron
`safeStorage`（macOS Keychain）加密 secret 与 MCP Token —— 复制文件**不能**保证换机后
仍可解密。阶段 13 处理这条限制：同身份同 Keychain 环境优先验证兼容，否则走旧版本地
opt-in 导出入口，不把 `enc1:` 原文当密码用。

## 3. Worktree（1C）

| 项目 | 值 |
|---|---|
| 目录 | `/Users/neil/Code/NeoHub-Rust`（计划首选：仓库相邻、用户可写） |
| 分支 | `rewrite/rust-gpui` |
| 起点 | 备份提交 `58f88de…` |
| 原始目录 | 停留在 `main`，不自动 push `main`，不删除/覆盖 GitHub 原引用 |

## 4. 恢复方法

```bash
# A. 从远端恢复旧版（引用永久保留，不移动）
git clone --branch legacy/electron ssh://git@ssh.github.com:443/neotu23/NeoHub.git neohub-legacy
git -C neohub-legacy rev-parse HEAD          # 期望 58f88defd9df25e3b9c2556f2c20e20472d36ae8

# B. 从离线 bundle 恢复（网络不可用时）
git clone ~/Backups/NeoHub-Migration-20261106/neohub-electron-20261106.bundle neohub-legacy
git -C neohub-legacy rev-parse electron-final-20261106^{commit}

# C. 恢复旧版用户数据：先完全退出任何 NeoHub/Rust 进程，再把备份目录复制回去
#    恢复前把当前目录另存，保留证据，不覆盖唯一副本。
```

## 5. 本阶段没做的事（不当作已完成）

- 没有删除、移动或改写任何远端引用；没有 force push；没有新建 GitHub 仓库。
- 没有修改旧版 Electron 源码（`src/` 一动未动），也没有删除 `main` 上的旧构建文件。
- 没有导入任何用户数据到 Rust 侧（阶段 13 才有迁移入口）。
- 用户数据只做了**冷备份**，真实目录的内容与 mtime 未改动。
