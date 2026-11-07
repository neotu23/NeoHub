# 从 Electron 版迁移数据

本文件说明如何把旧版（Electron）的 NeoHub 数据导入 Rust 版。**没有自动执行**：所有步骤都要
先看预览、再明确确认。

## 已有的保护

| 风险 | 现在的处理 |
|---|---|
| 动了用户的真实数据库 | 源目录只读；先把库（含 `-wal`/`-shm`）复制到临时 staging 再操作；测试断言导入后源库**字节不变** |
| 导入中途崩溃留下半个库 | 首次导入写到 `<target>/.import-staging/`，校验 `integrity_check` 与 `foreign_key_check` 通过后，用**文件级 rename** 发布 |
| 重复导入把新编辑覆盖掉 | 已存在的 `id` 一律 `INSERT OR IGNORE`（跳过），并计入 `skipped`；设置用 `INSERT OR REPLACE`（单值，旧版意图应生效） |
| 自动启动/MCP 在验证期抢端口 | 导入后强制 `apps.auto_start = 0`、`settings.mcp_enabled = 0`、`launch_at_login = 0`，报告里标 `paused_automation = true` |
| 未知的 schema 版本 | `_schema_migrations` 里的未知版本记入 `unknown_versions`，不猜测、不丢弃 |
| 未知设置键 | 记入 `unknown_settings`，不写入新库（旧库里仍在） |
| 旧版密文 `enc1:` | 原样复制（不丢数据），并统计到 `secrets_unavailable`；这些变量在修好之前会**阻止任务启动**，不会当密码传下去 |

## 一条命令完成（推荐）

打包后的二进制自带迁移入口，**先 dry-run 看报告，再真跑**：

```bash
APP="$PWD/native/dist/NeoHub.app/Contents/MacOS/neohub-desktop"

# 1) 只读预览：什么都不写
"$APP" --migrate-legacy --dry-run --data-dir "$HOME/Library/Application Support/NeoHub-Rust-Dev"

# 2) 确认数量无误后导入
"$APP" --migrate-legacy --data-dir "$HOME/Library/Application Support/NeoHub-Rust-Dev"
```

`--legacy-dir` 可指定源目录（默认按 `NeoHub` → `DevHarbor` → `devharbor` 查找）。
`--dry-run` 会复制到临时 staging 后立刻删除，**目标目录一个字节都不写**。

## 手工步骤（等同上面的实现）

1. 完全退出旧版（关窗只到菜单栏不算退出）：

   ```bash
   pgrep -fl "NeoHub|DevHarbor"    # 必须为空
   ```

2. 确认旧数据目录（按 `NeoHub` → `DevHarbor` → `devharbor` 顺序找含 `devharbor.db` 的目录）：

   ```bash
   ls -d ~/Library/Application\ Support/{NeoHub,DevHarbor,devharbor} 2>/dev/null
   ```

3. 冷备份旧目录（本仓库阶段 1 已做过一份，见 `specs/rust-migration/BACKUP.md`）：

   ```bash
   cp -a ~/Library/Application\ Support/devharbor ~/Backups/neohub-legacy-$(date +%Y%m%d)
   sqlite3 ~/Backups/neohub-legacy-*/devharbor.db "PRAGMA integrity_check;"
   ```

4. 预览（只读，只出数量与名称）：

   ```rust
   let copied = copy_source(&legacy_dir, &staging)?;      // 复制，不动源
   let preview = inspect(&copied, &legacy_dir, had_wal)?; // 版本、行数、未知键、sha256
   ```

5. 确认预览后再导入（目标目录是 Rust 版自己的 `~/Library/Application Support/NeoHub-Rust`）：

   ```rust
   let outcome = import_legacy(&copied, &preview, &rust_dir)?;
   ```

6. 看报告：`MigrationReport::summary()` 只含数量；`secrets_unavailable > 0` 时先处理秘密，
   再启动任何项目。

## 密钥（`enc1:`）需要旧版进程配合

`safeStorage` 的密文只能用旧版（同身份、同 Keychain 环境）解开。可选路径：

- 在旧版里重新录入这些密钥（最省事）；
- 或在旧版源码上增加一个明确 opt-in 的本地导出入口（计划里预留的位置是
  `src/main/services/LegacyMigrationExport.ts`），由旧版进程把解密后的必要字段通过一次性本机
  IPC 或权限 `0600` 的临时文件交给 Rust 版，导入并校验后立即删除该文件。

**不要**把 `enc1:…` 当明文使用：Rust 版会拒绝它并阻止受影响任务启动。

## 回退

1. 退出 Rust 版，确认它的受管任务已停止。
2. 用 `legacy/electron` 分支或 `electron-final-20261106` 标签构建/运行旧版。
3. 旧版 userData 目录**未被本次迁移修改**，可直接继续使用；Rust 期间的新修改需要单独导出，
   不能拿新 schema 覆盖旧库。
4. 若旧目录被人为损坏，只从阶段 1 的冷备份恢复，且先把当前目录另存，保留证据。
