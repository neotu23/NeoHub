import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DebouncedSaveQueue } from '../DebouncedSaveQueue';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('debounced saves before panel close', () => {
  it('sends a flush before a following destructive IPC operation', async () => {
    const order: string[] = [];
    const queue = new DebouncedSaveQueue<object>(() => { order.push('save'); });
    queue.queue({ name: 'edited' }, 300);
    const saving = queue.flush();
    order.push('remove');
    await saving;
    expect(order).toEqual(['save', 'remove']);
  });

  it('drains new edits queued while an earlier save is still in flight', async () => {
    const saved: object[] = [];
    let finish!: () => void;
    const queue = new DebouncedSaveQueue<{ name: string }>((patch) => {
      saved.push(patch);
      if (patch.name === 'first') return new Promise<void>((resolve) => { finish = resolve; });
    });
    queue.queue({ name: 'first' }, 300);
    const closing = queue.flush();
    await Promise.resolve();
    queue.queue({ name: 'second' }, 300);
    finish();
    await closing;
    expect(saved).toEqual([{ name: 'first' }, { name: 'second' }]);
  });

  it('saves the merged patch immediately on flush and cancels the timer', async () => {
    const saved: object[] = [];
    const queue = new DebouncedSaveQueue<{ name?: string; script?: string }>((patch) => { saved.push(patch); });
    queue.queue({ name: 'new name' }, 300);
    queue.queue({ script: 'dev' }, 300);
    await queue.flush();
    await vi.runAllTimersAsync();
    expect(saved).toEqual([{ name: 'new name', script: 'dev' }]);
  });

  it('waits for an already running save before allowing the panel to close', async () => {
    let finish!: () => void;
    const queue = new DebouncedSaveQueue<object>(() => new Promise<void>((resolve) => { finish = resolve; }));
    queue.queue({ name: 'pending' }, 300);
    await vi.advanceTimersByTimeAsync(300);
    let flushed = false;
    const closing = queue.flush().then(() => { flushed = true; });
    await Promise.resolve();
    expect(flushed).toBe(false);
    finish();
    await closing;
    expect(flushed).toBe(true);
  });

  it('retains a failed nested patch for a second close attempt', async () => {
    const saved: object[] = [];
    let fail = true;
    type Edit = { id: string; patch: { name?: string; script?: string } };
    const queue = new DebouncedSaveQueue<Edit>((edit) => {
      if (fail) throw new Error('database unavailable');
      saved.push(edit);
    }, (pending, next) => pending.id === next.id
      ? { id: next.id, patch: { ...pending.patch, ...next.patch } }
      : next);
    queue.queue({ id: 'task-a', patch: { name: 'keep this edit' } }, 300);
    await expect(queue.flush()).rejects.toThrow('database unavailable');
    fail = false;
    await queue.flush();
    expect(saved).toEqual([{ id: 'task-a', patch: { name: 'keep this edit' } }]);
  });

  it('preserves newer fields when retrying a failed save', async () => {
    const saved: object[] = [];
    let reject!: (error: Error) => void;
    let fail = true;
    const queue = new DebouncedSaveQueue<{ name?: string; script?: string }>((patch) => {
      if (fail) return new Promise<void>((_resolve, rejectSave) => { reject = rejectSave; });
      saved.push(patch);
    });
    queue.queue({ name: 'old', script: 'dev' }, 300);
    const saving = queue.flush();
    await Promise.resolve();
    queue.queue({ name: 'new' }, 300);
    reject(new Error('failed'));
    await expect(saving).rejects.toThrow('failed');
    fail = false;
    await queue.flush();
    expect(saved).toEqual([{ name: 'new', script: 'dev' }]);
  });
});
