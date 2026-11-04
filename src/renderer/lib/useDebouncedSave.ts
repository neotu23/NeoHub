import { useCallback, useEffect, useRef } from 'react';
import { registerPanelSave } from './panelSaves';
import { DebouncedSaveQueue } from './DebouncedSaveQueue';

/**
 * Coalesce a stream of partial saves into one debounced call.
 *
 * - Calls accumulate via Object.assign (shallow merge). Pass `merge` when the payload
 *   nests the actual patch (e.g. { id, patch }) - a shallow merge would replace the
 *   whole nested object and drop fields queued earlier in the same window.
 * - The save function fires `delay` ms after the last call.
 * - Pending changes are flushed on unmount.
 * - Returned `flush()` lets the caller force an immediate save (e.g. before switching contexts).
 */
export function useDebouncedSave<P extends object>(
  save: (patch: P) => Promise<void> | void,
  delay = 300,
  merge?: (pending: P, next: P) => P
): { queue: (patch: P) => void; flush: () => Promise<void> } {
  const saveRef = useRef(save);
  const mergeRef = useRef(merge);
  mergeRef.current = merge;
  const queueRef = useRef<DebouncedSaveQueue<P> | null>(null);
  if (queueRef.current === null) {
    queueRef.current = new DebouncedSaveQueue(
      (patch) => saveRef.current(patch),
      (pending, next) => mergeRef.current
        ? mergeRef.current(pending, next)
        : { ...pending, ...next }
    );
  }
  useEffect(() => {
    saveRef.current = save;
  }, [save]);

  const flush = useCallback((): Promise<void> => queueRef.current!.flush(), []);

  const queue = useCallback(
    (patch: P): void => {
      queueRef.current!.queue(patch, delay);
    },
    [delay]
  );

  useEffect(() => {
    const unregister = registerPanelSave(flush);
    return () => {
      unregister();
      void flush().catch(() => {});
    };
  }, [flush]);

  return { queue, flush };
}
