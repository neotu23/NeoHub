# 性能与体积对比（Electron 版 vs Rust 版）

测量时间：2026-11-07（同一台 Mac，Apple Silicon / arm64，macOS 26 环境变量 `DEVELOPER_DIR=Xcode`）。
所有 Rust 侧数字来自 `native/scripts/bench.sh`，5 次运行取中位数。

## 体积

| 项目 | Electron 版（1.4.0） | Rust 版（1.4.0） | 变化 |
|---|---|---|---|
| `.app` 包 | **265 MB** | **29 MB** | **−89%** |
| `.dmg` | **104 MB** | **13 MB** | **−87%** |
| 运行时依赖 | Chromium + Node（随包） | 无（系统框架） | — |

## 启动

| 指标 | Electron 版 | Rust 版 |
|---|---|---|
| 到数据库就绪 | 未测（见下） | **82 ms**（min 79 / median 82 / max 84，n=5） |
| 进程数 | 3+（main / GPU / network / renderer） | **1** |

## 内存

| 指标 | Electron 版 | Rust 版 |
|---|---|---|
| 常驻内存（空闲，启动后 2 s） | 未测（见下） | **135 MB**（单进程 RSS，n=5，波动 < 1 MB） |

## 为什么 Electron 侧没有启动/内存数字

打包版**不接受隔离的数据目录**：`--user-data-dir` 被应用内部的固定路径覆盖
（实测启动后子进程命令行显示 `--user-data-dir=/Users/…/Library/Application Support/DevHarbor`），
也就是说任何一次启动都会直接操作**真实用户数据库**。

为保护真实数据，测量在这里停止。要做公平对比，需要在 Electron 版里让 userData 路径可被
参数覆盖（一行改动），或用一台一次性环境跑。

> 这与 Rust 版的一个直接改进对应：`--data-dir` 让运行时完全不碰真实数据，
> 上面 82 ms / 135 MB 就是在临时目录里测出来的。

## 可信度说明

- 启动时间的定义是"进程 exec 到 `<data-dir>/devharbor.db` 出现"，即核心初始化完成；
  **不包含**首帧绘制（那需要屏幕录制权限，本机不可用）。
- 内存是 RSS，不含压缩内存；GPUI 的 Metal 纹理按需增长，空闲值不代表加载大项目后的峰值。
- 本机 `CGWindowListCopyWindowInfo` 对所有应用都返回失真尺寸，因此没有"窗口尺寸"类指标。
