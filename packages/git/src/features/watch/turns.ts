type Turn = "subscribe" | "unsubscribe";

/**
 * Runs tasks of one kind together, and the other kind once those are done. A subscribe goes ahead
 * of the unsubscribes that are waiting: a repository that's just been switched to isn't watched
 * until it's done, so it doesn't wait for one that's being left to finish walking its folders.
 */
export class Turns {
  private running: Turn | undefined;
  private count = 0;
  private readonly waiting: { kind: Turn; start: () => void }[] = [];

  take<T>(kind: Turn, task: () => Promise<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      const start = () => {
        this.running = kind;
        this.count++;
        // Also if it throws rather than rejects.
        void new Promise<T>((done) => done(task())).then(resolve, reject).finally(() => {
          if (--this.count === 0) this.next();
        });
      };
      // Along with what's running, if that's of its kind and no subscribe is waiting for it.
      const joins =
        this.running === kind &&
        (kind === "subscribe" || !this.waiting.some((other) => other.kind === "subscribe"));
      if (this.count === 0 || joins) start();
      else this.waiting.push({ kind, start });
    });
  }

  /** Starts what's waiting of one kind: the subscribes, if there are any. */
  private next(): void {
    this.running = undefined;
    const kind = this.waiting.some((other) => other.kind === "subscribe")
      ? "subscribe"
      : "unsubscribe";
    const starting = this.waiting.filter((other) => other.kind === kind);
    for (const other of starting) this.waiting.splice(this.waiting.indexOf(other), 1);
    for (const other of starting) other.start();
  }
}
