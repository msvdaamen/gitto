import { EventEmitter, on } from "node:events";
import { watch } from "node:fs";
import { sep } from "node:path";

import type { Repo } from "../../core/repo";

const DEBOUNCE_MS = 300;

/** Yields (debounced) whenever something in the repository changes, until `signal` aborts. */
export async function* watchChanges(repo: Repo, signal?: AbortSignal): AsyncGenerator<null> {
  const changes = new EventEmitter();
  let timer: NodeJS.Timeout | undefined;

  const watcher = watch(repo.path, { recursive: true }, (_event, filename) => {
    if (filename && isNoise(filename)) return;
    clearTimeout(timer);
    timer = setTimeout(() => changes.emit("change"), DEBOUNCE_MS);
  });

  try {
    for await (const _ of on(changes, "change", { signal })) yield null;
  } catch (error) {
    if (!signal?.aborted) throw error;
  } finally {
    clearTimeout(timer);
    watcher.close();
  }
}

/** Paths whose changes never affect what the UI shows, but churn a lot. */
function isNoise(filename: string): boolean {
  const segments = filename.split(sep);
  return (
    segments.includes("node_modules") ||
    (segments[0] === ".git" && segments[1] === "objects") ||
    filename.endsWith(".lock")
  );
}
