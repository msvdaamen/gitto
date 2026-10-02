import { realpath } from "node:fs/promises";
import { basename, dirname } from "node:path";

import { subscribe, type AsyncSubscription } from "@parcel/watcher";

import type { Repo } from "../../core/repo";
import { trace } from "../../core/trace";
import { Batches } from "./batches";
import { classify, gitDirs, indexSignature, inside } from "./git-dirs";
import type { GitDirChange, TreeEvent } from "./schema";

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
  // The index as Gitto's own last write left it. While it's still that, a change to it was that
  // write, which the UI refetches after by itself; reporting it would refetch the same thing again.
  let ownIndex: string | undefined;
  const stopListening = repo.onWrite(async () => {
    ownIndex = await indexSignature(dirs);
  });
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
      const own =
        ownIndex !== undefined &&
        batch.includes("index") &&
        (await indexSignature(dirs)) === ownIndex;
      const reported = own ? batch.filter((change) => change !== "index") : batch;
      trace(
        `git dir changed in ${repo.path}: ${reported.join(", ")}` +
          (own ? " (index: Gitto's own write, not reported)" : ""),
      );
      if (reported.length > 0) yield reported;
    }
  } finally {
    stopListening();
    changes.close();
    await Promise.all(subscriptions.map((subscription) => subscription.unsubscribe()));
  }
}

/** A changed path in the working tree (relative, `/`-separated), or `.git/info/exclude` changing. */
type TreeChange = string | typeof EXCLUDES;
const EXCLUDES = Symbol("excludes");

/**
 * Yields `ready` once it's watching, then `changed` (debounced) whenever a file in the working tree
 * changes, until `signal` aborts.
 */
export async function* watchWorkingTree(
  repo: Repo,
  signal?: AbortSignal,
): AsyncGenerator<TreeEvent> {
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
          const path = inside(root, event.path);
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
    yield "ready";

    for await (const batch of changes.stream()) {
      const paths = batch.filter((change) => change !== EXCLUDES);
      if (batch.includes(EXCLUDES) || paths.some((path) => basename(path) === ".gitignore")) {
        trace(`ignore rules changed in ${root}`);
        await watchTree();
        yield "changed";
        continue;
      }
      if (paths.length === 0) continue;

      // Only a snapshot of what git ignores is left out, so a folder it ignores that's new since,
      // like fresh build output, is still watched. Changes in it don't show, so they're dropped.
      // Asking git takes a while per path, and thousands can change at once (a checkout, a
      // formatter), so it's asked about a few at a time, until one turns out to show: that's all
      // it takes to report the batch.
      let shown: string | undefined;
      // Paths git said it ignores: what a new ignored folder could be among. The ones it wasn't
      // asked about are left out; if a new ignored folder hides among them, the next batch with
      // only its changes finds it.
      const ignoredNow: string[] = [];
      for (const some of growingChunks(paths)) {
        // oxlint-disable-next-line no-await-in-loop -- one at a time: the next is only needed if this one shows nothing
        const dropped = await ignoredAmong(repo, some);
        // Not spread into `push`: there can be more than a call takes arguments.
        for (const path of dropped) ignoredNow.push(path);
        shown = some.find((path) => !dropped.has(path));
        if (shown !== undefined) break;
      }
      if (shown !== undefined) {
        trace(`working tree changed in ${root}: ${paths.length} paths (${shown}, …)`);
        yield "changed";
      } else {
        trace(`${paths.length} ignored paths changed in ${root}`);
      }
      if (ignoredNow.length > 0) await leaveOutNewIgnoredFolders(ignoredNow);
    }
  } finally {
    closed = true;
    changes.close();
    await Promise.all([tree?.unsubscribe(), excludes?.unsubscribe()]);
  }

  /**
   * Starts over without the ignored folders that `changed` paths are in, if they weren't left out
   * yet, so their changes stop coming in. Not for single files, which are dropped as they come:
   * they're cheap, and starting over walks the whole tree on Linux.
   */
  async function leaveOutNewIgnoredFolders(changed: string[]) {
    const nowIgnored = await ignoredPaths(repo);
    const folders = foldersOf(changed);
    const newFolders = nowIgnored.filter((path) => !ignored.has(path) && folders.has(path));
    if (newFolders.length === 0) return;
    trace(`leaving out new ignored folders in ${root}: ${newFolders.join(", ")}`);
    await watchTree(nowIgnored);
  }
}

/** How many paths git is asked about first; 100 take it about 15ms. */
const FIRST_CHUNK = 100;

/** `paths` in chunks, each four times the size of the one before. */
function* growingChunks(paths: string[]): Generator<string[]> {
  for (let start = 0, size = FIRST_CHUNK; start < paths.length; start += size, size *= 4) {
    yield paths.slice(start, start + size);
  }
}

/** Every folder `paths` (`/`-separated) are in, at any depth. */
function foldersOf(paths: string[]): Set<string> {
  const folders = new Set<string>();
  for (const path of paths) {
    for (let end = path.lastIndexOf("/"); end > 0; end = path.lastIndexOf("/", end - 1)) {
      const folder = path.slice(0, end);
      // Its parents were added along with it.
      if (folders.has(folder)) break;
      folders.add(folder);
    }
  }
  return folders;
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
