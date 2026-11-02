import type { en } from './en';

/**
 * 简体中文。类型标注为 `typeof en`，所以英文新增 key 而这里漏翻会直接编译失败。
 */
export const zhCN: typeof en = {
  common: {
    close: '关闭',
    copy: '复制',
    copied: '已复制',
    loading: '加载中…',
    restart: '重启',
    cancel: '取消',
    confirm: '确认',
    dismiss: '关闭',
    dismissError: '关闭此错误',
    dismissNotification: '关闭通知',
    emptyValue: '内容不能为空。',
    notifications: '通知',
    sortApps: '排序应用',
    sortTooltip: '排序应用 - {{mode}}',
    sortName: '名称（A-Z）',
    sortRecent: '最近使用',
    sortRunning: '运行中优先'
  },

  state: {
    idle: '空闲',
    starting: '启动中',
    running: '运行中',
    exiting: '停止中',
    exited: '已退出',
    crashed: '已崩溃'
  },

  appShell: {
    deepLinkStartTitle: '启动“{{name}}”？',
    deepLinkStartDesc:
      'devharbor:// 链接请求启动此应用，这会执行它的任务（包括自定义 shell 命令）。',
    deepLinkStartConfirm: '启动应用'
  },

  errors: {
    startFailed: '启动失败',
    stopFailed: '停止失败',
    restartFailed: '重启失败',
    removeFailed: '移除失败',
    unknown: '出错了，请重试。'
  },

  sidebar: {
    dashboard: '仪表盘',
    apps: '应用',
    appsCount: '· {{shown}} / {{total}}',
    filterApps: '筛选应用（⌘ P）',
    commandPalette: '命令面板（⌘ K）',
    groupByFolder: '按文件夹分组',
    groupByTag: '按标签分组',
    addAppOrFolder: '添加应用或文件夹',
    newApp: '新建应用…',
    newFolder: '新建文件夹…',
    importProjects: '导入项目…',
    noApps: '还没有应用',
    noMatches: '没有匹配项',
    noTagsYet: '还没有标签。在应用的设置（⚙）里添加标签，或把带标签的应用拖到这里。',
    ungrouped: '（未分组）',
    untagged: '（未加标签）',
    settings: '设置',
    settingsTooltip: '设置（⌘ ,）',
    moreActions: '更多操作',
    moreActionsFor: '{{name}} 的更多操作',
    folderActions: '文件夹操作',
    folderActionsFor: '文件夹 {{name}} 的操作',
    groupRunning: '该分组中有应用正在运行',
    groupRunningAria: '分组内有运行中的应用',
    emptyDropHere: '空文件夹 - 把应用拖到这里。',
    headerTagNone: '没有标签的应用。',
    headerTag: '标签：{{name}}。把应用拖到这里即可添加该标签。',
    headerFolderNone: '未归入文件夹的应用。把应用拖到这里可移除其文件夹。',
    headerFolder: '文件夹：{{name}}。拖拽可调整顺序 · 右键或 ⋮ 查看操作。',

    menu: {
      open: '打开',
      start: '启动',
      stop: '停止',
      restart: '重启',
      addTag: '添加标签：{{name}}',
      newTag: '新建标签…',
      removeTag: '移除标签：{{name}}',
      moveToFolder: '→ {{name}}',
      newFolder: '→ 新建文件夹…',
      removeFromFolder: '→ 移出文件夹',
      revealInFinder: '在 Finder 中显示',
      openInTerminal: '在终端中打开',
      removeApp: '从 NeoHub 移除…'
    },

    folderMenu: {
      startAll: '全部启动',
      stopAll: '全部停止',
      renameFolder: '重命名文件夹…',
      moveUp: '上移',
      moveDown: '下移',
      deleteFolder: '删除文件夹'
    },

    folderStartContext: '全部启动 - {{name}}',
    folderStopContext: '全部停止 - {{name}}',

    prompt: {
      newFolderTitle: '新建文件夹',
      newFolderDesc: '空文件夹会立即出现，把应用拖进去即可。',
      newFolderDescForApp: '新建一个文件夹，并把“{{name}}”移入其中。',
      newFolderPlaceholder: '例如：工作 · 个人 · 副业',
      create: '创建',
      newTagTitle: '新建标签',
      newTagDesc: '为“{{name}}”添加一个标签。',
      newTagPlaceholder: '例如：api · 前端 · project-x',
      add: '添加',
      renameFolderTitle: '重命名文件夹“{{name}}”',
      renameFolderPlaceholder: '新的文件夹名',
      rename: '重命名',
      deleteFolderTitle: '删除文件夹“{{name}}”？',
      deleteFolderDesc: '{{count}} 个应用将移动到“（未分组）”。',
      deleteFolderDescOne: '1 个应用将移动到“（未分组）”。',
      deleteFolderFilesNote: '磁盘上的文件不受影响。',
      deleteFolder: '删除文件夹',
      removeAppTitle: '从 NeoHub 移除“{{name}}”？',
      removeAppDesc: '只会从 NeoHub 中移除，磁盘上的文件不会被删除。',
      remove: '移除'
    },

    validation: {
      reserved: '“（未分组）”是保留名称。',
      folderExists: '文件夹“{{name}}”已存在。',
      maxChars: '最多 {{max}} 个字符。',
      alreadyTagged: '已添加标签“{{name}}”。'
    }
  },

  dashboard: {
    title: '仪表盘',
    noAppsRegistered: '还没有注册应用',
    appCount_one: '{{count}} 个应用',
    appCount_other: '{{count}} 个应用',
    tagSuffix: ' · 标签：{{tag}}',
    allTags: '全部标签',
    stopAllTooltip: '停止所有运行中的应用',
    stopAll: '全部停止',
    empty: '还没有注册应用。',
    noTagMatch: '没有应用带有标签“{{tag}}”。',
    statRunning: '运行中',
    statCpu: 'CPU',
    statMemory: '内存',
    statOpenPorts: '占用端口',
    taskCount_one: '{{count}} 个任务',
    taskCount_other: '{{count}} 个任务',
    runningSuffix: ' · {{count}} 个运行中',
    cardStart: '启动',
    cardStartTooltip: '启动此应用的所有任务',
    cardStop: '停止',
    cardStopTooltip: '停止此应用里所有正在运行的任务',
    cardLogs: '日志',
    cardLogsTooltip: '打开此应用详情与日志'
  },

  appDetail: {
    envVars: '环境变量',
    appSettings: '应用设置',
    startApp: '启动应用',
    startAppTooltip: '启动此应用的所有任务（⌘ ↩）',
    restartApp: '重启应用',
    restartAppTooltip: '停止后再重新启动此应用',
    stopApp: '停止应用',
    stopAppTooltip: '停止此应用里所有正在运行的任务',
    removeApp: '从 NeoHub 移除',
    copyPath: '复制路径',
    copyPathAria: '复制应用路径到剪贴板',
    noTasksHint: '还没有任务。可在下方任务栏添加。',
    envFilesChanged_one: '{{file}} 已在磁盘上被修改。',
    envFilesChanged_other: '{{count}} 个 env 文件已在磁盘上被修改。',
    envRestartToApply: '重启后生效。',
    envRestartTooltip: '重启应用以加载改动后的 env 文件',
    dismissNotice: '关闭此提示',
    noTasksForApp: '此应用还没有任务。',
    addTask: '添加任务',
    logs: '日志',
    logsLive: '实时',
    logsFilter: '筛选',
    copyLogs: '复制全部日志到剪贴板',
    saveLogs: '保存日志到文件',
    logsSave: '保存',
    clearLogs: '清空',
    noTasksUseAdd: '还没有任务。请使用上方任务栏中的“添加任务”。',
    removeTitle: '从 NeoHub 移除“{{name}}”？',
    removeDescRunning:
      '此应用正在运行。会先停止它的所有任务再移除。磁盘上的文件不会被删除。',
    removeDescIdle: '只会从 NeoHub 中移除，磁盘上的文件不会被删除。',
    stopAndRemove: '停止并移除'
  },

  empty: {
    welcome: '欢迎使用 NeoHub',
    tagline:
      '为本地开发服务器打造的港湾。在一个窗口里运行并监控你的 Node.js 项目。所有数据都留在本地：无需账号、不上云、没有遥测。',
    featureRunTitle: '运行与监控',
    featureRunDesc: '启动、停止、重启开发服务器，实时查看 CPU、内存与端口。',
    featureTaskTitle: '多任务应用',
    featureTaskDesc: '包含多个服务的应用会按依赖顺序启动，并等待就绪信号。',
    featureNodeTitle: '按项目切换 Node 与环境变量',
    featureNodeDesc: '自动切换 Node 版本，环境变量分层覆盖：全局 → 应用 → 任务。',
    featureLogsTitle: '实时日志与端口',
    featureLogsDesc: '支持搜索的 xterm 日志，自动识别端口并生成可点击的 localhost 链接。',
    addFirstApp: '添加第一个应用',
    importFolder: '导入包含多个项目的文件夹…',
    hint: '把 NeoHub 指向一个 Node.js 项目文件夹，它会自动识别包管理器、Node 版本和可用脚本。'
  },

  settings: {
    title: '设置',
    closeTooltip: '关闭设置',
    general: '通用',
    theme: '主题',
    themeDesc: '界面配色方案。',
    themeSystem: '跟随系统',
    themeDark: '深色',
    themeLight: '浅色',
    language: '语言',
    languageDesc: 'NeoHub 的界面语言。“跟随系统”将使用 macOS 的语言设置。',
    languageSystem: '跟随系统',
    autoUpdate: '启动时检查更新',
    autoUpdateDesc: '自动下载并安装 NeoHub 的新版本。',
    launchAtLogin: '登录时启动 NeoHub',
    launchAtLoginDesc: '登录 Mac 时自动启动 NeoHub。',
    trayEnabled: '显示菜单栏图标',
    trayEnabledDesc: '在 macOS 菜单栏保留托盘图标，便于快速访问和后台控制。',

    notifications: '通知',
    notifyCrash: '应用崩溃时通知',
    notifyCrashDesc: '当 NeoHub 在后台运行时，任务意外退出会发送桌面通知。',
    notifyReady: '应用就绪时通知',
    notifyReadyDesc: '应用启动完成（收到就绪信号）时发送桌面通知。',

    unitMs: 'ms',
    unitLines: '行',
    unitRuns: '次',

    dashboard: '仪表盘',
    refreshLabel: '指标刷新间隔',
    refreshDesc: '每个任务 CPU/内存的采样频率（毫秒）。数值越小曲线越平滑，但更耗 CPU。',

    logs: '日志',
    ringLabel: '环形缓冲区大小',
    ringDesc: '每个任务在内存中保留的日志行数，更早的行会被丢弃。',
    historyLabel: '每个应用保留的运行记录',
    historyDesc: '每个应用最多保留的 run_history 行数，超出部分会在启动时清理。',

    processes: '进程',
    killGraceLabel: '停止宽限期',
    killGraceDesc:
      '点击停止后，等待 SIGTERM 生效的时间（毫秒），超时后会对进程组发送 SIGKILL。',
    readinessLabel: '就绪超时',
    readinessDesc: '若任务的就绪信号在该时间内没有出现（毫秒），则中止启动。',

    node: 'Node 检测',
    nodeDesc:
      '在这台机器上发现的安装（只读 - 每次启动应用时都会重新扫描）。要新增版本，请用常用的版本管理器（nvm / fnm / volta / asdf）安装，然后点击应用的启动按钮。',
    nodeNone: '未发现。',

    mcp: 'MCP 服务器',
    mcpIntro:
      '让编码代理通过本地 HTTP 经由 NeoHub 管理你的开发服务器。服务只监听 127.0.0.1。持有令牌的客户端可以注册文件夹并执行其中脚本，请把它当作密码对待。',
    mcpEnable: '启用 MCP 服务器',
    mcpEnableDesc: '向 MCP 客户端开放应用管理工具（列表、添加、启动、停止、日志）。',
    mcpStatus: '状态',
    mcpStatusDesc: '本地服务器的实时状态。',
    mcpStopped: '已停止',
    mcpPort: '端口',
    mcpPortDesc: '监听端口（1024-65535）。修改后服务器会重启。',
    mcpHttps: '使用 HTTPS',
    mcpHttpsDesc: '使用自签名的 localhost 证书以 TLS 提供服务。客户端需要信任该证书。',
    mcpCert: '证书',
    mcpCertDesc: '让客户端指向该文件（例如 NODE_EXTRA_CA_CERTS），或将它加入钥匙串。',
    mcpCopyPath: '复制路径',
    mcpRequireAuth: '要求认证令牌',
    mcpRequireAuthDesc: '每个请求都必须携带 bearer 令牌。推荐开启。',
    mcpNoAuthWarning:
      '不设令牌时，这台 Mac 上的任何程序都能通过 NeoHub 控制你的应用。在个人机器上问题不大；如果有其他人登录这台机器，或你会运行不信任的软件，请重新开启。',
    mcpToken: '认证令牌',
    mcpTokenDesc: '以 Authorization: Bearer 请求头发送。',
    mcpTokenHide: '隐藏',
    mcpTokenReveal: '显示',
    mcpTokenRegenerateTooltip: '重新生成令牌',
    mcpTokenRegenerateTitle: '重新生成令牌？',
    mcpTokenRegenerateDesc: '当前令牌会立即失效，所有已连接的客户端都必须更新为新令牌。',
    mcpTokenRegenerate: '重新生成',
    mcpPlaintextWarning:
      'macOS 钥匙串不可用，令牌以未加密形式保存，目前仅靠文件权限保护。',
    mcpClientConfig: '客户端配置',
    mcpCopyConfig: '复制配置',

    danger: '危险操作',
    dbPath: '数据库位置',
    exportedTo: '已导出到',
    exportLabel: '导出数据库',
    exportDesc:
      '把当前的 devharbor.db 复制到指定文件，包含应用列表、任务、环境变量、运行记录和设置。',
    exportButton: '导出…',
    resetLabel: '重置数据库',
    resetDesc:
      '把数据库移走并以空库重启。磁盘上的文件不受影响，但需要重新添加应用。同时会生成备份。',
    resetButton: '重置…',
    resetConfirmTitle: '重置数据库？',
    resetConfirmDesc:
      '这会关闭 NeoHub、归档当前数据库并以空库重启。同时会生成备份。磁盘上的文件不受影响，但你需要重新添加应用。',
    resetConfirm: '重置数据库'
  }
};
