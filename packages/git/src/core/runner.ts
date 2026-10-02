import { spawn, type ChildProcess } from "node:child_process";

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
   * Runs git in a session of its own, without the terminal Gitto may have been started from, so
   * nothing it starts can ask questions there that no one would answer: ssh can't ask for a
   * passphrase or to trust a host, and asks with the user's askpass program instead, if they have
   * one, or fails. (GIT_TERMINAL_PROMPT only covers git's own HTTPS prompts.) Not on Windows,
   * where there's no such terminal to keep away from.
   */
  noTerminal?: boolean;
}

// Settings that keep git's output stable and machine-readable, whatever the user's config says.
const CONFIG = [
  "-c",
  "core.quotepath=false",
  "-c",
  "color.ui=false",
  "-c",
  "log.showSignature=false",
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

/**
 * The git processes running in a session of their own (see `RunOptions.noTerminal`). Out of the
 * terminal's reach, Ctrl-C there doesn't stop them, so they're stopped when Gitto exits or is
 * stopped by a signal. (Not when it's killed outright: they then finish or fail on their own,
 * having no one to wait for.)
 */
const sessions = new Set<ChildProcess>();
const stopSessions = () => {
  for (const child of sessions) child.kill();
};
process.on("exit", stopSessions);
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"] as const) {
  process.once(signal, () => {
    stopSessions();
    // Handled as it would have been without this listener (once, so it's gone by now).
    process.kill(process.pid, signal);
  });
}

/** Runs `git` in `cwd` and resolves to its stdout; rejects with a `GitError` (or a more specific
 * subclass, see `commandError`) on a non-zero exit. */
export function runGit(cwd: string, args: string[], options: RunOptions = {}): Promise<string> {
  const start = tracing ? performance.now() : 0;
  return new Promise((resolve, reject) => {
    const config = (options.config ?? []).flatMap((setting) => ["-c", setting]);
    const child = spawn("git", [...CONFIG, ...config, ...args], {
      cwd,
      env: { ...process.env, ...ENV, ...options.env },
      signal: options.signal,
      stdio: ["pipe", "pipe", "pipe"],
      detached: options.noTerminal && process.platform !== "win32",
    });
    if (options.noTerminal) {
      sessions.add(child);
      child.on("close", () => sessions.delete(child));
    }

    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
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
      const out = Buffer.concat(stdout).toString("utf8");
      const err = Buffer.concat(stderr).toString("utf8");
      if (tracing) {
        const ms = (performance.now() - start).toFixed(0);
        trace(`${ms.padStart(5)}ms ${String(out.length).padStart(8)}B  git ${args.join(" ")}`);
      }
      if (code === 0) {
        resolve(out);
      } else {
        reject(commandError(cwd, args, code, out, err));
      }
    });

    // Git can exit before reading all of stdin, e.g. when the index is locked; writing the rest
    // then fails with EPIPE. Its exit code and stderr already say why, so that's not reported.
    child.stdin.on("error", () => undefined);
    child.stdin.end(options.stdin);
  });
}

/**
 * Writes each repository's commit-graph, once per run of the app. With it, git looks commits up
 * instead of parsing them, which makes the history (sorted over all refs) several times faster:
 * 0.35s instead of 2.3s on vscode. Fresh clones don't have one: git only writes it in gc or
 * maintenance.
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
