import type { BrowserWindow } from 'electron';

export interface PanelWindow {
  win: BrowserWindow;
  created: boolean;
  /** Resolves when React has installed its IPC listeners, or false if closed before that. */
  ready: Promise<boolean>;
}
