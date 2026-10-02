import { realpath } from "node:fs/promises";
import { basename, dirname } from "node:path";

import { subscribe, type AsyncSubscription } from "@parcel/watcher";

import type { Repo } from "../../core/repo";
import { Batches } from "./batches";
import { classify, gitDirs, inside } from "./git-dirs";
import type { GitDirChange } from "./schema";

/**
 * Folders in the git directory that are big or churn a lot, but never change what the UI shows.
 * `info` (with the exclude rules) is `watchWorkingTree`'s; the two never watch the same folder.
 */
const GIT_DIR_IGNORED = ["objects", "logs", "hooks", "lfs", "modules", "info"];

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
