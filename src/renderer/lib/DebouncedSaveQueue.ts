/** A panel close flushes both the debounce timer and saves already waiting on IPC. */
export class DebouncedSaveQueue<P extends object> {
  private pending: P | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly inFlight = new Set<Promise<void>>();

  constructor(
    private readonly save: (patch: P) => Promise<void> | void,
    private readonly merge: (pending: P, next: P) => P = (pending, next) => ({ ...pending, ...next })
  ) {}

  queue(patch: P, delay: number): void {
    this.pending = this.pending ? this.merge(this.pending, patch) : patch;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => { void this.flush().catch(() => {}); }, delay);
  }

  async flush(): Promise<void> {
    do {
      clearTimeout(this.timer);
      this.timer = undefined;
      if (this.pending) {
        const patch = this.pending;
        this.pending = undefined;
        let result: Promise<void>;
        try {
          // Send IPC now, preserving ordering with a following task switch/removal.
          result = Promise.resolve(this.save(patch));
        } catch (error) {
          result = Promise.reject(error);
        }
        const saving = result.catch((error: unknown) => {
          this.pending = this.pending ? this.merge(patch, this.pending) : patch;
          throw error;
        });
        this.inFlight.add(saving);
        void saving.finally(() => this.inFlight.delete(saving)).catch(() => {});
      }
      await Promise.all([...this.inFlight]);
      // A user can type again while an earlier IPC request is completing.
    } while (this.pending || this.inFlight.size > 0);
  }
}
