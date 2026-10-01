import { EventEmitter, on } from "node:events";
import { realpath } from "node:fs/promises";
import { basename, dirname, isAbsolute, relative, resolve, sep } from "node:path";

import { subscribe, type AsyncSubscription } from "@parcel/watcher";

import type { Repo } from "../../core/repo";
import type { GitDirChange } from "./schema";

const DEBOUNCE_MS = 300;

/**
 * Folders in the git directory that are big or churn a lot, but never change what the UI shows.
 * `info` (with the exclude rules) is `watchWorkingTree`'s; the two never watch the same folder.
 */
const GIT_DIR_IGNORED = ["objects", "logs", "hooks", "lfs", "modules", "info"];

/**
 * Files in a worktree's own git directory: HEAD (and MERGE_HEAD etc.), operations in progress, and
 * its refs when they're stored in a reftable (`git init --ref-format=reftable`).
 */
const WORKTREE_STATE = /^([A-Z_]*HEAD|(rebase-merge|rebase-apply|sequencer|reftable)(\/.*)?)$/;
/**
 * Files in the shared git directory: branches, tags and remotes (as files, packed, or in a
 * reftable), and the config (upstreams).
 */
const SHARED_STATE = /^((refs|reftable)(\/.*)?|packed-refs|config)$/;

/** Yields what changed in the repository's git directory (debounced), until `signal` aborts. */
export async function* watchGitDir(
  repo: Repo,
  signal?: AbortSignal,
): AsyncGenerator<GitDirChange[]> {
  if (signal?.aborted) return;
  const dirs = await gitDirs(repo);
  // Checked again: `Batches` throws if it's given a signal that's already aborted.
  if (signal?.aborted) return;
  // A linked worktree has its own git directory (inside the shared one) for HEAD and the index.
  const roots =
    dirs.gitDir === dirs.commonDir || inside(dirs.commonDir, dirs.gitDir) !== undefined
      ? [dirs.commonDir]
      : [dirs.commonDir, dirs.gitDir];

  const changes = new Batches<GitDirChange>(signal);
  const subscriptions: AsyncSubscription[] = [];
  try {
    const subscribed = await Promise.allSettled(
      roots.map((root) =>
        subscribe(
          root,
          (error, events) => {
            if (error) return changes.fail(error);
            for (const event of events) {
              const change = classify(dirs, event.path);
              if (change) changes.add(change);
            }
          },
          { ignore: GIT_DIR_IGNORED },
        ),
      ),
    );
    // All the subscriptions that worked are kept (and so unsubscribed below), even if one failed.
    for (const result of subscribed) {
      if (result.status === "fulfilled") subscriptions.push(result.value);
    }
    const failed = subscribed.find((result) => result.status === "rejected");
    if (failed) throw failed.reason;
    for await (const batch of changes.stream()) yield [...new Set(batch)];
  } finally {
    changes.close();
    await Promise.all(subscriptions.map((subscription) => subscription.unsubscribe()));
  }
}

/** Yields (debounced) whenever a file in the working tree changes, until `signal` aborts. */
export async function* watchWorkingTree(repo: Repo, signal?: AbortSignal): AsyncGenerator<null> {
  if (signal?.aborted) return;
  const [root, dirs] = await Promise.all([realpath(repo.path), gitDirs(repo)]);
  // Checked again: `Batches` throws if it's given a signal that's already aborted.
  if (signal?.aborted) return;

  // Whether the ignore rules changed.
  const changes = new Batches<boolean>(signal);
  let tree: AsyncSubscription | undefined;
  let excludes: AsyncSubscription | undefined;
  let closed = false;

  // Everything git ignores (e.g. `node_modules`) is left out, so it's never walked or watched, and
  // so is the git directory: that's `watchGitDir`'s. The ignored paths are a snapshot, so this
  // starts over whenever the ignore rules change.
  async function watchTree() {
    const ignore = [dirs.gitDir, ...(await ignoredPaths(repo))];
    // Stopped first: the watcher reuses what an existing subscription on the folder already walked,
    // which would leave out a folder that's no longer ignored. Changes in between are reported
    // anyway, as the ignore rules changing is one.
    await tree?.unsubscribe();
    tree = undefined;
    if (closed) return;
    // A new callback each time: the watcher shares subscriptions with the same callback and folder.
    const next = await subscribe(
      root,
      (error, events) => {
        if (error) return changes.fail(error);
        changes.add(events.some((event) => basename(event.path) === ".gitignore"));
      },
      { ignore },
    );
    if (closed) return next.unsubscribe();
    tree = next;
  }

  try {
    await watchTree();
    // `.git/info/exclude` has ignore rules too. Fine to miss if `info` doesn't exist.
    excludes = await subscribe(dirname(dirs.excludeFile), (error, events) => {
      if (!error && events.some((event) => basename(event.path) === basename(dirs.excludeFile))) {
        changes.add(true);
      }
    }).catch(() => undefined);

    for await (const batch of changes.stream()) {
      if (batch.includes(true)) await watchTree();
      yield null;
    }
  } finally {
    closed = true;
    changes.close();
    await Promise.all([tree?.unsubscribe(), excludes?.unsubscribe()]);
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

interface GitDirs {
  /** This worktree's git directory: HEAD, the index. */
  gitDir: string;
  /** The git directory shared by all worktrees: refs, config. The same as `gitDir`, usually. */
  commonDir: string;
  excludeFile: string;
}

/** The repository's git directories, symlinks resolved (that's how the watcher reports paths). */
async function gitDirs(repo: Repo): Promise<GitDirs> {
  // Not `--path-format=absolute`, which needs git 2.31: the other two can be relative to the
  // repository, so they're resolved here.
  const output = await repo.read([
    "rev-parse",
    "--absolute-git-dir",
    "--git-common-dir",
    "--git-path",
    "info/exclude",
  ]);
  const [gitDir = "", commonDir = "", excludeFile = ""] = output.split("\n");
  return {
    gitDir: await realpath(gitDir),
    commonDir: await realpath(resolve(repo.path, commonDir)),
    excludeFile: resolve(repo.path, excludeFile),
  };
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
