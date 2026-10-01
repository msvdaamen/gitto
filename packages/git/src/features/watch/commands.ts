import { EventEmitter, on } from "node:events";
import { watch, type FSWatcher } from "node:fs";
import { isAbsolute, join, relative, sep } from "node:path";

import type { Repo } from "../../core/repo";
import type { GitDirChange } from "./schema";

const DEBOUNCE_MS = 300;

/** Files in a worktree's own git directory: HEAD (and MERGE_HEAD etc.), operations in progress. */
const WORKTREE_STATE = /^([A-Z_]*HEAD|rebase-merge|rebase-apply|sequencer)$/;
/** Files in the shared git directory: branches, tags and remotes, and the config (upstreams). */
const SHARED_STATE = /^(refs(\/.*)?|packed-refs|config)$/;

/**
 * Yields what changed in the repository's git directory (debounced), until `signal` aborts.
 *
 * Only the top of the git directory (HEAD, the index, packed-refs...) and `refs` are watched: a
 * handful of folders, whatever the size of the repository. Not `objects`, which is big and churns.
 */
export async function* watchGitDir(
  repo: Repo,
  signal?: AbortSignal,
): AsyncGenerator<GitDirChange[]> {
  if (signal?.aborted) return;
  const dirs = await gitDirs(repo);

  const changes = new Batches<GitDirChange>(signal);
  const watchers: FSWatcher[] = [];
  function follow(dir: string, recursive: boolean) {
    const watcher = watch(dir, { recursive }, (_event, filename) => {
      // Some platforms don't always say which file changed; assume the worst.
      const change = filename ? classify(dirs, join(dir, filename)) : "refs";
      if (change) changes.add(change);
    });
    watcher.on("error", (error) => changes.fail(error));
    watchers.push(watcher);
  }

  try {
    follow(dirs.gitDir, false);
    // A linked worktree has a git directory of its own for HEAD and the index; the rest is shared.
    if (dirs.commonDir !== dirs.gitDir) follow(dirs.commonDir, false);
    follow(join(dirs.commonDir, "refs"), true);

    for await (const batch of changes.stream()) yield [...new Set(batch)];
  } finally {
    changes.close();
    for (const watcher of watchers) watcher.close();
  }
}

/** Collects what the watchers report and hands it over in batches, once it's quiet for a bit. */
class Batches<T> {
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

  /** Ends the stream with `error`, e.g. when the repository is deleted. */
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

interface GitDirs {
  /** This worktree's git directory: HEAD, the index. */
  gitDir: string;
  /** The git directory shared by all worktrees: refs, config. The same as `gitDir`, usually. */
  commonDir: string;
}

async function gitDirs(repo: Repo): Promise<GitDirs> {
  const output = await repo.read([
    "rev-parse",
    "--path-format=absolute",
    "--git-dir",
    "--git-common-dir",
  ]);
  const [gitDir = "", commonDir = ""] = output.split("\n");
  return { gitDir, commonDir };
}

/** What a change to `path`, in the git directory, means for the UI; `undefined` if nothing. */
function classify(dirs: GitDirs, path: string): GitDirChange | undefined {
  // Written to while git works, then renamed onto the real file; that's the change that counts.
  if (path.endsWith(".lock")) return undefined;

  const own = inside(dirs.gitDir, path);
  if (own === "index") return "index";
  if (own !== undefined && WORKTREE_STATE.test(own)) return "refs";

  const shared = inside(dirs.commonDir, path);
  if (shared !== undefined && SHARED_STATE.test(shared)) return "refs";
  return undefined;
}

/** `path` relative to `dir` and `/`-separated, or `undefined` if it's not inside `dir`. */
function inside(dir: string, path: string): string | undefined {
  const rel = relative(dir, path);
  if (!rel || rel === ".." || rel.startsWith(`..${sep}`) || isAbsolute(rel)) return undefined;
  return rel.split(sep).join("/");
}
