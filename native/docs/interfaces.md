# 核心接口契约（阶段 2 落地，阶段 3—8 实现）

> 状态：**契约先定、实现后跟**。本文件描述的枚举与结构在实现时会细化，但
> `CoreHandle::request` / `subscribe` / `GetSnapshot` / `LogCursor` / `WindowSession::prepare_close`
> / `Shutdown` 这几个名字与语义是计划约定，不得在实现阶段改名或换成"UI 各自实现一套"。
>
> 依赖方向（不得反向）：
> `domain` ← `storage` ← `core` ← {`mcp`, `ui`} ← `desktop`；`platform-macos` 不得依赖 `ui`。
> GPUI 类型只允许出现在 `ui` 与 `desktop`：`domain`/`storage`/`core`/`mcp` 里出现
> `gpui::*` 即为设计违规。

## 1. 唯一业务入口

```rust
/// 所有业务命令的唯一入口。UI 与 MCP 都走这条路径，不存在第二套启动/写库逻辑。
#[derive(Clone)]
pub struct CoreHandle { /* Arc<CoreActor> + request channel */ }

impl CoreHandle {
    /// 提交一条命令，等待结果。命令在核心 actor 中串行化执行；
    /// 阻塞型工作（PTY 读、进程等待、SQL、目录扫描、就绪探测）派发到工作线程，
    /// 绝不占用 GPUI 主线程。
    pub async fn request(&self, command: CoreCommand) -> Result<CoreReply, CoreError>;

    /// 订阅状态事件。返回的订阅先建好接收缓冲，再让调用方取快照。
    pub fn subscribe(&self) -> CoreSubscription;
}
```

`CoreSubscription` 语义：

- 事件带单调递增的 `revision`；`CoreCommand::GetSnapshot` 返回同一 `revision` 的快照。
- 订阅建立后收到的、`revision <= snapshot.revision` 的事件直接丢弃——只应用更新的。
- 通道满时**不允许静默丢生命周期事件**：要么背压，要么发 `CoreEventKind::ResyncRequired { revision }`
  让 UI 重新取快照。统计类事件（CPU/内存）允许合并。

## 2. 命令与回复

```rust
pub enum CoreCommand {
    GetSnapshot,
    // 目录/任务
    CreateApp(CreateAppInput),
    UpdateApp(UpdateAppInput),
    RemoveApp { app_id: AppId },
    ScanFolder { dir: PathBuf },
    DetectProject { path: PathBuf },
    AddTask(AddTaskInput),
    UpdateTask(UpdateTaskInput),
    RemoveTask { task_id: TaskId },
    ReorderTasks { app_id: AppId, task_ids: Vec<TaskId> },
    RenameFolder { from: String, to: String },
    ClearFolder { name: String },
    // 生命周期
    StartApp { app_id: AppId },
    StopApp { app_id: AppId },
    RestartApp { app_id: AppId },
    StartTask { task_id: TaskId },
    StopTask { task_id: TaskId },
    ResizeTask { task_id: TaskId, cols: u16, rows: u16 },
    // 日志
    ReadLogBuffer { task_id: TaskId },
    TailLogBuffer { task_id: TaskId, max_lines: Option<usize> },
    ClearLogBuffer { task_id: TaskId },
    SearchLogs { query: String, flags: Option<String>, limit: Option<usize> },
    // 环境变量 / 设置 / 历史
    SetEnvVars(SetEnvVarsInput),
    UpdateSettings(SettingsPatch),
    ListRunHistory { app_id: AppId, limit: Option<usize> },
    // 系统
    Shutdown,
}

pub enum CoreReply {
    Snapshot(Box<CoreSnapshot>),
    App(App),
    Task(Task),
    Tasks(Vec<Task>),
    Logs(String),
    LogMatches(Vec<GlobalLogMatch>),
    Settings(Settings),
    RunHistory(Vec<RunHistoryRow>),
    Ack,
}
```

**输入结构显式化**：`CreateAppInput`/`UpdateAppInput`/`AddTaskInput`/`UpdateTaskInput`/`SettingsPatch`
都是具名字段的结构体，**不接受**可以覆盖 `id`/`path`/`createdAt` 的任意 map。

## 3. 快照与事件

```rust
pub struct CoreSnapshot {
    pub revision: u64,
    pub apps: Vec<App>,
    pub tasks_by_app: BTreeMap<AppId, Vec<Task>>,
    pub running_tasks: BTreeMap<TaskId, RunningTask>,
    pub running_apps: BTreeMap<AppId, RunningApp>,
    pub app_states: BTreeMap<AppId, ProcessState>,
    pub settings: Settings,
}

pub struct CoreEvent {
    pub revision: u64,
    pub kind: CoreEventKind,
}

pub enum CoreEventKind {
    TaskStatus { task_id: TaskId, app_id: AppId, state: ProcessState, ready: bool,
                 readiness_kind: ReadinessKind, exit: Option<ExitOutcome> },
    AppStatus  { app_id: AppId, state: ProcessState },
    TaskStats  { task_id: TaskId, cpu_percent: f32, mem_bytes: u64 },
    TaskPorts  { task_id: TaskId, ports: Vec<u16> },
    EnvFileChanged { app_id: AppId, path: PathBuf, kind: EnvFileEventKind },
    CatalogChanged { app_id: Option<AppId> },
    SettingsChanged(Box<Settings>),
    LogChunk { task_id: TaskId, run_id: RunId, seq: u64, bytes: Bytes },
    LogReset { task_id: TaskId, bootstrap: Box<LogBootstrap> },
    ResyncRequired { revision: u64 },
    McpStatusChanged(Box<McpStatus>),
}
```

`ProcessState` 精确保留 `idle / starting / running / exiting / exited / crashed`。
`ready` 与 `listening` 是**独立**概念：`ready` 来自 readiness 探测，`listening` 来自端口探测，
未配置探测时 MCP 的 `readinessVerified` 必须是 `null` 而不是 `false`。

## 4. 日志订阅（跨 UTF-8/ANSI 边界安全）

```rust
pub struct LogCursor { pub run_id: RunId, pub seq: u64 }

pub struct LogBootstrap {
    pub cursor: LogCursor,        // 客户端应从这条之后继续接收
    pub terminal: TerminalGrid,   // 已解析好的网格 + scrollback，不含任何 GPUI 实体
    pub raw_tail: Bytes,          // 有界原始尾部，用于"只看原始输出"
    pub truncated: bool,          // 历史已被淘汰时必须为 true，UI 要显示截断提示
}

pub enum LogSubscriptionOutcome {
    Ok(LogBootstrap),
    ResetRequired { bootstrap: LogBootstrap }, // 游标已淘汰：给快照，不伪造连续日志
}

impl CoreHandle {
    pub fn subscribe_logs(&self, task_id: TaskId, after: Option<LogCursor>) -> LogSubscription;
}
```

约束：原始缓冲按**真实 UTF-8 字节**计（旧版用 JS 字符串长度近似字节，差异记入兼容报告）；
淘汰历史时不得丢弃解析器还没消费的字节，否则终端会永久失步。

## 5. 窗口与进程生命周期

```rust
pub struct WindowSession { /* 可释放：窗口、视图实体、订阅、定时器 */ }

impl WindowSession {
    /// 关窗前的保存屏障：把在途编辑与会话状态提交到核心。
    /// 失败返回可见错误，调用方必须保留窗口；成功后才销毁窗口。
    /// 它**不会**触发 `CoreCommand::Shutdown`。
    pub async fn prepare_close(&self, core: &CoreHandle) -> Result<(), SaveError>;
}
```

`Shutdown` 顺序（不得交换）：

1. 状态置为"拒绝新启动"；
2. 等待/取消在途启动，并回收它们可能已经创建的子进程；
3. 反向拓扑停止全部受管任务（TERM → grace → KILL 整棵进程树）；
4. 写运行历史（把仍在运行的任务记为退出）；
5. 关闭 PTY/统计/监听工作单元；
6. 落库并关闭数据库线程；
7. 退出主循环。

只终止本进程实际启动的 `live handle`；**不得**凭数据库里的旧 PID 杀进程，也不得终止
NeoHub 自己或无关进程组。

## 6. 错误

```rust
pub enum CoreError {
    NotFound { entity: &'static str, id: String },
    InvalidInput { field: &'static str, message: String },
    PathMissing { path: PathBuf },
    DuplicatePath { path: PathBuf, existing: AppId },
    DependencyCycle { tasks: Vec<TaskId> },
    UnknownDependency { task_id: TaskId, missing: TaskId },
    SpawnFailed { task_id: TaskId, message: String },
    AlreadyStarting { task_id: TaskId },
    SecretUnavailable { key: String },
    ReadinessTimeout { task_id: TaskId, timeout_ms: u64 },
    Storage(String),
    Cancelled,
}
```

错误必须可定位到字段/实体：UI 把 `InvalidInput.field` 映射到具体输入框，MCP 把它映射成
参数校验错误，两者都不产生"半个事务"。

## 7. 与旧 IPC 的映射

旧版的 59 个 invoke 频道不是逐条翻译，而是收敛成上面的命令；映射表随阶段实现补全，
届时 `FEATURE-PARITY.md` 的 Rust 状态列同步更新。旧的 22 个 event 频道同理收敛为
`CoreEventKind`，其中 `task:log` / `task:subscribeLogs` 的可见性门控改为
`subscribe_logs` + `CoreEventKind::LogChunk`（只为订阅中的任务推送）。
