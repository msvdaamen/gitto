import { realpath } from "node:fs/promises";
import { basename, dirname, relative, sep } from "node:path";

import { subscribe, type AsyncSubscription } from "@parcel/watcher";

import type { Repo } from "../../core/repo";
import { trace } from "../../core/trace";
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
    for await (const batch of changes.stream()) {
      const unique = [...new Set(batch)];
      trace(`git dir changed in ${repo.path}: ${unique.join(", ")}`);
      yield unique;
    }
  } finally {
    changes.close();
    await Promise.all(subscriptions.map((subscription) => subscription.unsubscribe()));
  }
}

/** A changed path in the working tree (relative, `/`-separated), or `.git/info/exclude` changing. */
type TreeChange = string | typeof EXCLUDES;
const EXCLUDES = Symbol("excludes");

/** Yields (debounced) whenever a file in the working tree changes, until `signal` aborts. */
export async function* watchWorkingTree(repo: Repo, signal?: AbortSignal): AsyncGenerator<null> {
  if (signal?.aborted) return;
  const [root, dirs] = await Promise.all([realpath(repo.path), gitDirs(repo)]);
  // Checked again: `Batches` throws if it's given a signal that's already aborted.
  if (signal?.aborted) return;

  const changes = new Batches<TreeChange>(signal);
  let tree: AsyncSubscription | undefined;
  let excludes: AsyncSubscription | undefined;
  let closed = false;
  // What the current subscription leaves out.
  let ignored = new Set<string>();

  // Everything git ignores (e.g. `node_modules`) is left out, so it's never walked or watched, and
  // so is the git directory: that's `watchGitDir`'s. The ignored paths are a snapshot, so this
  // starts over whenever the ignore rules change.
  async function watchTree(paths?: string[]) {
    const nowIgnored = paths ?? (await ignoredPaths(repo));
    const start = performance.now();
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
        for (const event of events) {
          const path = relative(root, event.path).split(sep).join("/");
          if (path) changes.add(path);
        }
      },
      { ignore: [dirs.gitDir, ...nowIgnored] },
    );
    const ms = (performance.now() - start).toFixed(0);
    trace(`watching ${root} (${nowIgnored.length} ignored paths, set up in ${ms}ms)`);
    if (closed) return next.unsubscribe();
    tree = next;
    ignored = new Set(nowIgnored);
  }

  try {
    await watchTree();
    // `.git/info/exclude` has ignore rules too. Fine to miss if `info` doesn't exist.
    excludes = await subscribe(dirname(dirs.excludeFile), (error, events) => {
      if (!error && events.some((event) => basename(event.path) === basename(dirs.excludeFile))) {
        changes.add(EXCLUDES);
      }
    }).catch(() => undefined);

    for await (const batch of changes.stream()) {
      const paths = [...new Set(batch.filter((change) => change !== EXCLUDES))];
      if (batch.includes(EXCLUDES) || paths.some((path) => basename(path) === ".gitignore")) {
        trace(`ignore rules changed in ${root}`);
        await watchTree();
        yield null;
        continue;
      }

      // Only a snapshot of what git ignores is left out, so a folder it ignores that's new since,
      // like fresh build output, is still watched. Changes in it don't show, so they're dropped.
      const dropped = await ignoredAmong(repo, paths);
      const shown = paths.filter((path) => !dropped.has(path));
      trace(
        `working tree changed in ${root}: ${shown.length} files (${shown.slice(0, 5).join(", ")})` +
          (dropped.size ? `, ${dropped.size} ignored` : ""),
      );
      if (shown.length > 0) yield null;
      if (dropped.size > 0) await leaveOutNewIgnoredFolders([...dropped]);
    }
  } finally {
    closed = true;
    changes.close();
    await Promise.all([tree?.unsubscribe(), excludes?.unsubscribe()]);
  }

  /**
   * Starts over without the folders `dropped` (ignored paths that changed) are in, if they weren't
   * left out yet, so their changes stop coming in. Not for single files, which are dropped as they
   * come: they're cheap, and starting over walks the whole tree on Linux.
   */
  async function leaveOutNewIgnoredFolders(dropped: string[]) {
    const nowIgnored = await ignoredPaths(repo);
    const newFolders = nowIgnored.filter(
      (path) => !ignored.has(path) && dropped.some((changed) => changed.startsWith(`${path}/`)),
    );
    if (newFolders.length === 0) return;
    trace(`leaving out new ignored folders in ${root}: ${newFolders.join(", ")}`);
    await watchTree(nowIgnored);
  }
}

/** Which of `paths` (relative, `/`-separated) git ignores; tracked files never are. */
async function ignoredAmong(repo: Repo, paths: string[]): Promise<Set<string>> {
  if (paths.length === 0) return new Set();
  // Exits with 1 when none are; on any failure, nothing's dropped. check-ignore refuses literal
  // pathspecs, but takes its paths as plain paths anyway.
  const output = await repo
    .read(["check-ignore", "-z", "--stdin"], {
      stdin: `${paths.join("\0")}\0`,
      env: { GIT_LITERAL_PATHSPECS: "0" },
    })
    .catch(() => "");
  return new Set(output.split("\0").filter(Boolean));
}

/**
 * What git ignores right now, e.g. `node_modules` or build output: whole directories (listed once,
 * with --directory) and single files. Relative to the repository, `/`-separated.
 */
export async function ignoredPaths(repo: Repo): Promise<string[]> {
  const output = await repo
    .read(["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"])
    .catch(() => "");
  return output
    .split("\0")
    .filter(Boolean)
    .map((path) => path.replace(/\/$/, ""));
}
