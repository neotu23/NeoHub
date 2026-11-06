# 构建与打包

## 前置条件

- macOS 13+（arm64；脚本只构建本机架构）
- Xcode（**必须**，不是 CLT）：GPUI 需要 Metal 工具链

  ```bash
  export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
  ```

- Rust：`native/rust-toolchain.toml` 锁定 **1.99.1**，`rustup` 会自动安装

## 日常构建

```bash
cd native
cargo build                      # 调试
cargo test --workspace --locked  # 全量测试
cargo run -p neohub-desktop      # 直接运行（默认数据目录 ~/Library/Application Support/NeoHub-Rust-Dev）
cargo run -p neohub-desktop -- --data-dir /tmp/neohub-scratch   # 隔离数据目录
```

## 打包 .app 与 .dmg

```bash
cd native
bash scripts/package.sh
```

产物：

| 路径 | 说明 |
|---|---|
| `native/dist/NeoHub.app` | 应用包（arm64，约 29 MB） |
| `native/dist/NeoHub-1.4.0.dmg` | 分发镜像（约 13 MB） |

脚本做的事：

1. `cargo build --release --locked -p neohub-desktop`
2. 组装 `NeoHub.app/Contents/{MacOS,Resources}`，复制 `crates/ui/locales/*.json`
   与 `build/icon.icns`（图标沿用 Electron 版的）
3. 生成 `Info.plist`：`CFBundleIdentifier = app.neohub.desktop`、版本 `1.4.0`、
   `LSMinimumSystemVersion = 13.0`、`LSApplicationCategoryType = developer-tools`
4. 签名（见下）并 `codesign --verify --strict`
5. `hdiutil` 打 UDZO 镜像（含 `/Applications` 快捷方式）

可用环境变量：`NEOHUB_VERSION`（覆盖版本号）、`NEOHUB_SIGN_IDENTITY`（指定证书）、
`SKIP_SIGN=1`、`SKIP_DMG=1`。

## 签名与分发

- 有 **Developer ID Application** 证书时脚本自动使用（`--options runtime --timestamp`，
  已为公证准备好 hardened runtime）。
- 没有证书时退回 **ad-hoc**（`codesign --sign -`）。ad-hoc 包**只本机可用**：
  拷到别的 Mac 会被 Gatekeeper 拦下（"应用已损坏或无法验证"）。
- 要真正分发，需要：Developer ID 证书 → `codesign` → `xcrun notarytool submit` → `xcrun stapler staple`。
  旧版用的 `build/notarize.cjs` 是同一流程的 Electron 版本，可对照参考。

## 已验证

| 检查 | 结果 |
|---|---|
| `bash scripts/package.sh` | 成功；ad-hoc 签名；`codesign --verify --strict` 通过 |
| `plutil -lint Info.plist` | 通过 |
| 架构 | `lipo -archs` → `arm64` |
| 通过 LaunchServices 启动（`open -a`） | `lsappinfo` 报告 `bundleID="app.neohub.desktop"`、`Version="1.4.0"`、`Arch=ARM64`、`type="Foreground"`；窗口 onscreen；数据库在指定 `--data-dir` 初始化 |
| `SIGTERM` | 正常退出（走 shutdown，不残留进程） |

**窗口尺寸未自动断言**：本机上 `CGWindowListCopyWindowInfo` 对所有应用都返回明显失真的
尺寸（例如微信 144×146），因此它不能用来断言窗口大小。样式与尺寸需要人眼确认；
`window-state.json` 会记录关闭时的真实 frame，可用来反查。
