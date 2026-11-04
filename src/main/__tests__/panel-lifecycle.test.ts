import { afterAll, beforeEach, describe, expect, it, vi } from 'vitest';

// Electron owns the native window/process boundary. Keep the real main entry point and
// replace that boundary so these tests exercise its actual event registration and guards.
const fake = await vi.hoisted(async () => {
  const { EventEmitter } = await import('node:events');
  const { mkdtempSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');
  const directory = mkdtempSync(join(tmpdir(), 'neohub-panel-'));
  const handlers = new Map<string, (event: { sender: unknown }, req?: unknown) => unknown>();
  const ipcMain = {
    handle: (channel: string, handler: (event: { sender: unknown }, req?: unknown) => unknown) => handlers.set(channel, handler),
    removeHandler: (channel: string) => handlers.delete(channel)
  };
  const dock = {
    visible: true,
    isVisible: () => dock.visible,
    show: vi.fn(async () => { dock.visible = true; }),
    hide: vi.fn(() => { dock.visible = false; })
  };
  const app = Object.assign(new EventEmitter(), {
    dock,
    getPath: () => directory,
    setPath: vi.fn(), setName: vi.fn(),
    requestSingleInstanceLock: () => true,
    whenReady: () => Promise.resolve(),
    getVersion: () => '1.3.0',
    setAboutPanelOptions: vi.fn(),
    focus: vi.fn(), quit: vi.fn()
  });
  const runtime = {
    trayEnabled: vi.fn(() => true),
    onRendererReload: vi.fn(),
    runningTaskCount: vi.fn(() => 1),
    beginShutdown: vi.fn(),
    disposeServices: vi.fn(async () => {}),
    stopAllRunning: vi.fn(async () => {})
  };
  class Window extends EventEmitter {
    static instances: Window[] = [];
    destroyed = false;
    visible = true;
    webContents = Object.assign(new EventEmitter(), {
      setWindowOpenHandler: vi.fn(), openDevTools: vi.fn(), reload: vi.fn(),
      getURL: () => 'file:///renderer/index.html', send: vi.fn()
    });
    constructor(public options: unknown) { super(); Window.instances.push(this); }
    isDestroyed() { return this.destroyed; }
    isMinimized() { return false; }
    isFullScreen() { return false; }
    restore() {}
    focus() {}
    show() { this.visible = true; }
    hide() { this.visible = false; }
    loadFile() { return Promise.resolve(); }
    getNormalBounds() { return { x: 10, y: 20, width: 1100, height: 750 }; }
    close() {
      let cancelled = false;
      this.emit('close', { preventDefault: () => { cancelled = true; } });
      if (cancelled) return;
      this.destroyed = true;
      this.emit('closed');
    }
  }
  return { directory, app, dock, runtime, Window, ipcMain, handlers,
    invoke: (channel: string, win: Window, req?: unknown) => handlers.get(channel)?.({ sender: win.webContents }, req),
    open: null as null | (() => unknown),
    confirm: vi.fn(async () => ({ response: 1 })), closeDb: vi.fn() };
});

vi.mock('electron', () => ({
  app: fake.app, BrowserWindow: fake.Window, ipcMain: fake.ipcMain,
  screen: { getAllDisplays: () => [{ workArea: { x: 0, y: 0, width: 1920, height: 1080 } }] },
  dialog: { showMessageBox: fake.confirm }, shell: {}
}));
vi.mock('../db/index.js', () => ({ db: () => ({}), closeDb: fake.closeDb }));
vi.mock('../services/AppIdentity.js', () => ({ resolveUserDataDirectory: () => fake.directory }));
vi.mock('../services/Logger.js', () => ({
  installProcessLogging: () => {}, logger: { error: vi.fn() }
}));
vi.mock('../menu.js', () => ({ installAppMenu: () => {} }));
vi.mock('../ipc/index.js', () => ({
  registerAllIpcHandlers: (_win: unknown, open: () => unknown) => {
    fake.open = open;
    return fake.runtime;
  }
}));

beforeEach(async () => {
  vi.resetModules();
  vi.clearAllMocks();
  fake.app.removeAllListeners();
  fake.Window.instances = [];
  fake.handlers.clear();
  fake.dock.visible = true;
  fake.runtime.trayEnabled.mockReturnValue(true);
  await import('../index');
});

afterAll(async () => {
  const { rmSync } = await import('node:fs');
  rmSync(fake.directory, { recursive: true, force: true });
});

describe('panel lifetime', () => {
  it('waits for pending renderer saves before releasing the window', () => {
    const win = fake.Window.instances[0]!;
    fake.invoke('panel:ready', win);
    win.close();
    expect(win.destroyed).toBe(false);
    expect(win.webContents.send).toHaveBeenCalledWith('panel:prepareClose', { requestId: 1 });
    fake.invoke('panel:closeReady', win, { requestId: 1, saved: true });
    expect(win.destroyed).toBe(true);
    expect(fake.dock.visible).toBe(false);
  });

  it('keeps the panel open if a pending save fails, and allows retrying', () => {
    const win = fake.Window.instances[0]!;
    fake.invoke('panel:ready', win);
    win.close();
    fake.invoke('panel:closeReady', win, { requestId: 1, saved: false });
    expect(win.destroyed).toBe(false);
    expect(fake.dock.visible).toBe(true);
    win.close();
    fake.invoke('panel:closeReady', win, { requestId: 2, saved: true });
    expect(win.destroyed).toBe(true);
  });

  it('does not accept stale close acknowledgements after a renderer navigation', () => {
    const win = fake.Window.instances[0]!;
    fake.invoke('panel:ready', win);
    win.close();
    win.webContents.emit('did-start-navigation', {}, 'file:///renderer/index.html', false, true);
    fake.invoke('panel:ready', win);
    win.close();
    fake.invoke('panel:closeReady', win, { requestId: 1, saved: true });
    expect(win.destroyed).toBe(false);
    fake.invoke('panel:closeReady', win, { requestId: 2, saved: true });
    expect(win.destroyed).toBe(true);
  });

  it('releases the window on close without shutting down background services', () => {
    const win = fake.Window.instances[0]!;
    win.close();
    expect(win.destroyed).toBe(true);
    expect(fake.dock.visible).toBe(false);
    expect(fake.runtime.onRendererReload).toHaveBeenCalledOnce();
    expect(fake.runtime.beginShutdown).not.toHaveBeenCalled();
    expect(fake.runtime.disposeServices).not.toHaveBeenCalled();
    expect(fake.runtime.stopAllRunning).not.toHaveBeenCalled();
    expect(fake.closeDb).not.toHaveBeenCalled();
    expect(fake.app.quit).not.toHaveBeenCalled();
  });

  it.each(['tray', 'activate', 'second-instance'])('recreates one panel via %s and restores bounds', async (entry) => {
    const first = fake.Window.instances[0]!;
    first.close();
    const reopen = () => entry === 'tray' ? fake.open!() : fake.app.emit(entry);
    reopen();
    reopen();
    await Promise.resolve();
    expect(fake.Window.instances).toHaveLength(2);
    expect(fake.Window.instances[1]!.options).toMatchObject({
      x: 10, y: 20, width: 1100, height: 750
    });
    expect(fake.dock.visible).toBe(true);
  });

  it('keeps the Dock entry available when the menu bar is disabled', () => {
    fake.runtime.trayEnabled.mockReturnValue(false);
    fake.Window.instances[0]!.close();
    expect(fake.dock.visible).toBe(true);
  });

  it('resumes menu bar residence after a cancelled quit', async () => {
    const event = { preventDefault: vi.fn() };
    fake.app.emit('before-quit', event);
    await Promise.resolve();
    expect(event.preventDefault).toHaveBeenCalled();
    fake.Window.instances[0]!.close();
    expect(fake.Window.instances[0]!.destroyed).toBe(true);
    expect(fake.dock.visible).toBe(false);
    expect(fake.runtime.stopAllRunning).not.toHaveBeenCalled();
  });

  it('keeps the Dock hidden when quitting from a closed panel is cancelled', async () => {
    fake.Window.instances[0]!.close();
    fake.app.emit('before-quit', { preventDefault: vi.fn() });
    await Promise.resolve();
    await Promise.resolve();
    expect(fake.dock.visible).toBe(false);
  });

  it('still flushes saves after a temporarily unresponsive renderer recovers', () => {
    const win = fake.Window.instances[0]!;
    fake.invoke('panel:ready', win);
    win.emit('unresponsive');
    win.emit('responsive');
    win.close();
    expect(win.destroyed).toBe(false);
    expect(win.webContents.send).toHaveBeenCalledWith('panel:prepareClose', { requestId: 1 });
  });
});
