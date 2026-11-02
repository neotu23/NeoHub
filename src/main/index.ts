import { app, BrowserWindow, dialog, screen, shell } from 'electron';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { db, closeDb } from './db/index.js';
import { registerAllIpcHandlers, type IpcRuntime } from './ipc/index.js';
import { installAppMenu } from './menu.js';
import { installProcessLogging, logger } from './services/Logger.js';
import { resolveUserDataDirectory } from './services/AppIdentity.js';

const isDev = process.env.NODE_ENV === 'development' || !!process.env.ELECTRON_RENDERER_URL;

// A product rename must not switch to an empty registry. Resolve before the lock or DB.
const userDataDirectory = resolveUserDataDirectory(app.getPath('appData'));
mkdirSync(userDataDirectory, { recursive: true });
app.setPath('userData', userDataDirectory);
app.setName('NeoHub');

let mainWindow: BrowserWindow | null = null;
let ipcRuntime: IpcRuntime | null = null;
let isQuitting = false;
// Set the moment the quit teardown commits (before-quit), cleared if the user cancels there.
// Closing the window must hide it only while we are NOT on our way out.
let quitCommitted = false;

interface WindowBounds {
  x?: number;
  y?: number;
  width: number;
  height: number;
}

function boundsFile(): string {
  return join(app.getPath('userData'), 'window-state.json');
}

/** Restore the last window bounds, validated against currently-connected displays. */
function loadBounds(): WindowBounds {
  const fallback: WindowBounds = { width: 1280, height: 820 };
  try {
    const raw = readFileSync(boundsFile(), 'utf8');
    const b = JSON.parse(raw) as Partial<WindowBounds>;
    if (typeof b.width !== 'number' || typeof b.height !== 'number') return fallback;
    const w = Math.max(960, Math.min(b.width, 8000));
    const h = Math.max(600, Math.min(b.height, 8000));
    // Only keep x/y if the window would land on some connected display (avoid off-screen).
    if (typeof b.x === 'number' && typeof b.y === 'number') {
      const onScreen = screen.getAllDisplays().some((d) => {
        const wa = d.workArea;
        return b.x! < wa.x + wa.width && b.x! + w > wa.x && b.y! < wa.y + wa.height && b.y! + h > wa.y;
      });
      if (onScreen) return { x: b.x, y: b.y, width: w, height: h };
    }
    return { width: w, height: h };
  } catch {
    return fallback;
  }
}

function saveBounds(win: BrowserWindow): void {
  if (win.isDestroyed()) return;
  try {
    const b = win.getNormalBounds();
    writeFileSync(boundsFile(), JSON.stringify({ x: b.x, y: b.y, width: b.width, height: b.height }));
  } catch {
    // ignore
  }
}

/**
 * Menubar-only residence. Closing the window hides it (taking the Dock icon with it) instead of
 * destroying it: background tasks keep running, the renderer keeps its state (selection, log
 * subscriptions) so reopening is instant, and the menubar icon is the way back in. Closing only
 * means closing when there is no tray icon to get back in, or when we are actually quitting.
 */
function shouldStayInMenubar(): boolean {
  return process.platform === 'darwin' && !quitCommitted && (ipcRuntime?.trayEnabled() ?? false);
}

/**
 * Restore Dock presence - the icon is hidden while the app lives in the menubar. Resolves to
 * true only when the Dock icon was actually hidden and macOS has now re-activated the app
 * (dock.show() is async); never rejects.
 */
function restoreDock(): Promise<boolean> {
  const dock = app.dock;
  if (!dock || dock.isVisible()) return Promise.resolve(false);
  return dock.show().then(
    () => true,
    () => false
  );
}

/** Bring the window - and the Dock icon that belongs to it - back to the foreground. */
function revealWindow(win: BrowserWindow): void {
  if (win.isDestroyed()) return;
  if (win.isMinimized()) win.restore();
  win.show();
  win.focus();
  // Re-assert focus once the app is a Dock app again: a window shown while still in menubar-only
  // (accessory) mode does not necessarily become the key window.
  void restoreDock().then((wasHidden) => {
    if (!wasHidden || win.isDestroyed()) return;
    app.focus({ steal: true });
    win.show();
    win.focus();
  });
}

/** Hide the window into the menubar, taking the Dock icon with it. */
function hideToMenubar(win: BrowserWindow): void {
  const hide = (): void => {
    if (win.isDestroyed()) return;
    win.hide();
    app.dock?.hide();
  };
  // Hiding a fullscreen window strands macOS on an empty Space - leave fullscreen first.
  if (win.isFullScreen()) {
    win.once('leave-full-screen', hide);
    win.setFullScreen(false);
    return;
  }
  hide();
}

// Single-instance lock: a second launch (or a deep-link open) focuses the existing
// window instead of spawning a second process with its own DB handle.
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    // Re-launching while an instance holds the lock must surface a window. Previously this
    // only focused an EXISTING window - if all windows were closed (app still alive on
    // macOS), the second launch did nothing, so the app looked like it "won't open".
    getOrCreateWindow();
  });
}

function createWindow(): BrowserWindow {
  // The Dock icon goes away with the window in menubar-only residence - bring it back with the
  // window (revealWindow does the same for an existing hidden window).
  void restoreDock();
  const bounds = loadBounds();
  const win = new BrowserWindow({
    ...bounds,
    minWidth: 960,
    minHeight: 600,
    title: 'NeoHub',
    show: false,
    backgroundColor: '#18181b', // matches the zinc-900 theme base - no launch color flash
    titleBarStyle: 'hiddenInset',
    trafficLightPosition: { x: 12, y: 14 },
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true
    }
  });
  mainWindow = win;

  win.once('ready-to-show', () => win.show());

  // Persist window size/position so the app reopens where the user left it. Closing then means
  // "live in the menubar" for as long as a tray icon exists (see shouldStayInMenubar) - the
  // window is hidden, not destroyed, so running dev servers and renderer state survive.
  win.on('close', (event) => {
    saveBounds(win);
    if (!shouldStayInMenubar()) return;
    event.preventDefault();
    hideToMenubar(win);
  });

  win.on('closed', () => {
    if (mainWindow === win) mainWindow = null;
  });

  // Reset per-renderer main-process state (log subscriptions) on reload/navigation, so a ⌘R
  // can't leave log forwarding gated to taskIds the new renderer never subscribed to.
  win.webContents.on('did-start-navigation', (_e, _url, _isInPlace, isMainFrame) => {
    if (isMainFrame) ipcRuntime?.onRendererReload();
  });

  // Renderer crash recovery: a GPU/OOM crash (xterm WebGL contexts are a realistic source)
  // would otherwise leave a blank window. Reload once so it self-heals - running PTYs live in
  // the main process and survive (IMPROVEMENT-PLAN 13.2).
  let reloadAttempts = 0;
  win.webContents.on('render-process-gone', (_e, details) => {
    logger.error('render-process-gone', details.reason, details.exitCode);
    if (reloadAttempts < 2 && details.reason !== 'clean-exit' && !win.isDestroyed()) {
      reloadAttempts += 1;
      win.webContents.reload();
    }
  });

  win.webContents.setWindowOpenHandler(({ url }) => {
    // Only forward http(s) - refuse file://, mailto:, etc. that a compromised
    // renderer could try to abuse.
    try {
      const parsed = new URL(url);
      if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
        shell.openExternal(url);
      }
    } catch {
      // Invalid URL - ignore.
    }
    return { action: 'deny' };
  });

  // Navigation guard: the renderer should never navigate the main window away from its
  // own document (loaded from file:// in prod, the dev server in dev). Any other target
  // is opened externally instead, so a compromised renderer can't replace the app shell.
  win.webContents.on('will-navigate', (event, url) => {
    const current = win.webContents.getURL();
    if (url === current) return;
    // Compare by ORIGIN, not string prefix - a prefix check would let
    // `http://localhost:5173.evil.com` pass as the dev server.
    let sameDevServer = false;
    if (process.env.ELECTRON_RENDERER_URL) {
      try {
        sameDevServer =
          new URL(url).origin === new URL(process.env.ELECTRON_RENDERER_URL).origin;
      } catch {
        sameDevServer = false;
      }
    }
    if (sameDevServer) return;
    event.preventDefault();
    try {
      const parsed = new URL(url);
      if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
        void shell.openExternal(url);
      }
    } catch {
      // ignore invalid URLs
    }
  });

  // Block <webview> embedding outright (we never use it).
  win.webContents.on('will-attach-webview', (event) => event.preventDefault());

  if (process.env.ELECTRON_RENDERER_URL) {
    win.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    win.loadFile(join(__dirname, '../renderer/index.html'));
  }

  if (isDev) {
    win.webContents.openDevTools({ mode: 'right' });
  }

  return win;
}

/**
 * Return the main window, creating it if there isn't one (the app keeps running on macOS
 * with all windows closed, and menubar-only residence hides rather than destroys it).
 * `created` lets the caller wait for the renderer to load before sending it a message. Used
 * by the application menu so ⌘, / ⌘N / ⌘⇧N aren't dead keys, by the tray icon, and by
 * notifications/deep links.
 */
function getOrCreateWindow(): { win: BrowserWindow; created: boolean } {
  if (mainWindow && !mainWindow.isDestroyed()) {
    revealWindow(mainWindow);
    return { win: mainWindow, created: false };
  }
  return { win: createWindow(), created: true };
}

app.whenReady().then(() => {
  if (isDev && process.platform === 'darwin') {
    app.dock?.setIcon(join(__dirname, '../../build/icon.png'));
  }
  // Route uncaught exceptions / rejections to the local log file for support diagnostics.
  installProcessLogging();

  // Configure the macOS About panel ( ⌘ → NeoHub → About NeoHub ).
  if (process.platform === 'darwin') {
    app.setAboutPanelOptions({
      applicationName: 'NeoHub',
      applicationVersion: app.getVersion(),
      version: app.getVersion(),
      copyright: 'Copyright © 2026 Jainath Ponnala',
      credits: 'A home for your local projects. Based on DevHarbor. Licensed under GNU AGPL-3.0.',
      website: 'https://github.com/neotu23/NeoHub'
    });
  }

  installAppMenu(isDev, getOrCreateWindow);
  // Opening the DB can fail (corrupt file, half-applied migration). Surface it instead of
  // launching to a dock icon with no window and no error (IMPROVEMENT-PLAN 8.1).
  try {
    db();
  } catch (e) {
    logger.error('database open/migration failed', e);
    const choice = dialog.showMessageBoxSync({
      type: 'error',
      title: 'NeoHub - database error',
      message: 'NeoHub could not open its database.',
      detail: `${(e as Error).message}\n\nThe database is at:\n${join(app.getPath('userData'), 'devharbor.db')}\n\nYou can move the corrupt database aside and start fresh, or quit.`,
      buttons: ['Move aside & restart', 'Quit'],
      defaultId: 0,
      cancelId: 1
    });
    if (choice === 0) {
      try {
        const p = join(app.getPath('userData'), 'devharbor.db');
        for (const suffix of ['', '-wal', '-shm']) {
          if (existsSync(`${p}${suffix}`)) {
            writeFileSync(`${p}${suffix}.corrupt-${Date.now()}.bak`, readFileSync(`${p}${suffix}`));
          }
        }
        for (const suffix of ['', '-wal', '-shm']) rmSync(`${p}${suffix}`, { force: true });
      } catch (moveErr) {
        logger.error('failed to move corrupt DB aside', moveErr);
      }
      app.relaunch();
    }
    app.exit(choice === 0 ? 0 : 1);
    return;
  }
  ipcRuntime = registerAllIpcHandlers(() => mainWindow, getOrCreateWindow);
  createWindow();

  app.on('activate', () => {
    // Dock click / reopening the app: reveal the window, whether it was destroyed (tray off)
    // or hidden in the menubar (tray on).
    getOrCreateWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

// Graceful teardown on quit (including auto-update's quitAndInstall). The whole point of the
// app is "stop apps cleanly, no orphans" - but the only quit hook used to be closeDb(), so
// running dev servers were killed abruptly via PTY teardown (or survived as orphans). Now we
// confirm, run the reverse-topo graceful stop, then close the DB last (IMPROVEMENT-PLAN 5.9).
let teardownInFlight = false;

app.on('before-quit', (event) => {
  if (isQuitting) return; // teardown finished - let the final quit proceed
  // From here on, a window close is a real close (not "hide into the menubar").
  quitCommitted = true;
  // Quitting from menubar-only residence: bring the app back into the Dock first, so the
  // teardown confirmation below can actually be seen.
  void restoreDock();
  if (teardownInFlight) {
    // A second ⌘Q / dock-quit while we're still stopping tasks must NOT bypass the teardown
    // (it would abandon the SIGTERM→grace sequence mid-flight and skip the DB close).
    event.preventDefault();
    return;
  }
  const running = ipcRuntime?.runningTaskCount() ?? 0;
  if (running === 0) {
    // No live tasks - but an in-flight MCP start_app could still be queued (not yet at
    // runner.start(), so it counts as 0). Flag the orchestrator so any such queued start
    // no-ops instead of spawning an orphan after we quit. Then close the MCP listener
    // best-effort (fire-and-forget - the socket also closes when the process exits) and go.
    ipcRuntime?.beginShutdown();
    void ipcRuntime?.disposeServices();
    closeDb();
    return;
  }
  event.preventDefault();
  teardownInFlight = true;
  void (async () => {
    const { response } = await dialog.showMessageBox({
      type: 'warning',
      title: 'Quit NeoHub?',
      message: `${running} running ${running === 1 ? 'task is' : 'tasks are'} still active.`,
      detail: 'NeoHub will stop your running dev servers before quitting.',
      buttons: ['Stop & Quit', 'Cancel'],
      defaultId: 0,
      cancelId: 1
    });
    if (response !== 0) {
      teardownInFlight = false;
      quitCommitted = false; // cancelled - stay open, and closing the window hides again
      return; // cancelled - stay open
    }
    // Close the MCP listener FIRST - before stopAllRunning - so no in-flight tool call can
    // spawn a task that misses the teardown snapshot and survives as an orphan.
    try {
      await ipcRuntime?.disposeServices();
    } catch (e) {
      logger.error('service dispose on quit failed', e);
    }
    try {
      await ipcRuntime?.stopAllRunning();
    } catch (e) {
      logger.error('teardown on quit failed', e);
    }
    closeDb();
    isQuitting = true;
    app.quit();
  })();
});
