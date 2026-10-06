import { spawn } from "node:child_process";
import { stat } from "node:fs/promises";

import { commandError, GitError } from "./errors";
import { trace, tracing } from "./trace";

export interface RunOptions {
  signal?: AbortSignal;
  /** Written to git's stdin, e.g. a commit message for `commit -F -`. */
  stdin?: string;
  /** Overrides the environment git runs with (see `ENV`), for a command one doesn't suit. */
  env?: Record<string, string>;
  /** Settings for this command, e.g. `gc.auto=0`, as with `git -c`. */
  config?: string[];
  /**
   * Takes paths as globs, as git does by default, rather than literally (see `ENV`): for a command
   * that takes no paths from the UI but uses pathspecs of its own, like `stash push`.
   */
  globPathspecs?: boolean;
  /**
   * Git's output, and `stdin`, as bytes, one per character, rather than as UTF-8: for paths that
   * are handed back to git as it gave them. A file's name needn't be UTF-8, and one that isn't
   * doesn't survive being read as it.
   */
  binary?: boolean;
  /**
   * The command rewrites files in the working tree, like a checkout: the repository's index is
   * refreshed a little after it's done, or has failed partway (see `IndexRefreshes`).
   */
  rewritesFiles?: boolean;
  /**
   * Stopped (with all it started, like ssh) when Gitto exits, rather than left running in its
   * session (see `runGit`), out of reach: for a command that's safe to stop partway, like a fetch.
   * Others, like a rebase, aren't stopped on purpose, though one that writes output once Gitto
   * has gone stops then, its pipe closed. Without a terminal, none waits on an answer that won't
   * come, so even left behind (when Gitto is stopped by a signal, say) they end.
   */
  stopOnExit?: boolean;
  /**
   * Stops git once its output is over `bytes`, and rejects with `error()`: for output that's sent to
   * the renderer whole, like a file's patch, which can be of a file of any size.
   */
  maxOutput?: { bytes: number; error: () => Error };
}

// Settings that keep git's output stable and machine-readable, whatever the user's config says.
const CONFIG = [
  "-c",
  "core.quotepath=false",
  "-c",
  "color.ui=false",
  "-c",
  "log.showSignature=false",
  // Text is read and written in UTF-8, e.g. commit messages, as JS strings are sent and decoded.
  "-c",
  "i18n.commitEncoding=UTF-8",
  "-c",
  "i18n.logOutputEncoding=UTF-8",
];

const ENV = {
  // Parse-stable, English output.
  LC_ALL: "C",
  // Never block on a tty prompt; there's no terminal to answer it.
  GIT_TERMINAL_PROMPT: "0",
  // Read commands (status, diff) won't refresh the index, so they never take index.lock and
  // collide with git running in the user's terminal.
  GIT_OPTIONAL_LOCKS: "0",
  // Paths from the UI are file names, not globs.
  GIT_LITERAL_PATHSPECS: "1",
  GIT_PAGER: "cat",
};

/** The process groups of the commands to stop when Gitto exits (see `RunOptions.stopOnExit`). */
const stopOnExit = new Set<number>();
process.on("exit", () => {
  for (const group of stopOnExit) stopGroup(group);
});

/** Stops the process group `group`, if it's still there. */
function stopGroup(group: number): void {
  try {
    process.kill(-group);
  } catch {
    // Already gone.
  }
}

/**
 * Runs `git` in `cwd` and resolves to its stdout; rejects with a `GitError` (or a more specific
 * subclass, see `commandError`) on a non-zero exit.
 *
 * Git runs in a session of its own, without the terminal Gitto may have been started from, so
 * nothing it starts can ask questions there that no one would answer and hold up the repository
 * meanwhile: ssh can't ask for a passphrase or to trust a host (it asks with the user's askpass
 * program instead, if they have one, or fails), nor gpg for one to sign with. GIT_TERMINAL_PROMPT
 * only covers git's own HTTPS prompts. Not on Windows, which has no such terminal to keep away
 * from.
 */
export function runGit(cwd: string, args: string[], options: RunOptions = {}): Promise<string> {
  const start = tracing ? performance.now() : 0;
  return new Promise((resolve, reject) => {
    const config = (options.config ?? []).flatMap((setting) => ["-c", setting]);
    const child = spawn("git", [...CONFIG, ...config, ...args], {
      cwd,
      env: {
        ...process.env,
        ...ENV,
        ...(options.globPathspecs ? { GIT_LITERAL_PATHSPECS: "0" } : {}),
        ...options.env,
      },
      signal: options.signal,
      stdio: ["pipe", "pipe", "pipe"],
      detached: process.platform !== "win32",
    });
    // Its own session is its own process group too, numbered after it.
    const group = process.platform !== "win32" ? child.pid : undefined;
    if (group) {
      if (options.stopOnExit) stopOnExit.add(group);
      // On exit rather than close: what git started (ssh, say) can hold its output open.
      child.on("exit", () => {
        stopOnExit.delete(group);
        // Cancelling only stops git: what it started is stopped with its group.
        if (options.signal?.aborted) stopGroup(group);
      });
    }

    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    let stdoutBytes = 0;
    let tooLarge = false;
    child.stdout.on("data", (chunk: Buffer) => {
      if (tooLarge) return;
      stdoutBytes += chunk.length;
      if (options.maxOutput && stdoutBytes > options.maxOutput.bytes) {
        tooLarge = true;
        stdout.length = 0;
        // With what it started, like a textconv filter, as when it's cancelled.
        if (group) stopGroup(group);
        else child.kill();
        return;
      }
      stdout.push(chunk);
    });
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));

    child.on("error", (error: NodeJS.ErrnoException) => {
      // Cancelled (e.g. the renderer no longer needs the result); that's not a git failure.
      if (error.name === "AbortError") {
        reject(error);
        return;
      }
      const message =
        error.code === "ENOENT" ? "Git is not installed or not on the PATH." : error.message;
      reject(new GitError(message, args, null, ""));
    });

    child.on("close", (code) => {
      const out = Buffer.concat(stdout).toString(options.binary ? "latin1" : "utf8");
      const err = Buffer.concat(stderr).toString("utf8");
      if (tracing) {
        const ms = (performance.now() - start).toFixed(0);
        trace(`${ms.padStart(5)}ms ${String(out.length).padStart(8)}B  git ${args.join(" ")}`);
      }
      if (tooLarge) {
        reject(options.maxOutput!.error());
      } else if (code === 0) {
        resolve(out);
      } else {
        reject(commandError(cwd, args, code, out, err));
      }
    });

    // Git can exit before reading all of stdin, e.g. when the index is locked; writing the rest
    // then fails with EPIPE. Its exit code and stderr already say why, so that's not reported.
    child.stdin.on("error", () => undefined);
    if (options.stdin === undefined) child.stdin.end();
    else child.stdin.end(options.stdin, options.binary ? "latin1" : "utf8");
  });
}

/**
 * Writes each repository's commit-graph, once per run of the app. With it, git looks commits up
 * instead of parsing them, which makes sorting the history over all refs, when it needs that (see
 * `getLog`), several times faster: 0.5s instead of 3.4s on vscode. Fresh clones don't have one:
 * git only writes it in gc or maintenance.
 *
 * Split, so it only adds the commits that aren't in it yet (in ~30ms when there are none), and
 * not through `WriteQueue`: it doesn't touch the index or refs, and can take seconds the first
 * time, which a commit shouldn't wait for. It's only a cache, so a failure (another git writing
 * it at the same time, a read-only repository) is left for next time.
 */
export class CommitGraphs {
  private readonly writes = new Map<string, Promise<void>>();
  /** Writes `path`'s commit-graph if that hasn't been done yet; resolves once it's written. */
  update(path: string): Promise<void> {
    let write = this.writes.get(path);
    if (!write) {
      write = runGit(path, ["commit-graph", "write", "--reachable", "--split", "--no-progress"])
        .then(() => trace(`wrote the commit-graph of ${path}`))
        .catch((error: unknown) => trace(`couldn't write the commit-graph of ${path}: ${error}`));
      this.writes.set(path, write);
    }
    return write;
  }
}

/**
 * How long after a write that rewrote files the index is refreshed: git compares the files' and the
 * index's timestamps in whole seconds.
 */
const REFRESH_INDEX_AFTER_MS = 1000;

/** A status that takes this long may be reading every file again (see `IndexRefreshes`). */
const SLOW_STATUS_MS = 500;

/**
 * With this many files the index can't vouch for, a slow first status is put down to them. Fewer
 * are always there after a commit, say, and are read in no time: 0.1ms each in vscode.
 */
const MANY_UNSURE_FILES = 1000;

/**
 * Refreshes each repository's index after a write that rewrote files in the working tree, like a
 * checkout. Git can't tell whether a file written in the same second as the index changed since,
 * so it reads it again, in full, whenever it compares the working tree with the index, until the
 * index is written in a later second. Gitto's reads never write it (see `ENV`), so every `status`
 * did: 0.5s instead of 80ms on vscode after switching to a branch 3,000 commits back.
 *
 * Through `WriteQueue`, as it takes the index's lock, once a second has passed without another such
 * write. It's only a cache, so a failure (the user's git holding the lock, say) is left alone.
 *
 * A fresh clone is in that state too: 19,939 files written in the index's second in vscode, and
 * 1s rather than 25ms for every status. So the index is also refreshed when a repository's first
 * status of this run was slow and the index has many such files. Only then: it holds the lock for
 * as long as that status took, at a moment nobody asked Gitto to write, and a git command run in
 * a terminal meanwhile fails on it. So not for a status that was slow for another reason, like
 * files that aren't in the disk's cache yet, and a checkout made in a terminal later is left to
 * the next write there or here.
 */
export class IndexRefreshes {
  private readonly timers = new Map<string, NodeJS.Timeout>();
  /** The repositories whose first status of this run has been timed. */
  private readonly timed = new Set<string>();

  constructor(private readonly writes: WriteQueue) {}

  schedule(path: string): void {
    clearTimeout(this.timers.get(path));
    const timer = setTimeout(() => {
      this.timers.delete(path);
      void this.writes
        // `-q` and `--unmerged`: files that did change, and conflicts, aren't errors.
        .run(path, () => runGit(path, ["update-index", "-q", "--unmerged", "--refresh"]))
        .then(
          () => trace(`refreshed the index of ${path}`),
          (error: unknown) => trace(`couldn't refresh the index of ${path}: ${error}`),
        );
    }, REFRESH_INDEX_AFTER_MS);
    // Not worth keeping the app from exiting for.
    timer.unref();
    this.timers.set(path, timer);
  }

  /**
   * Schedules a refresh if this status, which took `ms`, is `path`'s first, was slow, and the
   * index has many files it can't vouch for.
   */
  statusTook(path: string, ms: number): void {
    if (this.timed.has(path)) return;
    this.timed.add(path);
    if (ms < SLOW_STATUS_MS) return;
    void this.refreshUnsure(path, ms);
  }

  /** Schedules a refresh if `path`'s index has many files it can't vouch for. */
  private async refreshUnsure(path: string, ms: number): Promise<void> {
    // The repository is gone, say: nothing to refresh.
    const count = await unsureFiles(path).catch(() => 0);
    trace(`the first status of ${path} took ${ms.toFixed(0)}ms, with ${count} unsure files`);
    if (count >= MANY_UNSURE_FILES) this.schedule(path);
  }
}

/**
 * How many of the index's files it can't vouch for: the ones it has as last written in the second
 * it was written itself, or later.
 */
async function unsureFiles(path: string): Promise<number> {
  const [entries, index] = await Promise.all([
    runGit(path, ["ls-files", "--debug"]),
    runGit(path, ["rev-parse", "--path-format=absolute", "--git-path", "index"]),
  ]);
  const written = Math.floor((await stat(index.trimEnd())).mtimeMs / 1000);
  let count = 0;
  // Each file's name, then what the index knows of it, indented: `  mtime: <seconds>:<nanoseconds>`.
  for (const [, seconds] of entries.matchAll(/^ {2}mtime: (\d+):/gm)) {
    if (Number(seconds) >= written) count++;
  }
  return count;
}

/**
 * A snapshot (see `statusSnapshot`) of each repository's status as the UI last got it. Git also
 * writes the index when nothing in it changes (another tool's `git status` refreshes it), and after
 * Gitto's own writes the UI already refetches the status itself: comparing the status with the one
 * the UI has tells those apart from a change it hasn't got yet (see `watchGitDir`).
 */
export class ShownStatuses {
  private readonly latest = new Map<string, Promise<string | undefined>>();

  /** Remembers `snapshot` as `path`'s; it resolves to `undefined` if the UI didn't get the status. */
  set(path: string, snapshot: Promise<string | undefined>): void {
    this.latest.set(path, snapshot);
  }

  /** `path`'s latest snapshot, once it's there: a status that's being read is waited for. */
  async get(path: string): Promise<string | undefined> {
    for (;;) {
      const latest = this.latest.get(path);
      // oxlint-disable-next-line no-await-in-loop -- whichever is the latest by then.
      const snapshot = await latest;
      if (this.latest.get(path) === latest) return snapshot;
    }
  }
}

/**
 * Runs write commands for a repository one at a time. Reads don't go through here: with
 * GIT_OPTIONAL_LOCKS=0 they take no locks, so they can run alongside anything.
 */
export class WriteQueue {
  private readonly tails = new Map<string, Promise<unknown>>();

  run<T>(key: string, task: () => Promise<T>): Promise<T> {
    const previous = this.tails.get(key) ?? Promise.resolve();
    const next = previous.then(task, task);
    const tail = next.catch(() => undefined);
    this.tails.set(key, tail);
    void tail.finally(() => {
      if (this.tails.get(key) === tail) this.tails.delete(key);
    });
    return next;
  }
}
