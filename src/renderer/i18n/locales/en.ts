/**
 * English strings - the source of truth for every key. `zh-CN.ts` is typed against this
 * object, so adding a key here without a translation is a compile error.
 *
 * Conventions:
 * - Group by the UI surface (`sidebar`, `dashboard`, `appDetail`, `settings`, ...).
 * - Interpolate with `{{name}}` and pass the values as the second argument to `t()`.
 * - Counted strings use i18next plural suffixes (`_one` / `_other`).
 */
export const en = {
  common: {
    close: 'Close',
    copy: 'Copy',
    copied: 'Copied',
    loading: 'Loading…',
    restart: 'Restart',
    cancel: 'Cancel',
    confirm: 'Confirm',
    dismiss: 'Dismiss',
    dismissError: 'Dismiss this error',
    dismissNotification: 'Dismiss notification',
    emptyValue: 'Value cannot be empty.',
    notifications: 'Notifications',
    sortApps: 'Sort apps',
    sortTooltip: 'Sort apps - {{mode}}',
    sortName: 'Name (A-Z)',
    sortRecent: 'Recently used',
    sortRunning: 'Running first'
  },

  /** Process-state labels, shared by StatusDot, the dashboard cards and the detail header. */
  state: {
    idle: 'Idle',
    starting: 'Starting',
    running: 'Running',
    exiting: 'Exiting',
    exited: 'Exited',
    crashed: 'Crashed'
  },

  /** App shell (App.tsx) prompts that aren't tied to a single surface. */
  appShell: {
    deepLinkStartTitle: 'Start “{{name}}”?',
    deepLinkStartDesc:
      'A devharbor:// link asked to start this app, which runs its tasks (including any custom shell commands).',
    deepLinkStartConfirm: 'Start app'
  },

  /** Toast prefixes for failed lifecycle IPC calls. */
  errors: {
    startFailed: 'Start failed',
    stopFailed: 'Stop failed',
    restartFailed: 'Restart failed',
    removeFailed: 'Remove failed',
    unknown: 'Something went wrong.'
  },

  sidebar: {
    dashboard: 'Dashboard',
    apps: 'Apps',
    appsCount: '· {{shown}} of {{total}}',
    filterApps: 'Filter apps (⌘ P)',
    commandPalette: 'Command palette (⌘ K)',
    groupByFolder: 'Group by folder',
    groupByTag: 'Group by tag',
    addAppOrFolder: 'Add app or folder',
    newApp: 'New app…',
    newFolder: 'New folder…',
    importProjects: 'Import projects…',
    noApps: 'No apps yet',
    noMatches: 'No matches',
    noTagsYet:
      "No tags yet. Add tags in an app's settings (⚙), or drag a tagged app here.",
    ungrouped: '(Ungrouped)',
    untagged: '(Untagged)',
    settings: 'Settings',
    settingsTooltip: 'Settings (⌘ ,)',
    moreActions: 'More actions',
    moreActionsFor: 'More actions for {{name}}',
    folderActions: 'Folder actions',
    folderActionsFor: 'Actions for folder {{name}}',
    groupRunning: 'An app in this group is running',
    groupRunningAria: 'Group has a running app',
    emptyDropHere: 'Empty - drop apps here.',
    headerTagNone: 'Apps with no tags.',
    headerTag: 'Tag: {{name}}. Drop an app here to add this tag.',
    headerFolderNone: 'Apps without a folder. Drop apps here to remove their folder.',
    headerFolder: 'Folder: {{name}}. Drag to reorder · right-click or ⋮ for actions.',

    menu: {
      open: 'Open',
      start: 'Start',
      stop: 'Stop',
      restart: 'Restart',
      addTag: 'Add tag: {{name}}',
      newTag: 'New tag…',
      removeTag: 'Remove tag: {{name}}',
      moveToFolder: '→ {{name}}',
      newFolder: '→ New folder…',
      removeFromFolder: '→ Remove from folder',
      revealInFinder: 'Reveal in Finder',
      openInTerminal: 'Open in Terminal',
      removeApp: 'Remove from NeoHub…'
    },

    folderMenu: {
      startAll: 'Start all',
      stopAll: 'Stop all',
      renameFolder: 'Rename folder…',
      moveUp: 'Move up',
      moveDown: 'Move down',
      deleteFolder: 'Delete folder'
    },

    /** Toast context for the folder-level start/stop sweep. */
    folderStartContext: 'Start all - {{name}}',
    folderStopContext: 'Stop all - {{name}}',

    prompt: {
      newFolderTitle: 'New folder',
      newFolderDesc: 'Empty folder shows immediately. Drag apps into it.',
      newFolderDescForApp: 'Create a new folder and move "{{name}}" into it.',
      newFolderPlaceholder: 'e.g. Work · Personal · Side projects',
      create: 'Create',
      newTagTitle: 'New tag',
      newTagDesc: 'Add a tag to "{{name}}".',
      newTagPlaceholder: 'e.g. api · frontend · project-x',
      add: 'Add',
      renameFolderTitle: 'Rename folder "{{name}}"',
      renameFolderPlaceholder: 'New folder name',
      rename: 'Rename',
      deleteFolderTitle: 'Delete folder "{{name}}"?',
      deleteFolderDesc: '{{count}} apps will move to "(Ungrouped)".',
      deleteFolderDescOne: '1 app will move to "(Ungrouped)".',
      deleteFolderFilesNote: 'Files on disk are not affected.',
      deleteFolder: 'Delete folder',
      removeAppTitle: 'Remove "{{name}}" from NeoHub?',
      removeAppDesc: 'This only removes it from NeoHub. Files on disk are not deleted.',
      remove: 'Remove'
    },

    validation: {
      reserved: '"(Ungrouped)" is reserved.',
      folderExists: 'Folder "{{name}}" already exists.',
      maxChars: 'Max {{max}} characters.',
      alreadyTagged: 'Already tagged "{{name}}".'
    }
  },

  dashboard: {
    title: 'Dashboard',
    noAppsRegistered: 'No apps registered yet',
    appCount_one: '{{count}} app',
    appCount_other: '{{count}} apps',
    tagSuffix: ' · tag: {{tag}}',
    allTags: 'All tags',
    stopAllTooltip: 'Stop every running app',
    stopAll: 'Stop all',
    empty: 'No apps registered yet.',
    noTagMatch: 'No apps match tag "{{tag}}".',
    statRunning: 'Running',
    statCpu: 'CPU',
    statMemory: 'Memory',
    statOpenPorts: 'Open ports',
    taskCount_one: '{{count}} task',
    taskCount_other: '{{count}} tasks',
    runningSuffix: ' · {{count}} running',
    cardStart: 'Start',
    cardStartTooltip: 'Start all tasks in this app',
    cardStop: 'Stop',
    cardStopTooltip: 'Stop all running tasks in this app',
    cardLogs: 'Logs',
    cardLogsTooltip: "Open this app's detail & logs"
  },

  appDetail: {
    envVars: 'Environment variables',
    appSettings: 'App settings',
    startApp: 'Start app',
    startAppTooltip: 'Start all tasks in this app (⌘ ↩)',
    restartApp: 'Restart app',
    restartAppTooltip: 'Stop, then start this app again',
    stopApp: 'Stop app',
    stopAppTooltip: 'Stop all running tasks in this app',
    removeApp: 'Remove from NeoHub',
    copyPath: 'Copy path',
    copyPathAria: 'Copy app path to clipboard',
    noTasksHint: 'No tasks yet. Add one from the strip below.',
    envFilesChanged_one: '{{file}} changed on disk.',
    envFilesChanged_other: '{{count}} env files changed on disk.',
    envRestartToApply: 'Restart to apply.',
    envRestartTooltip: 'Restart the app to load the changed env files',
    dismissNotice: 'Dismiss this notice',
    noTasksForApp: 'No tasks yet for this app.',
    addTask: 'Add task',
    logs: 'Logs',
    logsLive: 'Live',
    logsFilter: 'Filter',
    copyLogs: 'Copy all logs to clipboard',
    saveLogs: 'Save logs to file',
    logsSave: 'Save',
    clearLogs: 'Clear',
    noTasksUseAdd: 'No tasks yet. Use "Add task" in the strip above.',
    removeTitle: 'Remove "{{name}}" from NeoHub?',
    removeDescRunning:
      'This app is currently running. Its tasks will be stopped first, then it will be removed. Files on disk are not deleted.',
    removeDescIdle: 'This only removes it from NeoHub. Files on disk are not deleted.',
    stopAndRemove: 'Stop & remove'
  },

  empty: {
    welcome: 'Welcome to NeoHub',
    tagline:
      'A harbor for your local dev servers. Run and monitor your Node.js projects from one window. Everything stays local: no accounts, no cloud, no telemetry.',
    featureRunTitle: 'Run & monitor',
    featureRunDesc:
      'Start, stop, and restart your dev servers, with live CPU, memory, and ports.',
    featureTaskTitle: 'Multi-task apps',
    featureTaskDesc:
      'Apps with several services start in dependency order and wait for readiness.',
    featureNodeTitle: 'Per-project Node & env',
    featureNodeDesc:
      'Auto-switch Node versions and layer environment variables: global → app → task.',
    featureLogsTitle: 'Live logs & ports',
    featureLogsDesc:
      'Searchable xterm logs and auto-detected ports with clickable localhost links.',
    addFirstApp: 'Add your first app',
    importFolder: 'Import a folder of projects…',
    hint: 'Point NeoHub at a Node.js project folder - it detects the package manager, Node version, and scripts for you.'
  },

  settings: {
    title: 'Settings',
    closeTooltip: 'Close settings',
    general: 'General',
    theme: 'Theme',
    themeDesc: 'Color scheme for the UI.',
    themeSystem: 'System',
    themeDark: 'Dark',
    themeLight: 'Light',
    language: 'Language',
    languageDesc:
      'Display language for NeoHub. "System default" follows your macOS language.',
    languageSystem: 'System default',
    autoUpdate: 'Check for updates on launch',
    autoUpdateDesc: 'Automatically download and install new NeoHub releases.',
    launchAtLogin: 'Launch NeoHub at login',
    launchAtLoginDesc: 'Start NeoHub automatically when you log in to your Mac.',
    trayEnabled: 'Show menubar icon',
    trayEnabledDesc:
      'Keep an icon in the macOS menubar. Closing the window frees its memory and hides the Dock icon while your servers keep running. Click the menubar icon or reopen the app to restore the window and your last selection.',

    notifications: 'Notifications',
    notifyCrash: 'Notify when an app crashes',
    notifyCrashDesc:
      'Show a desktop notification when a task exits unexpectedly while NeoHub is backgrounded.',
    notifyReady: 'Notify when an app is ready',
    notifyReadyDesc:
      'Show a desktop notification when an app finishes starting (readiness reached).',

    unitMs: 'ms',
    unitLines: 'lines',
    unitRuns: 'runs',

    dashboard: 'Dashboard',
    refreshLabel: 'Stats refresh interval',
    refreshDesc:
      'How often CPU/memory are sampled per task (in ms). Lower = smoother sparklines, more CPU.',

    logs: 'Logs',
    ringLabel: 'Ring buffer size',
    ringDesc: 'Per-task in-memory log lines. Older lines drop off the front.',
    historyLabel: 'Keep run history per app',
    historyDesc: 'Maximum run_history rows retained per app. Older rows are pruned on boot.',

    processes: 'Processes',
    killGraceLabel: 'Kill grace window',
    killGraceDesc:
      'After Stop, how long to wait (ms) for SIGTERM to take effect before escalating to SIGKILL on the process group.',
    readinessLabel: 'Readiness timeout',
    readinessDesc:
      "Abort a task's start if its readiness signal hasn't fired within this many ms.",

    node: 'Node detection',
    nodeDesc:
      "Installations discovered on this machine (read-only - the list is rescanned every time an app starts). To add a new version, install it via your usual manager (nvm / fnm / volta / asdf) and click an app's Start.",
    nodeNone: 'None found.',

    mcp: 'MCP server',
    mcpIntro:
      'Let coding agents manage your dev servers through NeoHub over local HTTP. The server listens on 127.0.0.1 only. Holding the token lets a client register a folder and run its scripts, so treat it like a password.',
    mcpEnable: 'Enable MCP server',
    mcpEnableDesc:
      'Expose app management tools (list, add, start, stop, logs) to MCP clients.',
    mcpStatus: 'Status',
    mcpStatusDesc: 'Live state of the local server.',
    mcpStopped: 'Stopped',
    mcpPort: 'Port',
    mcpPortDesc: 'Listen port (1024-65535). The server restarts when changed.',
    mcpHttps: 'Use HTTPS',
    mcpHttpsDesc:
      'Serve over TLS with a self-signed localhost certificate. Clients must trust the certificate.',
    mcpCert: 'Certificate',
    mcpCertDesc:
      'Point your client at this file (e.g. NODE_EXTRA_CA_CERTS) or add it to your keychain.',
    mcpCopyPath: 'Copy path',
    mcpRequireAuth: 'Require auth token',
    mcpRequireAuthDesc: 'Require a bearer token on every request. Recommended.',
    mcpNoAuthWarning:
      'Without a token, any program running on this Mac can control your apps through NeoHub. Fine on a personal machine; turn it back on if other people log in here or you run software you do not trust.',
    mcpToken: 'Auth token',
    mcpTokenDesc: 'Send as an Authorization: Bearer header.',
    mcpTokenHide: 'Hide',
    mcpTokenReveal: 'Reveal',
    mcpTokenRegenerateTooltip: 'Regenerate token',
    mcpTokenRegenerateTitle: 'Regenerate token?',
    mcpTokenRegenerateDesc:
      'The current token stops working immediately. Any connected client must be updated with the new token.',
    mcpTokenRegenerate: 'Regenerate',
    mcpPlaintextWarning:
      'The macOS keychain was unavailable, so the token is stored unencrypted. File permissions are its only protection.',
    mcpClientConfig: 'Client config',
    mcpCopyConfig: 'Copy config',

    danger: 'Danger zone',
    dbPath: 'Database lives at',
    exportedTo: 'Exported to',
    exportLabel: 'Export database',
    exportDesc:
      'Copy the current devharbor.db to a file. Includes app registry, tasks, env vars, run history, settings.',
    exportButton: 'Export…',
    resetLabel: 'Reset database',
    resetDesc:
      "Move the DB aside and restart with an empty one. Existing files on disk are untouched; you'll need to re-add your apps. A backup is created alongside.",
    resetButton: 'Reset…',
    resetConfirmTitle: 'Reset database?',
    resetConfirmDesc:
      'This will close NeoHub, archive the current database, and restart with an empty one. A backup is created alongside. Files on disk are untouched, but you will need to re-add your apps.',
    resetConfirm: 'Reset database'
  }
};
