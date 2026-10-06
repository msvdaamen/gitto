import { EventEmitter, on } from "node:events";

const DEBOUNCE_MS = 300;
/**
 * A batch is handed over this long after its first change at the latest. Changes that keep coming
 * without a pause, like a build writing its output, would otherwise hold it back until they stop.
 */
const MAX_WAIT_MS = 2000;

/**
 * Collects what the watchers report and hands it over in batches: once it's quiet for a bit, or
 * once the batch has waited long enough.
 */
export class Batches<T> {
  private readonly events = new EventEmitter();
  // Listening from the start, so nothing reported before `stream()` is lost.
  private readonly batches: ReturnType<typeof on>;
  private pending: T[] = [];
  private timer: NodeJS.Timeout | undefined;
  /** When the pending batch is handed over at the latest, on `performance.now()`'s clock. */
  private deadline: number | undefined;
  private closed = false;

  constructor(private readonly signal?: AbortSignal) {
    this.batches = on(this.events, "batch", { signal });
  }

  add(value: T) {
    this.addAll([value]);
  }

  /**
   * Adds `values` at once, as a watcher reports its changes: the batch is put off once for all of
   * them, rather than once per change, of which a build can report thousands at a time.
   */
  addAll(values: readonly T[]) {
    if (this.closed || values.length === 0) return;
    // One by one: spread into `push`, thousands of them would overflow the stack.
    for (const value of values) this.pending.push(value);
    const now = performance.now();
    this.deadline ??= now + MAX_WAIT_MS;
    clearTimeout(this.timer);
    this.timer = setTimeout(
      () => {
        const batch = this.pending;
        this.pending = [];
        this.deadline = undefined;
        this.events.emit("batch", batch);
      },
      Math.min(DEBOUNCE_MS, this.deadline - now),
    );
  }

  /** Ends the stream with `error`, e.g. when there are no file watches left (ENOSPC on Linux). */
  fail(error: Error) {
    if (!this.closed) this.events.emit("error", error);
  }

  async *stream(): AsyncGenerator<T[]> {
    try {
      for await (const [batch] of this.batches) yield batch as T[];
    } catch (error) {
      if (!this.signal?.aborted) throw error;
    }
  }

  close() {
    this.closed = true;
    clearTimeout(this.timer);
    void this.batches.return?.();
  }
}
