import { expect, it, vi } from 'vitest';
import type { BrowserWindow } from 'electron';

const fake = await vi.hoisted(async () => {
  const { EventEmitter } = await import('node:events');
  return { updater: new EventEmitter() };
});
vi.mock('electron', () => ({}));
vi.mock('electron-updater', () => ({ autoUpdater: fake.updater }));
import { Updater } from '../Updater';

it('restores a downloaded update after reopening a panel', () => {
  let win: BrowserWindow | null = null;
  const updater = new Updater(() => win);
  fake.updater.emit('update-downloaded', { version: '2.0.0', releaseNotes: 'Changes' });
  const send = vi.fn();
  win = { webContents: { send } } as unknown as BrowserWindow;
  updater.replayToPanel();
  expect(send).toHaveBeenCalledWith('update:ready', { version: '2.0.0', releaseNotes: 'Changes' });
});
