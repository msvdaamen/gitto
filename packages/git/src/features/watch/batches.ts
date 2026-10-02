import { EventEmitter, on } from "node:events";

const DEBOUNCE_MS = 300;

/** Collects what the watchers report and hands it over in batches, once it's quiet for a bit. */
export class Batches<T> {
  private readonly events = new EventEmitter();
  // Listening from the start, so nothing reported before `stream()` is lost.
  private readonly batches: ReturnType<typeof on>;
  private pending: T[] = [];
  private timer: NodeJS.Timeout | undefined;
  private closed = false;

  constructor(private readonly signal?: AbortSignal) {
    this.batches = on(this.events, "batch", { signal });
  }

  add(value: T) {
    if (this.closed) return;
    this.pending.push(value);
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      const batch = this.pending;
      this.pending = [];
      this.events.emit("batch", batch);
    }, DEBOUNCE_MS);
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
