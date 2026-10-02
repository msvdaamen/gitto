import { EventEmitter, on } from "node:events";

const DEBOUNCE_MS = 300;
/**
 * How long a batch is held back at most. Something that never stops writing, like a build or a
 * log file, would otherwise keep postponing it (and growing it) for as long as it runs.
 */
const MAX_WAIT_MS = 2000;

/**
 * Collects what the watchers report and hands it over in batches, each value once: when it's been
 * quiet for a bit, or the batch has waited long enough.
 */
export class Batches<T> {
  private readonly events = new EventEmitter();
  // Listening from the start, so nothing reported before `stream()` is lost.
  private readonly batches: ReturnType<typeof on>;
  private pending = new Set<T>();
  /** When the pending batch got its first value. */
  private since = 0;
  private timer: NodeJS.Timeout | undefined;
  private closed = false;

  constructor(private readonly signal?: AbortSignal) {
    this.batches = on(this.events, "batch", { signal });
  }

  add(value: T) {
    if (this.closed) return;
    const now = performance.now();
    if (this.pending.size === 0) this.since = now;
    this.pending.add(value);
    clearTimeout(this.timer);
    const wait = Math.min(DEBOUNCE_MS, Math.max(0, this.since + MAX_WAIT_MS - now));
    this.timer = setTimeout(() => {
      const batch = [...this.pending];
      this.pending = new Set();
      this.events.emit("batch", batch);
    }, wait);
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
