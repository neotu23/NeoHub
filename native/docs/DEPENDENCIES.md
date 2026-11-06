# 依赖与构建环境（阶段 2 锁定）

> 记录时间：2026-11-06T12:47Z。**这里的版本是本次实际编译通过的组合**，
> 不是"候选清单"。任何升级都要重跑本文第 5 节的验证命令并更新本节。

## 1. 工具链

| 项目 | 值 | 说明 |
|---|---|---|
| Rust | **1.99.1** (58cf8f9c3 2026-10-13) | 固定在 `native/rust-toolchain.toml`（`channel = "1.99.1"`），`Cargo.toml` 的 `rust-version = "1.99"` |
| 为什么不是 1.97.0 | 本机 `stable` 是 1.97.0，编译 `gpui-pre-macos 0.3.11` 时报 `E0658: use of unstable library feature 'result_option_main_default'`（`window.rs` 用了 `Option/Result::map_or_default`）；1.99.1 已稳定该 API，编译通过 | 不用 nightly：nightly 会引入未声明的不稳定性 |
| Cargo | 1.99.1 | |
| Xcode | Xcode 27.1 (27A9275) | |
| **`DEVELOPER_DIR`** | **`/Applications/Xcode.app/Contents/Developer`** | 本机 `xcode-select -p` 指向 `/Library/Developer/CommandLineTools`，那里没有 `metal`。GPUI 走 Metal，构建必须导出 `DEVELOPER_DIR`（无需 sudo、不改系统设置）：`DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer cargo build …` |
| 目标平台 | `aarch64-apple-darwin` | 首版只验收 macOS Apple Silicon |
| 二进制最低系统 | **minos 11.0**（`otool -l target/release/neohub-desktop` 的 `LC_BUILD_VERSION`） | 与旧版 Electron 声明的 macOS 11 一致，**不需要提高系统要求**。注意：未在真实 macOS 11 机器上运行验证过，只核对了 load command |
| 链接的系统框架 | AppKit / Foundation / CoreServices / ApplicationServices / CoreVideo / Carbon / CoreText / UserNotifications / libobjc | 全部系统自带，没有随包分发的运行时 |

## 2. 直接依赖

| 包 | 版本 | 许可证 | 用途 | 备注 |
|---|---|---|---|---|
| `gpui-kit` | `=0.9.0` | Apache-2.0 | 唯一 UI 依赖：重导出 GPUI + `gpui-base` + `gpui-component` + `gpui-kit-assets` | 只允许通过它使用 GPUI。**不允许**再直接添加 `gpui`/`gpui-pre`，否则二进制里会出现两份不兼容的 GPUI 类型 |
| `neohub-ui` | path | AGPL-3.0-only | 本仓库 UI crate | 通过 workspace 依赖 |

`gpui-kit 0.9.0` 的传递依赖里与本项目直接相关的：

| 包 | 版本 | 许可证 | 说明 |
|---|---|---|---|
| `gpui-pre`（库名仍是 `gpui`） | 0.3.11 | Apache-2.0 | GPUI 本体。Zed 的正规 `gpui` crate 发布不规律，Kit 钉住的快照以 `gpui-pre` 名义发布；`use gpui::*` 语义不变 |
| `gpui-pre-platform` / `gpui-pre-macos` / `gpui-pre-apple` | 0.3.11 | Apache-2.0 | macOS 平台层（Metal 渲染、AppKit 桥、AccessKit） |
| `gpui-component` | 0.9.0 | Apache-2.0 | 组件库（`gpui_component`），`gpui-kit` 默认开启 |
| `gpui-base` | 0.9.0 | Apache-2.0 | 行为/状态层 |
| `gpui-kit-assets` | 0.9.0 | Apache-2.0 | 图标资源，`gpui-kit` 默认开启 |
| `tokio` | 1.54.1 | MIT | 异步运行时（后续核心显式持有 Runtime） |
| `wgpu` | 30.0.1 | MIT/Apache-2.0 | Kit 的 GPU 抽象层之一（非 macOS 主路径） |
| `cosmic-text` | 0.19.0 | MIT/Apache-2.0 | 文本整形 —— 中文/emoji 渲染依赖它 |
| `accesskit` | 0.24.1 | MIT/Apache-2.0 | 无障碍树 |
| `objc2` | 0.5.3 **与** 0.6.5 | MIT | 上游同时存在两个大版本（见 §3），不做 patch |
| `rust-embed` | 8.13.0 | MIT | 资源内嵌 |

GPUI 系的许可证全是 **Apache-2.0**，与项目自身的 AGPL-3.0-only 兼容（Apache-2.0 → GPLv3 → AGPLv3
的单向兼容）。发布时必须保留上游版权/NOTICE 与 `LICENSE` 中的原始声明；技术重写不消除
原有声明（阶段 17 一并核对）。

## 3. 重复依赖（`cargo tree -d --depth 0`，实测 71 组）

不盲目 patch，先记录：重复项全部来自上游传递依赖，锁定它们的唯一方式是 fork/patch 上游，
计划明确要求"用 `cargo tree -d` 分析重复项，不盲目 patch"。影响面最大的几组：

`objc2 0.5.3 / 0.6.5`、`objc2-app-kit 0.2.2 / 0.3.2`、`objc2-foundation 0.2.2 / 0.3.2`、
`base64 0.22.1 / 0.23.1`、`bitflags 1.3.2 / 2.13.2`、`dirs 5.0.1 / 6.0.0`、
`png 0.17.16 / 0.18.1`、`rand 0.8.8 / 0.9.5`、`syn 2.0.119 / 3.0.9`、`toml 0.8.23 / 1.2.0`。

这些重复只增加体积，不造成类型不兼容（同一 crate 的不同 major 在 Rust 里可以共存）。
阶段 16 做体积优化时再评估是否有值得上移的项。

依赖总包数：`Cargo.lock` 800 个包（含各 target 的条件依赖；实际 arm64 macOS 参与编译的少于此数）。

## 4. Cargo 配置与产物

- `native/Cargo.toml`：workspace，`resolver = "3"`，release profile 用 `lto = "thin"`、
  `strip = "debuginfo"`、`codegen-units = 16`。
- 所有注册表依赖都用明确版本（`=0.9.0`），**没有** `version = "*"`；`Cargo.lock` 已提交。
- 尚无 git 依赖（阶段 3+ 若引入必须钉 `rev` 并记录完整 SHA）。
- 实测产物（本机 arm64）：`target/debug/neohub-desktop` 99 MB（未 strip、含调试信息）、
  `target/release/neohub-desktop` **19 MB**（strip 后）。不含 `.app` 包装、图标与语言资源。
  与旧版 `NeoHub.app` 265 MB / dmg 104 MB 的正式对比在阶段 16 做，这里不作结论。

## 5. 本阶段验证命令与结果（2026-11-06T12:47Z）

```bash
cd /Users/neil/Code/NeoHub-Rust/native
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
cargo check --workspace --locked      # exit 0, Finished dev profile in 28.64s
cargo build -p neohub-desktop --release --locked   # exit 0, Finished release in 1m 25s
cargo tree -d                          # 71 组重复，见 §3
cargo test -p neohub-ui --locked       # 2 passed（主题调色板/模式解析）
cargo run -p neohub-desktop --release  # 见 §6 的窗口验证
```

## 6. 实机窗口验证（阶段 2 验收）

- `target/debug/neohub-desktop` 启动后进程稳定驻留、无 stderr 输出；
- `CGWindowListCopyWindowInfo`（`.optionAll`）实测到一个 `owner=neohub-desktop`、
  `layer=0`（普通窗口层）的窗口，`bounds = 1280 × 853`（1280×820 内容 + 33pt 标题栏），
  位置 `(160, 103)`，与 `WindowOptions.window_bounds` 一致；`window_min_size` 为 960×600。
- **未完成**：像素级/交互级验证。本会话 `screencapture` 失败（无屏幕录制权限），
  `osascript` 访问窗口属性被拒（无辅助功能权限，`-25211`），GPUI 的
  `Window::render_to_image` 只在 `test-support` feature 下提供且 macOS 平台未实现。
  因此"中文输入法组合、选择、剪贴板、Tab 焦点、暗色主题"的人工确认需要在用户的桌面会话里
  打开该窗口执行，或用阶段 9 之后的无头视觉测试（`VisualTestContext`）在 CI 里断言。
  这条限制不隐瞒，记为阶段 9/16 的待补项。
