import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { App, AppId, Task, TaskId } from '@shared/types';

const APP = 'project-a' as AppId;
const TASK = 'task-b' as TaskId;
const apps = [{ id: APP }] as App[];

beforeEach(() => {
  vi.resetModules();
  const values = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: vi.fn((key: string, value: string) => values.set(key, value))
  });
});

describe('panel session', () => {
  it('restores navigation in a fresh renderer without restoring stale runtime data', async () => {
    const { useStore } = await import('../store');
    useStore.getState().setApps(apps);
    useStore.getState().setSelected(APP);
    useStore.getState().selectTaskTab(APP, TASK);
    useStore.getState().applyTaskStatus(TASK, APP, 'running', true);
    vi.resetModules();
    const { useStore: reopened } = await import('../store');
    expect(reopened.getState()).toMatchObject({
      view: 'app', selectedAppId: APP, selectedTaskByApp: { [APP]: TASK },
      loaded: false, taskState: {}, runningTasks: {}
    });
    reopened.getState().setApps(apps);
    reopened.getState().setTasksForApp(APP, [{ id: 'task-a' }, { id: TASK }] as Task[]);
    expect(reopened.getState().selectedTaskByApp[APP]).toBe(TASK);
  });

  it('falls back to the dashboard when the selected project was removed', async () => {
    const { useStore } = await import('../store');
    useStore.getState().setSelected(APP);
    vi.resetModules();
    const { useStore: reopened } = await import('../store');
    reopened.getState().setApps([{ id: 'other' }] as App[]);
    expect(reopened.getState().view).toBe('dashboard');
    expect(reopened.getState().selectedAppId).toBe('other');
  });

  it('falls back to an existing task when the saved tab was removed', async () => {
    const { useStore } = await import('../store');
    useStore.getState().setSelected(APP);
    useStore.getState().selectTaskTab(APP, TASK);
    vi.resetModules();
    const { useStore: reopened } = await import('../store');
    reopened.getState().setApps(apps);
    reopened.getState().setTasksForApp(APP, [{ id: 'remaining-task' }] as Task[]);
    expect(reopened.getState().selectedTaskByApp[APP]).toBe('remaining-task');
  });

  it('ignores malformed navigation and unavailable storage', async () => {
    vi.stubGlobal('localStorage', { getItem: () => '{broken', setItem: () => { throw Error('denied'); } });
    const { useStore } = await import('../store');
    expect(useStore.getState().view).toBe('dashboard');
    expect(() => useStore.getState().setSelected(APP)).not.toThrow();
  });

  it('does not write storage for streaming task stats', async () => {
    const { useStore } = await import('../store');
    useStore.getState().setSelected(APP);
    vi.mocked(localStorage.setItem).mockClear();
    useStore.getState().applyTaskStats(TASK, 5, 120);
    useStore.getState().applyTaskPorts(TASK, [3000]);
    expect(localStorage.setItem).not.toHaveBeenCalled();
  });
});
