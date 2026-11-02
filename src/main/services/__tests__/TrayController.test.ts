import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppId, ProcessState } from '@shared/types';

/**
 * TrayController drives real Electron objects, so the module is mocked. The tests pin the
 * menubar-only contract: left-click is the way back into the window (the app keeps running
 * with its window closed), and the full control menu is on right-click.
 */
const mocks = vi.hoisted(() => {
  interface FakeMenuItem {
    label?: string;
    click?: () => void;
    submenu?: FakeMenuItem[];
    type?: string;
    enabled?: boolean;
  }

  const instances: FakeTray[] = [];
  const state = { failCreate: false };

  class FakeTray {
    handlers = new Map<string, () => void>();
    tooltip = '';
    image: unknown = null;
    contextMenu: unknown = null;
    contextMenuSetVia = '';
    popUpCalls: unknown[] = [];
    destroyed = false;

    constructor(image: unknown) {
      if (state.failCreate) throw new Error('no display');
      this.image = image;
      instances.push(this);
    }
    setToolTip(t: string) {
      this.tooltip = t;
    }
    setImage(i: unknown) {
      this.image = i;
    }
    /** Real Electron would bind the menu to every left-click - the app must not do this. */
    setContextMenu(m: unknown) {
      this.contextMenu = m;
      this.contextMenuSetVia = 'setContextMenu';
    }
    popUpContextMenu(m: unknown) {
      this.popUpCalls.push(m);
    }
    on(event: string, fn: () => void) {
      this.handlers.set(event, fn);
    }
    emit(event: string) {
      this.handlers.get(event)?.();
    }
    destroy() {
      this.destroyed = true;
    }
  }

  return {
    instances,
    state,
    FakeTray,
    buildFromTemplate: (template: FakeMenuItem[]) => ({ template }),
    createFromBuffer: () => ({ setTemplateImage: () => {} })
  };
});

vi.mock('electron', () => ({
  Tray: mocks.FakeTray,
  Menu: { buildFromTemplate: mocks.buildFromTemplate },
  nativeImage: { createFromBuffer: mocks.createFromBuffer }
}));

import { TrayController } from '../TrayController';

const APP = 'app-1' as AppId;

function makeCallbacks(state: ProcessState = 'running') {
  return {
    listApps: () => [{ id: APP, name: 'Docs', state, ports: [3000] }],
    start: vi.fn(),
    stop: vi.fn(),
    stopAll: vi.fn(),
    open: vi.fn(),
    quit: vi.fn()
  };
}

function menuTemplate(tray: InstanceType<typeof mocks.FakeTray>) {
  const menu = tray.popUpCalls.at(-1) as { template: Array<Record<string, unknown>> } | undefined;
  return menu?.template ?? [];
}

beforeEach(() => {
  mocks.instances.length = 0;
});

describe('TrayController', () => {
  it('left-click reopens the window instead of opening the menu', () => {
    const cb = makeCallbacks();
    const tray = new TrayController(cb);
    tray.enable();

    const inst = mocks.instances[0]!;
    expect(tray.enabled).toBe(true);
    // No setContextMenu: that would make every left-click open the menu, not the window.
    expect(inst.contextMenu).toBeNull();

    inst.emit('click');
    expect(cb.open).toHaveBeenCalledTimes(1);
    expect(inst.popUpCalls).toHaveLength(0);
  });

  it('right-click pops the control menu with per-app start/stop and quit', () => {
    const cb = makeCallbacks('running');
    const tray = new TrayController(cb);
    tray.enable();
    const inst = mocks.instances[0]!;

    inst.emit('right-click');
    const template = menuTemplate(inst);
    const labels = template.map((i) => i.label ?? i.type);

    expect(labels).toContain('Open NeoHub');
    expect(labels).toContain('Stop all');
    expect(labels).toContain('Quit NeoHub');
    expect(labels.some((l) => String(l).includes('Docs'))).toBe(true);

    // The per-app item stops a live app and the quit item quits.
    const appItem = template.find((i) => String(i.label).includes('Docs')) as {
      submenu: Array<{ label: string; click: () => void }>;
    };
    expect(appItem.submenu[0]!.label).toBe('Stop');
    appItem.submenu[0]!.click();
    expect(cb.stop).toHaveBeenCalledWith(APP);

    (template.find((i) => i.label === 'Quit NeoHub') as { click: () => void }).click();
    expect(cb.quit).toHaveBeenCalledTimes(1);
  });

  it('a stopped app is started from the menu', () => {
    const cb = makeCallbacks('idle');
    const tray = new TrayController(cb);
    tray.enable();
    const inst = mocks.instances[0]!;

    inst.emit('right-click');
    const appItem = menuTemplate(inst).find((i) => String(i.label).includes('Docs')) as {
      submenu: Array<{ label: string; click: () => void }>;
    };
    expect(appItem.submenu[0]!.label).toBe('Start');
    appItem.submenu[0]!.click();
    expect(cb.start).toHaveBeenCalledWith(APP);
  });

  it('disable() drops the tray and enabled reflects it', () => {
    const tray = new TrayController(makeCallbacks());
    tray.enable();
    const inst = mocks.instances[0]!;
    tray.disable();
    expect(tray.enabled).toBe(false);
    expect(inst.destroyed).toBe(true);
  });

  it('survives a tray that cannot be created', () => {
    const tray = new TrayController(makeCallbacks());
    mocks.state.failCreate = true;
    try {
      tray.enable();
      expect(tray.enabled).toBe(false);
      expect(() => tray.refresh()).not.toThrow();
    } finally {
      mocks.state.failCreate = false;
    }
  });
});
