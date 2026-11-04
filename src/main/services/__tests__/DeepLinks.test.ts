import { describe, expect, it, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import { DeepLinks } from '../DeepLinks';
import type { AppRegistry } from '../AppRegistry';
import type { AppOrchestrator } from '../AppOrchestrator';
import type { BrowserWindow } from 'electron';

vi.mock('electron', () => ({
  app: Object.assign(new EventEmitter(), { setAsDefaultProtocolClient: () => {} })
}));

describe('deep links with the panel closed', () => {
  it('opens a panel and waits for its renderer before delivering the action', async () => {
    const send = vi.fn();
    const win = { isDestroyed: () => false, webContents: { send } } as unknown as BrowserWindow;
    let ready!: (value: boolean) => void;
    const loading = new Promise<boolean>((resolve) => { ready = resolve; });
    let current: BrowserWindow | null = null;
    const links = new DeepLinks(
      { get: () => ({ id: 'project-a', name: 'Project A' }) } as unknown as AppRegistry,
      {} as AppOrchestrator,
      () => { current = win; return loading.then(() => {}); },
      () => current
    );
    const opening = links.handle('devharbor://start?id=project-a');
    expect(send).not.toHaveBeenCalled();
    ready(true);
    await opening;
    expect(send).toHaveBeenCalledWith('deepLink:focusApp', { appId: 'project-a' });
    expect(send).toHaveBeenCalledWith('deepLink:confirmStart', { appId: 'project-a', appName: 'Project A' });
  });
});
