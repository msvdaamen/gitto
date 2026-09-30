import { EventEmitter, on } from "node:events";
import { watch, type FSWatcher } from "node:fs";
import { basename, join, sep } from "node:path";

import type { Repo } from "../../core/repo";

const DEBOUNCE_MS = 300;

/** Yields (debounced) whenever something in the repository changes, until `signal` aborts. */
export async function* watchChanges(repo: Repo, signal?: AbortSignal): AsyncGenerator<null> {
  const changes = new EventEmitter();
  let timer: NodeJS.Timeout | undefined;
  let watcher: FSWatcher | undefined;
  let ignoreRulesChanged = false;
  let closed = false;

  // On Linux, Node sets up a recursive watch by walking the whole tree synchronously, so leave out
  // everything git ignores: walking `node_modules` blocked the main process for hundreds of ms.
  // The ignored paths are a snapshot, so this starts over whenever the ignore rules change.
  async function start() {
    const ignored = await ignoredPaths(repo);
    if (closed) return;
    watcher?.close();
    watcher = watch(repo.path, { recursive: true, ignore: ignored }, (_event, filename) => {
      if (filename && isNoise(filename)) return;
      if (filename && isIgnoreRules(filename)) ignoreRulesChanged = true;
      clearTimeout(timer);
      timer = setTimeout(() => void changed(), DEBOUNCE_MS);
    });
  }

  async function changed() {
    if (ignoreRulesChanged) {
      ignoreRulesChanged = false;
      await start().catch(() => undefined);
    }
    changes.emit("change");
  }

  await start();
  try {
    for await (const _ of on(changes, "change", { signal })) yield null;
  } catch (error) {
    if (!signal?.aborted) throw error;
  } finally {
    closed = true;
    clearTimeout(timer);
    watcher?.close();
  }
}

/**
 * What git ignores right now, e.g. `node_modules` or build output: whole directories (listed once,
 * with --directory) and single files. Relative to the repository, `/`-separated.
 */
async function ignoredPaths(repo: Repo): Promise<string[]> {
  const output = await repo
    .read(["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"])
    .catch(() => "");
  return output
    .split("\0")
    .filter(Boolean)
    .map((path) => path.replace(/\/$/, ""));
}

/** Files with the repository's ignore rules: any `.gitignore`, and `.git/info/exclude`. */
function isIgnoreRules(filename: string): boolean {
  return basename(filename) === ".gitignore" || filename === join(".git", "info", "exclude");
}
/** Paths whose changes never affect what the UI shows, but churn a lot. */
function isNoise(filename: string): boolean {
  const segments = filename.split(sep);
  return (segments[0] === ".git" && segments[1] === "objects") || filename.endsWith(".lock");
}
